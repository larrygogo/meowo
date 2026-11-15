//! 真机走查：终端输入框里有草稿时，按对话页发送的那套做法把草稿收起、把消息发出去。
//!
//! 用的是生产规则：插件声明的那一份输入框规格（`AgentPlugin::composer`）和宿主的认框实现
//! （`meowo_agent::composer::read`，文字与光标取自同一帧）。按键顺序与前端
//! `views/chat/composer.ts` 的 `prepareComposer` 一致：认出有字才按清空键（或暂存键），每按一轮复核。
//!
//! 走的步骤（每一步打印结论，失败即断言）：
//! 1. 起 CLI，过掉启动询问，等到输入框认得出、是空的；
//! 2. 在终端输入框里打两行草稿，确认读出来的就是这两行；
//! 3. 收起草稿（清空并记下内容，或暂存），确认输入框空了、CLI 还活着；
//! 4. 写正文、隔一会儿回车，等到回答出现（`17+25` → `42`）——消息送达；
//! 5. 回合进行中再发一条（用插件声明的排队键），等它也被回答（`310+470` → `780`，数数的输出里不会有这个数）；
//! 6. 有暂存的（claude）确认草稿在提交后回到了输入框；其余确认第 3 步记下的内容与草稿一致。
//!
//! 会真的调模型，提示词都是一句话。`PROBE_ARGS` 追加启动参数（如 opencode 的 `--model`）；
//! `PROBE_NO_MODEL=1` 只走到第 4 步的「消息已提交」（上游服务不可用时用）；`PROBE_VERSION` 指定版本
//! （npm 垫片装的 CLI 探测不到版本时用）。
//!
//! ```text
//! PROBE_AGENT=codex cargo test -p meowo-app --test probe_composer_draft -- --ignored --nocapture
//! ```

use meowo_agent::composer::{read, ComposerRead, Cursor};
use portable_pty::{native_pty_system, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod common;

const ROWS: u16 = 40;
const COLS: u16 = 120;

fn use_bundled_conpty() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/conpty");
    let target = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    for name in ["conpty.dll", "OpenConsole.exe"] {
        let from = source.join(name);
        assert!(
            from.exists(),
            "缺 {}，先跑 scripts/fetch-conpty.mjs",
            from.display()
        );
        if !target.join(name).exists() {
            std::fs::copy(&from, target.join(name)).unwrap();
        }
    }
}

struct Tui {
    screen: Arc<Mutex<vt100::Parser>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Tui {
    fn lines(&self) -> Vec<String> {
        let parser = self.screen.lock().unwrap();
        let grid = parser.screen();
        let (_, cols) = grid.size();
        grid.rows(0, cols).collect()
    }
    fn text(&self) -> String {
        self.lines().join("\n")
    }
    /// 与生产的 `PtyBroker::screen_frame` 同口径：文字、光标、光标所在格是否暗色取自同一刻。
    fn composer(&self, spec: &meowo_agent::chat_ui::ComposerSpec) -> Option<ComposerRead> {
        let parser = self.screen.lock().unwrap();
        let grid = parser.screen();
        let (_, cols) = grid.size();
        let lines: Vec<String> = grid.rows(0, cols).collect();
        let (row, col) = grid.cursor_position();
        let cursor = Cursor {
            row: row as usize,
            col: col as usize,
            visible: !grid.hide_cursor(),
            dim: grid
                .cell(row, col)
                .is_some_and(|cell| cell.dim() && !cell.contents().trim().is_empty()),
        };
        read(&lines, Some(cursor), spec)
    }
    fn send(&self, data: &str) {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(data.as_bytes()).unwrap();
        writer.flush().unwrap();
    }
    fn wait(&self, what: &str, secs: u64, mut ready: impl FnMut(&Tui) -> bool) -> bool {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(secs) {
            if ready(self) {
                println!(
                    "  ✓ {what}（{:.1}s）",
                    started.elapsed().as_secs_f32()
                );
                return true;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        println!("  ✗ {what}：{secs}s 内没等到");
        self.dump();
        false
    }
    fn dump(&self) {
        println!("  ───── 画面 ─────");
        for line in self.lines().iter().filter(|l| !l.trim().is_empty()) {
            println!("  |{}", line.trim_end());
        }
    }
}

fn pause(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

#[test]
#[ignore = "启动真实 CLI 并调用模型；真机走查用"]
fn draft_is_put_away_and_the_message_lands() {
    use_bundled_conpty();
    let agent = std::env::var("PROBE_AGENT").expect("PROBE_AGENT");
    let plugin = meowo_agent::by_id(&agent).unwrap();
    let mut argv = plugin.launch_argv();
    if let Ok(extra) = std::env::var("PROBE_ARGS") {
        argv.extend(extra.split_whitespace().map(String::from));
    }
    let temp = std::env::temp_dir().join(format!("meowo-composer-probe-{}", std::process::id()));
    std::fs::create_dir_all(temp.join(".gemini")).unwrap();
    // gemini 关自更新只有 settings 一条路；项目级覆盖用户级，不改用户装机。
    std::fs::write(
        temp.join(".gemini/settings.json"),
        r#"{"general":{"enableAutoUpdate":false,"enableAutoUpdateNotification":false}}"#,
    )
    .unwrap();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: ROWS,
            cols: COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = common::agent_command(&argv[0], &temp);
    command.args(&argv[1..]);
    command.env("OPENCODE_DISABLE_AUTOUPDATE", "1");
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let writer: Arc<Mutex<Box<dyn Write + Send>>> =
        Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
    let screen = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLS, 0)));
    let (answer, parser) = (writer.clone(), screen.clone());
    std::thread::spawn(move || {
        let mut bytes = [0; 16_384];
        while let Ok(count) = reader.read(&mut bytes) {
            if count == 0 {
                break;
            }
            let chunk = &bytes[..count];
            let mut writer = answer.lock().unwrap();
            if chunk.windows(4).any(|part| part == b"\x1b[6n") {
                let _ = writer.write_all(b"\x1b[1;1R").and_then(|_| writer.flush());
            }
            if chunk.windows(3).any(|part| part == b"\x1b[c") {
                let _ = writer
                    .write_all(b"\x1b[?62;22c")
                    .and_then(|_| writer.flush());
            }
            drop(writer);
            parser.lock().unwrap().process(chunk);
        }
    });
    let tui = Tui { screen, writer };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| walk(&agent, &tui)));
    // 成败都收掉我们拉起的 CLI，别留进程。
    let _ = child.kill();
    let _ = child.wait();
    drop(pair.master);
    let _ = std::fs::remove_dir_all(&temp);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

fn walk(agent: &str, tui: &Tui) {
    let plugin = meowo_agent::by_id(agent).unwrap();
    // 版本探测失败（npm 的 .cmd 垫片起不来之类）时用 PROBE_VERSION 指定。
    let version = std::env::var("PROBE_VERSION").unwrap_or_else(|_| cli_version(agent));
    let spec = plugin
        .composer(Some(&version))
        .unwrap_or_else(|| panic!("{agent} {version} 没有输入框规格"));
    println!("===== {agent} {version} =====");

    // 1. 过掉启动询问，等到输入框认得出、是空的。询问各家不同，只处理本机见过的三种。
    let mut handled = std::collections::HashSet::new();
    assert!(tui.wait("输入框认得出，是空的", 90, |tui| {
        let text = tui.text().to_lowercase();
        if text.contains("update available") && text.contains("skip") && handled.insert("update") {
            // codex 的更新询问：Esc 跳过（回车是「立即更新」）。
            tui.send("\x1b");
        } else if text.contains("trust this folder?") && handled.insert("kimi-trust") {
            // kimi：默认项就是 Trust and continue。
            tui.send("\r");
        } else if text.contains("yes, i trust this folder") && handled.insert("claude-trust") {
            // claude：默认项是 No, exit，下移一格再回车。询问刚画出来时还不收键，等一下再按。
            pause(1000);
            tui.send("\x1b[B");
            pause(500);
            tui.send("\r");
        }
        tui.composer(&spec).is_some_and(|read| read.empty)
    }));
    pause(1500);

    // 2. 在终端输入框里打两行草稿。
    let newline = plugin.newline_input().unwrap_or("\n");
    tui.send("meowo draft one");
    pause(300);
    tui.send(newline);
    pause(300);
    tui.send("meowo draft two");
    assert!(tui.wait("草稿读得出来", 10, |tui| {
        tui.composer(&spec)
            .is_some_and(|read| !read.empty && read.text.contains("meowo draft two"))
    }));
    let draft = tui.composer(&spec).unwrap().text;
    println!("  草稿原文：{draft:?}");
    assert!(draft.contains("meowo draft one") && draft.contains("meowo draft two"));

    // 3. 收起草稿：有暂存键用暂存，否则按清空键逐轮清（与 prepareComposer 同序）。
    let moved = put_away(tui, &spec);
    assert!(
        tui.composer(&spec).is_some_and(|read| read.empty),
        "收起之后输入框应该是空的"
    );
    println!(
        "  ✓ 草稿已收起：{}",
        if moved.is_some() { "清空，内容带回对话页" } else { "暂存在 CLI 里" }
    );
    if let Some(text) = &moved {
        assert_eq!(text, &draft, "带回对话页的内容要与草稿一致");
    }

    // 4. 写正文、隔一会儿回车（正文与回车分两次写，见 terminalSubmit）。
    tui.send("What is 17+25? Reply with just the number.");
    pause(500);
    tui.send("\r");
    // 上游服务不可用时（本机 Gemini 的接口 2026-11-14 持续返回 503）只核对到「消息已提交」：
    // 正文离开输入框、进了对话区、CLI 开始处理。后面的步骤要模型作答，跳过。
    if std::env::var("PROBE_NO_MODEL").is_ok() {
        assert!(tui.wait("消息已提交（输入框空了，正文进了对话区）", 30, |tui| {
            tui.composer(&spec).is_some_and(|read| read.empty) && tui.text().contains("17+25")
        }));
        println!("===== {agent}：通过（只到消息提交，未等模型作答） =====");
        return;
    }
    assert!(tui.wait("第一条消息得到回答（42）", 120, |tui| answered(&tui.text(), "17+25", "42")));

    // 5. 回合进行中再发一条：先起一个要跑一阵的回合，再用排队键提交第二条。
    //    暂存的草稿（claude）在上一条提交后已经回到输入框：对话页每次发送前都会再收一次，这里照做。
    assert!(tui.wait("回到空闲", 60, |tui| {
        tui.composer(&spec)
            .is_some_and(|read| read.empty || read.text.contains("meowo draft two"))
    }));
    pause(1500);
    if moved.is_none() {
        println!("  ✓ 暂存的草稿在提交后回到了输入框，发下一条前再收一次");
        put_away(tui, &spec);
    }
    tui.send("Count from 1 to 150, one number per line, no other text.");
    pause(500);
    tui.send("\r");
    pause(3000);
    //    回合进行中终端里又有草稿（暂存的那份回来了）：同样先收起再写。
    if moved.is_none() && tui.composer(&spec).is_some_and(|read| !read.empty) {
        put_away(tui, &spec);
    }
    tui.send("What is 310+470? Reply with just the number.");
    pause(500);
    tui.send(plugin.queue_input().unwrap_or("\r"));
    assert!(tui.wait("运行中发的第二条得到回答（780）", 180, |tui| answered(&tui.text(), "310+470", "780")));

    // 6. 暂存的草稿在提交后回到输入框；清空的那份已经在第 3 步核对过。
    if moved.is_none() {
        assert!(tui.wait("暂存的草稿回到了输入框", 30, |tui| {
            tui.composer(&spec)
                .is_some_and(|read| read.text.contains("meowo draft two"))
        }));
    }
    println!("===== {agent}：通过 =====");
}

/// 回答出现了：在题目那一行之后，有一行去掉行首的标记（`●`、`✦`、`•` 之类）后正好是答案。
/// 只看「屏幕上有没有这个数」不行：临时目录名、数数的输出里都可能带着它。
fn answered(screen: &str, question: &str, answer: &str) -> bool {
    let Some(at) = screen.rfind(question) else {
        return false;
    };
    screen[at..].lines().skip(1).any(|line| {
        line.trim()
            .trim_start_matches(|c: char| !c.is_ascii_alphanumeric())
            .trim()
            == answer
    })
}

/// 收起草稿。返回 Some(内容) = 清空并带回；None = 暂存在 CLI 里。
fn put_away(tui: &Tui, spec: &meowo_agent::chat_ui::ComposerSpec) -> Option<String> {
    let before = tui.composer(spec).expect("认得出输入框");
    assert!(!before.empty, "有草稿才收");
    if let Some(stash) = spec.stash {
        tui.send(stash.input);
        assert!(tui.wait("暂存后输入框空了", 5, |tui| tui.composer(spec).is_some_and(|read| read.empty)));
        return None;
    }
    assert!(!spec.clear_inputs.is_empty(), "没有取证过的清空键");
    for round in 0..spec.clear_rounds.max(1) {
        if tui.composer(spec).is_some_and(|read| read.empty) {
            break;
        }
        for (index, input) in spec.clear_inputs.iter().enumerate() {
            if index > 0 {
                pause(if spec.clear_rounds > 1 { 60 } else { 300 });
                if tui.composer(spec).is_some_and(|read| read.empty) {
                    break;
                }
            }
            tui.send(input);
        }
        pause(400);
        println!("  第 {} 轮清空后：{:?}", round + 1, tui.composer(spec).map(|read| read.text));
    }
    Some(before.text)
}

fn cli_version(agent: &str) -> String {
    let exe = meowo_agent::by_id(agent).unwrap().launch_argv()[0].clone();
    let output = std::process::Command::new(&exe)
        .arg("--version")
        .output()
        .unwrap_or_else(|error| panic!("{exe} --version 失败：{error}"));
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    // 取第一段像版本号的东西（`codex-cli 0.167.2`、`2.1.325 (Claude Code)`）。
    text.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find(|part| part.matches('.').count() >= 2)
        .unwrap_or("0.0.0")
        .to_string()
}
