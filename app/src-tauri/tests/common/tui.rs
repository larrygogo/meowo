//! 真机走查探针共用的终端：拉起真实 CLI、按 vt100 重建屏幕、按生产的输入框规格读框。
//!
//! 只给新写的走查探针用（`#[path = "common/tui.rs"] mod tui;`）。老探针各带一份自己的，不动它们。
// 每个探针只用到其中一部分，没用到的不该报警。
#![allow(dead_code)]

use meowo_agent::composer::{read_with_cols, ComposerRead, Cursor};
use portable_pty::{native_pty_system, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const ROWS: u16 = 40;
pub const COLS: u16 = 120;

/// 探针终端的尺寸（行，列）。默认 120×40；`PROBE_ROWS` / `PROBE_COLS` 可改。
///
/// 为什么要能改：生产里终端列数跟着窗口变，而草稿保真的折行判定依赖一行能放多少格。只在 120 列
/// 取证的话，别的宽度下判定对不对没有证据（独立审查 2026-11-15 指出，见 probe_draft_fidelity 的
/// wrap 形态）。
pub fn size() -> (u16, u16) {
    let read = |key: &str, default: u16| {
        std::env::var(key)
            .ok()
            .and_then(|raw| raw.trim().parse().ok())
            .unwrap_or(default)
    };
    (read("PROBE_ROWS", ROWS), read("PROBE_COLS", COLS))
}

pub fn use_bundled_conpty() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/conpty");
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

pub struct Tui {
    screen: Arc<Mutex<vt100::Parser>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Tui {
    /// `env`：追加或覆盖的环境变量。外层会话（本探针常跑在 Meowo 托管的会话里）带进来的
    /// `MEOWO_PTY_*` 一律先摘掉：那是外层会话的审批通道与认领凭据，探针拉起的 CLI 不能拿去用。
    /// 外层 claude 会话留下的 `CLAUDE*` 标记也摘掉，只留 `CLAUDE_CONFIG_DIR`（登录态在那个目录里）：
    /// 带着 `CLAUDE_CODE_CHILD_SESSION` 起来的 claude 把自己当成子会话，不写会话记录
    /// （2.1.325 状态栏原文 `Transcript saving is off — inherited CLAUDE_CODE_CHILD_SESSION marker`），
    /// 与 Meowo 拉起的会话不是一回事。
    pub fn spawn(argv: &[String], cwd: &Path, env: &[(&str, String)]) -> Tui {
        let (rows, cols) = size();
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = crate::common::agent_command(&argv[0], cwd);
        command.args(&argv[1..]);
        // codex 有新版时启动先弹更新询问，回车就是「立即更新」（会改用户装机）。`wait_ready` 认得它、
        // 按 Esc 跳过，但不把装机安全押在「每次都认得出」上：直接关掉启动时的更新检查
        // （与 probe_codex_backtrack、tms_probe_codex 同一个配置键）。
        if argv.iter().any(|arg| arg.to_ascii_lowercase().contains("codex")) {
            command.args(["-c", "check_for_update_on_startup=false"]);
        }
        for (key, _) in std::env::vars() {
            if key.starts_with("MEOWO_PTY_")
                || (key.starts_with("CLAUDE") && key != "CLAUDE_CONFIG_DIR")
            {
                command.env_remove(key);
            }
        }
        // opencode 默认会在会话里静默自更新，取证不得改用户装机。
        command.env("OPENCODE_DISABLE_AUTOUPDATE", "1");
        for (key, value) in env {
            command.env(key, value);
        }
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
        let screen = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
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
        Tui {
            screen,
            writer,
            child,
            _master: pair.master,
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let parser = self.screen.lock().unwrap();
        let grid = parser.screen();
        let (_, cols) = grid.size();
        grid.rows(0, cols).collect()
    }
    pub fn text(&self) -> String {
        self.lines().join("\n")
    }
    /// 去掉行尾空白与首尾空行的整屏，存样本用。
    pub fn trimmed(&self) -> Vec<String> {
        let lines: Vec<String> = self
            .lines()
            .iter()
            .map(|line| line.trim_end().to_string())
            .collect();
        let first = lines.iter().position(|l| !l.is_empty()).unwrap_or(0);
        let last = lines.iter().rposition(|l| !l.is_empty()).map_or(0, |i| i + 1);
        lines[first..last.max(first)].to_vec()
    }
    /// 与生产的 `PtyBroker::screen_frame` 同口径：文字、光标、光标所在格是否暗色取自同一刻。
    pub fn composer(&self, spec: &meowo_agent::chat_ui::ComposerSpec) -> Option<ComposerRead> {
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
        read_with_cols(&lines, Some(cursor), Some(cols as usize), spec)
    }
    /// 在同一把锁里读仿真屏幕的任意状态（折行标记、粘贴模式、备用屏），取证用。
    pub fn with_screen<R>(&self, read: impl FnOnce(&vt100::Screen) -> R) -> R {
        read(self.screen.lock().unwrap().screen())
    }
    pub fn send(&self, data: &str) {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(data.as_bytes()).unwrap();
        writer.flush().unwrap();
    }
    /// 正文与回车分两次写（见前端 terminalSubmit：合成一次写会被一些 TUI 当成粘贴）。
    pub fn submit(&self, text: &str) {
        self.send(text);
        pause(500);
        self.send("\r");
    }
    pub fn wait(&self, what: &str, secs: u64, mut ready: impl FnMut(&Tui) -> bool) -> bool {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(secs) {
            if ready(self) {
                println!("  ✓ {what}（{:.1}s）", started.elapsed().as_secs_f32());
                return true;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        println!("  ✗ {what}：{secs}s 内没等到");
        self.dump();
        false
    }
    pub fn dump(&self) {
        println!("  ───── 画面 ─────");
        for line in self.lines().iter().filter(|l| !l.trim().is_empty()) {
            println!("  |{}", line.trim_end());
        }
    }
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }
    /// 成败都收掉拉起的 CLI：先杀它的后代（npm 垫片起的 node、MCP 子进程），再杀它自己。
    pub fn kill(&mut self) {
        if let Some(pid) = self.child.process_id() {
            kill_tree(pid);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Windows 上 `child.kill()` 只杀直接子进程；npm 垫片（gemini、opencode 是 `node …`）和 CLI 自己起的
/// MCP 服务会留下来。`taskkill /T` 连同后代一起收。
fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
    }
}

pub fn pause(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

/// 过掉启动询问，等到输入框认得出、是空的。询问各家不同，只处理本机见过的三种。
pub fn wait_ready(tui: &Tui, spec: &meowo_agent::chat_ui::ComposerSpec) {
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
        tui.composer(spec).is_some_and(|read| read.empty)
    }));
    pause(1500);
}

pub fn cli_version(agent: &str) -> String {
    if let Ok(version) = std::env::var("PROBE_VERSION") {
        return version;
    }
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

/// 探针的工作目录：一次性的，带 gemini 的项目级设置（关自更新只有 settings 一条路，
/// 项目级覆盖用户级，不改用户装机）。
pub fn temp_cwd(tag: &str) -> std::path::PathBuf {
    let temp = std::env::temp_dir().join(format!("meowo-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(temp.join(".gemini")).unwrap();
    std::fs::write(
        temp.join(".gemini/settings.json"),
        r#"{"general":{"enableAutoUpdate":false,"enableAutoUpdateNotification":false}}"#,
    )
    .unwrap();
    temp
}

/// kimi 把本会话的权限档切到 Always Ask。每一步都先在屏幕上认出来才按键。
pub fn kimi_always_ask(tui: &Tui, spec: &meowo_agent::chat_ui::ComposerSpec) {
    tui.submit("/permission");
    assert!(tui.wait("权限菜单打开", 15, |tui| tui.text().contains("Select permission mode")));
    pause(500);
    const LABELS: [&str; 3] = ["Always Ask", "Ask When Needed", "Never Ask"];
    // 菜单里每一档占两行（名称、说明），`❯` 标在名称行上。
    let focused = |tui: &Tui| -> Option<usize> {
        let lines = tui.lines();
        let line = lines.iter().find(|line| line.trim_start().starts_with('❯'))?;
        let label = line.trim_start().trim_start_matches('❯').trim_start();
        LABELS.iter().position(|name| label.starts_with(name))
    };
    let at = focused(tui).expect("认不出权限菜单的焦点");
    for _ in 0..at {
        tui.send("\x1b[A");
        pause(300);
    }
    assert_eq!(focused(tui), Some(0), "焦点应该落在 Always Ask 上");
    tui.send("\r");
    assert!(tui.wait("本会话切到 Always Ask", 15, |tui| {
        tui.text().contains("Permission mode: Always Ask")
            && tui.composer(spec).is_some_and(|read| read.empty)
    }));
    pause(800);
}
