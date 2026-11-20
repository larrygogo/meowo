//! 真机走查（Gemini）：发消息并得到回答、运行中再发一条。审批那一步走 `probe_approval.rs`。
//!
//! 只在屏幕认得出时才动手：启动后只等输入框（不处理任何启动询问，见到别的画面就超时退出、
//! 打出整屏）；之后写进终端的只有两句正文和各自的回车，都在输入框认得出或回合确认在跑时才写。
//!
//! 运行中那一句的写法与对话页一致：正文与回车分两次写（`Tui::submit`），回车是插件声明的
//! `queue_input`。用户设置里开着 `experimental.modelSteering` 时它是「插话」（插件 `queue_delivery`
//! 报 NextStep），否则是排队到回合结束。
//!
//! 每个时刻的屏幕、以及 Gemini 自己的会话记录（`~/.gemini/tmp/<cwd 名>/chats/*.jsonl`）在第二句送进去
//! 的那一刻和回合结束后各拷一份，写到 `PROBE_OUT`（缺省系统临时目录）下的 `gemini-walk/`。
//! 会话记录是回放测试的原料：回合中途那一刻的记录过生产解析与时间线，在跑的调用不该是「已中断」。
//!
//! ```text
//! PROBE_OUT=<目录> cargo test -p meowo-app --test probe_gemini_walk -- --ignored --nocapture
//! ```
//! 跑之前清掉 `MEOWO_PROFILE`、`CLAUDE_CONFIG_DIR`（外层会话的账号变量会被拉起的 CLI 继承）。
//! 跑完探针删掉自己的工作目录；`~/.gemini/tmp/<cwd 名>` 留给人核对后再删（路径打在输出里）。

use std::path::{Path, PathBuf};
use std::time::Instant;

mod common;
#[path = "common/tui.rs"]
mod tui;
use tui::{pause, Tui};

/// 两句正文都不带半角问号（0.68.0 会吞，对话页另走粘贴）；答案是算出来的，不会和回显混淆。
const FIRST: &str = "Reply with only the product of 1234 and 3, as digits.";
const FIRST_ANSWER: &str = "3702";
/// 让回合里有一个要跑四十几秒的调用，第二句在它跑的时候送进去。yolo 档不弹审批。
/// 要跑得比对话页的回显确认时限（ECHO_CONFIRM_MS，15 秒）久：插话要等这一步跑完才写进会话记录，
/// 第一次走查用 20 秒的 ping，从送进去到落盘约 12 秒，没超过时限，看不出对话页会不会误报。
const LONG: &str = "Run this exact shell command and nothing else: ping -n 45 127.0.0.1";
const SECOND: &str = "Also, after the command finishes, reply with the product of 2222 and 3, as digits.";
const SECOND_ANSWER: &str = "6666";

/// 屏幕上出现了答案。答案是算出来的、不在任何一句正文里，所以不会是回显。不要求行首是 `✦`：
/// 插话之后的回答先写一句确认（`✦ Acknowledged steering (ADD_TASK): …`），答案在下一行
/// （2026-11-20 第一次跑就是这样，按 `✦` 行首找没认出来）。
fn answered(tui: &Tui, answer: &str) -> bool {
    tui.text().contains(answer)
}

/// 回合在跑：spinner 行带 `esc to cancel`（与插件屏幕规则 spinner_line_working / esc_cancel_working 同口径）。
fn working(tui: &Tui) -> bool {
    tui.text().to_lowercase().contains("esc to cancel")
}

fn idle(tui: &Tui, spec: &meowo_agent::chat_ui::ComposerSpec) -> bool {
    !working(tui) && tui.composer(spec).is_some_and(|read| read.empty)
}

fn chats_dir(cwd: &Path) -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap();
    home.join(".gemini/tmp")
        .join(cwd.file_name().unwrap())
        .join("chats")
}

fn copy_chats(cwd: &Path, out: &Path, tag: &str) -> Vec<String> {
    let mut copied = Vec::new();
    let Ok(entries) = std::fs::read_dir(chats_dir(cwd)) else {
        return copied;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let target = out.join(format!("{tag}.{name}"));
        if std::fs::copy(entry.path(), &target).is_ok() {
            copied.push(target.display().to_string());
        }
    }
    copied
}

fn save_screen(tui: &Tui, out: &Path, tag: &str) {
    std::fs::write(
        out.join(format!("{tag}.screen.json")),
        serde_json::to_string_pretty(&tui.trimmed()).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "启动真实 Gemini CLI 并调用模型；真机走查用"]
fn gemini_answers_and_takes_a_message_mid_turn() {
    tui::use_bundled_conpty();
    let plugin = meowo_agent::by_id("gemini").unwrap();
    let version = tui::cli_version("gemini");
    let spec = plugin
        .composer(Some(&version))
        .unwrap_or_else(|| panic!("gemini {version} 没有输入框规格"));
    let out = std::env::var("PROBE_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("gemini-walk");
    std::fs::create_dir_all(&out).unwrap();
    let cwd = tui::temp_cwd("gemini-walk");
    let mut argv = plugin.launch_argv();
    argv.extend(["--approval-mode", "yolo"].map(String::from));
    println!(
        "===== gemini {version} =====\n  启动参数：{:?}\n  工作目录：{}\n  会话记录：{}\n  steering：{:?}",
        &argv[1..],
        cwd.display(),
        chats_dir(&cwd).display(),
        plugin.queue_delivery()
    );
    let mut tui = Tui::spawn(&argv, &cwd, &[]);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        walk(&tui, &spec, &cwd, &out)
    }));
    tui.kill();
    for _ in 0..20 {
        if std::fs::remove_dir_all(&cwd).is_ok() || !cwd.exists() {
            break;
        }
        pause(250);
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

fn walk(tui: &Tui, spec: &meowo_agent::chat_ui::ComposerSpec, cwd: &Path, out: &Path) {
    // 只等，不按任何键：见到不认识的启动画面就在这里超时，打出整屏。
    let ready = tui.wait("输入框认得出，是空的", 90, |tui| {
        tui.composer(spec).is_some_and(|read| read.empty)
    });
    save_screen(tui, out, "0-ready");
    assert!(ready, "启动后没等到空输入框（画面见上）");
    pause(1500);

    // 第一步：发消息并得到回答。
    let started = Instant::now();
    tui.submit(FIRST);
    let ok = tui.wait("第一句得到回答", 240, |tui| answered(tui, FIRST_ANSWER));
    save_screen(tui, out, "1-answer");
    assert!(ok, "第一句 240s 内没有回答");
    assert!(tui.wait("回合结束，输入框回到空闲", 60, |tui| idle(tui, spec)));
    println!("  第一步：{:.1}s 内得到回答", started.elapsed().as_secs_f32());
    pause(1500);

    // 第二步：起一个要跑一会儿的调用，在它跑的时候再发一句。
    tui.submit(LONG);
    // 认得出「调用在跑」：屏幕上回合在跑，且 ping 的输出已经开始出现（调用已经发出、没结束）。
    let running = tui.wait("ping 在跑", 240, |tui| {
        let text = tui.text();
        working(tui) && text.contains("Reply from 127.0.0.1") && !text.contains("Packets: Sent")
    });
    save_screen(tui, out, "2-running");
    assert!(running, "240s 内没看到 ping 在跑");
    let mid = copy_chats(cwd, out, "2-running");
    println!("  回合中途（第二句之前）的会话记录：{mid:?}");
    tui.submit(SECOND);
    // 第二句的回车写进终端的时刻（宿主机时钟，Unix 毫秒）：与会话记录里那条插话的 timestamp 相减，
    // 就是对话页从发出到在记录里见到它要等多久。
    let sent_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    println!("  第二句回车写进终端：unix_ms={sent_at}");
    std::fs::write(out.join("second-sent-at.txt"), sent_at.to_string()).unwrap();
    pause(1200);
    save_screen(tui, out, "3-after-second");
    tui.dump();
    let mid = copy_chats(cwd, out, "3-after-second");
    println!("  第二句送进去之后的会话记录：{mid:?}");
    assert!(working(tui), "第二句送进去之后回合不在跑了（被打断）");
    assert!(
        !tui.text().contains("Request cancelled"),
        "第二句把在跑的回合取消了"
    );

    let ok = tui.wait("第二句得到回答", 300, |tui| answered(tui, SECOND_ANSWER));
    save_screen(tui, out, "4-second-answer");
    assert!(ok, "第二句 300s 内没有回答");
    assert!(tui.wait("回合结束，输入框回到空闲", 120, |tui| idle(tui, spec)));
    pause(2000);
    save_screen(tui, out, "5-final");
    let done = copy_chats(cwd, out, "5-final");
    println!("  回合结束后的会话记录：{done:?}");
    let ping_done = tui.text().contains("Packets: Sent") || tui.text().contains("Average");
    println!("  ping 跑完的字样在屏幕上：{ping_done}");
    println!("===== gemini 走查：通过 =====");
}
