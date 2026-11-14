//! 真机走查：屏幕通道的「说明并拒绝」。让 CLI 做一件要批准的事，认出审批框，按对话页的做法
//! 选中「拒绝并说明」那一项、等屏幕进入等输入的状态、打一句说明、提交，然后断言：
//! 1. 被拒的命令没有执行（它本该写的标记文件不存在）；
//! 2. CLI 把说明交给了模型（说明里让模型只回一个算式的结果，屏幕上出现这个结果）；
//! 3. 回合结束，输入框回到空闲。
//!
//! 为什么每家都要真机走：这个流程是「选中该项、在终端里打字、再回车」，各家的键不同，按错可能
//! 变成放行（kimi 面板里数字键 1 就是 Approve once）。所以插件只在走通之后才给选项声明
//! `deny_with_feedback` 角色与 `FeedbackInput`。
//!
//! 按键不是探针自己定的：读前后端共用的 `app/src/test/fixtures/approval-frames.json` 里这家的
//! `denyFeedback`（`openInput` 进入输入状态的按键、`readyPattern` 屏幕上出现它才打字、
//! `submitInput` 提交键）。vitest 拿同一份文件里的真机屏幕过一遍生产的识别与卡片模型，断言算出来的
//! 三样就是它（`views/chat/approvalFrames.test.tsx`）。fixture 里还没有这家时（首次取证）用环境变量
//! `PROBE_FB_OPEN`、`PROBE_FB_READY`、`PROBE_FB_SUBMIT` 指定，支持 `\r` `\t` `\x1b` 转义。
//!
//! claude 平时走 broker 通道（hook 说了算）。这里不给它 broker 的环境变量，hook 直接放行给终端，
//! 走的就是屏幕兜底那条路（broker 过期、应用重启后的会话）。
//!
//! ```text
//! PROBE_AGENT=kimi PROBE_OUT=<目录> cargo test -p meowo-app --test probe_deny_feedback -- --ignored --nocapture
//! ```
//! 每家调一到两次模型（触发审批一次，收到说明后回一句）。

use meowo_agent::chat_ui::AttentionKind;
use std::path::Path;
use std::time::{Duration, Instant};

mod common;
#[path = "common/tui.rs"]
mod tui;
use tui::{pause, Tui};

const SHELL_PROMPT: &str =
    "Run this exact shell command and nothing else: echo meowo-probe > probe-out.txt";
/// 说明里带一个只有模型读到了才答得出的东西：1234×3 的结果。屏幕上别处不会有 3702。
const FEEDBACK: &str = "Do not run it. Reply with only the product of 1234 and 3.";
const FEEDBACK_ANSWER: &str = "3702";

fn unescape(raw: &str) -> String {
    raw.replace("\\r", "\r")
        .replace("\\t", "\t")
        .replace("\\x1b", "\x1b")
        .replace("\\e", "\x1b")
}

struct Flow {
    open: String,
    ready: regex::Regex,
    submit: String,
}

/// 「说明并拒绝」的三样：环境变量优先（首次取证），否则读共用 fixture。
fn flow(agent: &str) -> Flow {
    let from_env = |key: &str| std::env::var(key).ok().map(|raw| unescape(&raw));
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/test/fixtures/approval-frames.json");
    let fixture: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("读不到 approval-frames.json"))
            .unwrap();
    let declared = &fixture["frames"][agent]["denyFeedback"];
    let pick = |env: &str, key: &str| -> String {
        from_env(env)
            .or_else(|| declared[key].as_str().map(str::to_string))
            .unwrap_or_else(|| panic!("fixture 里没有 {agent} 的 denyFeedback.{key}；首次取证用 {env}"))
    };
    Flow {
        open: pick("PROBE_FB_OPEN", "openInput"),
        // 与前端同样的标志：逐行、不分大小写。
        ready: regex::Regex::new(&format!("(?im){}", pick("PROBE_FB_READY", "readyPattern")))
            .expect("readyPattern 编不过"),
        submit: pick("PROBE_FB_SUBMIT", "submitInput"),
    }
}

fn approval_on_screen(agent: &str, screen: &str) -> Option<&'static str> {
    meowo_agent::by_id(agent)
        .unwrap()
        .attention_patterns()
        .iter()
        .filter(|rule| rule.kind == AttentionKind::Approval)
        .find(|rule| {
            rule.patterns.iter().any(|pattern| {
                regex::Regex::new(&format!("(?im){pattern}"))
                    .unwrap()
                    .is_match(screen)
            })
        })
        .map(|rule| rule.id)
}

fn args(agent: &str) -> Vec<String> {
    let plugin = meowo_agent::by_id(agent).unwrap();
    let mut args: Vec<String> = match agent {
        // 模型用最便宜的；`--model` 只管本次启动，不改用户的默认模型。
        "claude" => ["--permission-mode", "default", "--model", "haiku"]
            .map(String::from)
            .to_vec(),
        "kimi" => Vec::new(),
        "gemini" => plugin
            .launch_options()
            .iter()
            .find(|o| o.id == "approval")
            .and_then(|o| o.choices.iter().find(|c| c.id == "default"))
            .expect("gemini 没有声明 approval/default")
            .args
            .iter()
            .map(|arg| arg.to_string())
            .collect(),
        other => panic!("没有 {other} 的走查方案"),
    };
    if let Ok(extra) = std::env::var("PROBE_ARGS") {
        args.extend(extra.split_whitespace().map(String::from));
    }
    args
}

#[test]
#[ignore = "启动真实 CLI 并调用模型；真机走查用"]
fn deny_with_feedback_rejects_and_delivers_the_reason() {
    tui::use_bundled_conpty();
    let agent = std::env::var("PROBE_AGENT").expect("PROBE_AGENT");
    let plugin = meowo_agent::by_id(&agent).unwrap();
    let version = tui::cli_version(&agent);
    let spec = plugin
        .composer(Some(&version))
        .unwrap_or_else(|| panic!("{agent} {version} 没有输入框规格"));
    let flow = flow(&agent);
    let args = args(&agent);
    let cwd = tui::temp_cwd("deny-feedback-probe");
    let mut argv = plugin.launch_argv();
    argv.extend(args.iter().cloned());
    println!("===== {agent} {version} =====\n  启动参数：{args:?}");
    let mut tui = Tui::spawn(&argv, &cwd, &[]);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        walk(&agent, &version, &args, &flow, &spec, &tui, &cwd)
    }));
    // 成败都收掉我们拉起的 CLI，别留进程。
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

fn walk(
    agent: &str,
    version: &str,
    args: &[String],
    flow: &Flow,
    spec: &meowo_agent::chat_ui::ComposerSpec,
    tui: &Tui,
    cwd: &Path,
) {
    tui::wait_ready(tui, spec);
    if agent == "kimi" {
        tui::kimi_always_ask(tui, spec);
    }
    let marker = cwd.join("probe-out.txt");
    tui.submit(SHELL_PROMPT);

    // 等审批框：规则命中且连续两次读屏都命中（画面稳住）。
    let started = Instant::now();
    let mut seen: Option<&'static str> = None;
    let mut rule: Option<&'static str> = None;
    while started.elapsed() < Duration::from_secs(150) {
        assert!(!marker.exists(), "{agent}：没有弹审批就直接执行了");
        let hit = approval_on_screen(agent, &tui.text());
        if hit.is_some() && hit == seen {
            rule = hit;
            break;
        }
        seen = hit;
        pause(500);
    }
    if rule.is_none() {
        tui.dump();
    }
    let rule = rule.unwrap_or_else(|| panic!("{agent}：150s 内屏幕上没认出审批框"));
    println!("  ✓ 认出审批（{:.1}s）：{rule}", started.elapsed().as_secs_f32());
    pause(800);
    tui.dump();
    let approval_screen = tui.trimmed();

    // 第一步：进入等输入的状态。认出来才往下打字，认不出就停（生产同样如此）。
    tui.send(&flow.open);
    let ready = tui.wait("进入等输入说明的状态", 8, |tui| flow.ready.is_match(&tui.text()));
    tui.dump();
    assert!(ready, "{agent}：按了 {:?} 之后屏幕上没出现等输入的标志，不打字", flow.open);
    assert!(!marker.exists(), "{agent}：进入输入状态这一步把命令放行了");
    let ready_screen = tui.trimmed();

    // 第二步：打说明。正文与提交键分两次写。
    pause(400);
    tui.send(FEEDBACK);
    pause(800);
    tui.dump();
    assert!(
        tui.text().contains("product of 1234"),
        "{agent}：打进去的说明没有出现在屏幕上"
    );
    assert!(!marker.exists(), "{agent}：打字这一步把命令放行了");
    let typed_screen = tui.trimmed();
    tui.send(&flow.submit);

    // 第三步：审批框消失，模型按说明作答，回合结束。
    assert!(tui.wait("审批框消失", 30, |tui| approval_on_screen(agent, &tui.text()).is_none()));
    assert!(tui.wait("模型按说明作答（屏幕上出现 3702）", 120, |tui| {
        tui.text().contains(FEEDBACK_ANSWER)
    }));
    assert!(tui.wait("回合结束，输入框回到空闲", 120, |tui| {
        approval_on_screen(agent, &tui.text()).is_none()
            && tui.composer(spec).is_some_and(|read| read.empty)
    }));
    pause(1500);
    tui.dump();
    assert!(
        !marker.exists(),
        "{agent}：说明并拒绝之后，被拒的命令还是执行了（标记文件出现）"
    );
    println!("  ✓ 标记文件不存在：被拒的命令没有执行");

    let sample = serde_json::json!({
        "agent": agent,
        "version": version,
        "args": args,
        "prompt": SHELL_PROMPT,
        "rule": rule,
        "feedback": FEEDBACK,
        "openInput": flow.open,
        "readyPattern": flow.ready.as_str().trim_start_matches("(?im)"),
        "submitInput": flow.submit,
        "screen": approval_screen,
        "readyScreen": ready_screen,
        "typedScreen": typed_screen,
        "finalScreen": tui.trimmed(),
    });
    let out = std::env::var("PROBE_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    std::fs::create_dir_all(&out).unwrap();
    let file = out.join(format!("deny-feedback-{agent}.json"));
    std::fs::write(&file, serde_json::to_string_pretty(&sample).unwrap()).unwrap();
    println!("  样本写到 {}", file.display());
    println!("===== {agent}：通过 =====");
}
