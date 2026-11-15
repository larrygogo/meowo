//! 真机走查：让 CLI 做一件要批准的事，认出这次请求，按对话页「允许一次」的做法作答，确认 CLI 接着做完。
//!
//! 两条通道都走生产的东西，不起完整应用：
//! - broker 通道（插件声明 `permission_hook_decides` 的家：claude、codex）：用户机器上装好的 hook 照常
//!   调 `meowo-reporter`，探针在本机起一个端口当 broker，用 `meowo_protocol::broker` 的生产编解码收
//!   `ApprovalRequest`、回 `ApprovalDecision::Allow`（对话页「允许一次」回的就是它）。应用里那一层
//!   （入表、弹卡、窗口）不在这里，由 vitest 用这次抓到的请求回放。
//! - 屏幕通道（kimi、gemini、opencode）：用插件声明的 `attention_patterns` 在整屏上认出审批框，
//!   按键取自 `app/src/test/fixtures/approval-frames.json` 里这家的 `allowOnceInput`。那个值不是手填了
//!   就算数：vitest 拿同一份文件里的真机屏幕过一遍生产的识别与卡片模型，断言「允许一次」算出来的
//!   按键就是它（`views/chat/approvalFrames.test.ts`）。
//!
//! 各家怎么才会弹审批（2026-11-14 本机取证）：
//! - claude：`--permission-mode default` 下带重定向的 shell 命令；
//! - codex：插件声明的「只读」档（`--sandbox read-only`）下写文件；
//! - kimi：用户配置的默认档是 Ask When Needed（删文件都直接执行），启动参数里没有更严的档。
//!   会话里 `/permission` 选 Always Ask：只对本会话生效，不写 config.toml（隔离目录里验证过）；
//! - gemini：`--approval-mode default` 下带重定向的 shell 命令；
//! - opencode：读工作区外的文件（默认 agent 对工作区内的命令不问）。
//!
//! 每家调一次模型，提示词一句话。抓到的屏幕与请求写到 `PROBE_OUT`（缺省系统临时目录）下的
//! `approval-<家>.json`，核对后并进上面那份 fixture。
//!
//! ```text
//! PROBE_AGENT=kimi PROBE_OUT=<目录> cargo test -p meowo-app --test probe_approval -- --ignored --nocapture
//! ```
//! `PROBE_ARGS` 追加启动参数（opencode 的 `--model`）；`PROBE_ALLOW_INPUT` 临时指定屏幕通道的按键
//! （fixture 里还没有这家时用，支持 `\r` `\x1b` 转义）。

use meowo_agent::chat_ui::AttentionKind;
use meowo_protocol::broker::{read_handshake, ApprovalDecision, ApprovalRequest, BrokerRequest};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

mod common;
#[path = "common/tui.rs"]
mod tui;
use tui::{pause, Tui};

const SHELL_PROMPT: &str =
    "Run this exact shell command and nothing else: echo meowo-probe > probe-out.txt";
const READ_PROMPT: &str = "Read the file C:/Windows/win.ini and reply with its first line only.";
const TOKEN: &str = "meowo-approval-probe-token";

/// 这一家怎么启动、发什么、做完的证据是什么。
struct Plan {
    args: Vec<String>,
    prompt: &'static str,
    /// 做完的证据：true = 工作目录里出现了 probe-out.txt；false = 屏幕上出现 win.ini 的第一行。
    file_evidence: bool,
}

fn plan(agent: &str) -> Plan {
    let plugin = meowo_agent::by_id(agent).unwrap();
    // 权限档用插件声明的启动参数（新建页选这一档时生产拼的就是它）。
    let choice = |option: &str, choice: &str| -> Vec<String> {
        plugin
            .launch_options()
            .iter()
            .find(|o| o.id == option)
            .and_then(|o| o.choices.iter().find(|c| c.id == choice))
            .unwrap_or_else(|| panic!("{agent} 没有声明 {option}/{choice}"))
            .args
            .iter()
            .map(|arg| arg.to_string())
            .collect()
    };
    let (mut args, prompt, file_evidence) = match agent {
        // claude 的默认档不带参数（跟用户配置走），这里显式指定；模型用最便宜的，不改用户默认值
        // （`--model` 只管本次启动）。
        "claude" => (
            ["--permission-mode", "default", "--model", "haiku"]
                .map(String::from)
                .to_vec(),
            SHELL_PROMPT,
            true,
        ),
        "codex" => (choice("approval", "readOnly"), SHELL_PROMPT, true),
        "kimi" => (Vec::new(), SHELL_PROMPT, true),
        "gemini" => (choice("approval", "default"), SHELL_PROMPT, true),
        "opencode" => (Vec::new(), READ_PROMPT, false),
        other => panic!("没有 {other} 的走查方案"),
    };
    if let Ok(extra) = std::env::var("PROBE_ARGS") {
        args.extend(extra.split_whitespace().map(String::from));
    }
    Plan {
        args,
        prompt,
        file_evidence,
    }
}

/// 探针充当 broker：只收审批请求，收到的连同连接一起交给主线程，由它决定何时回、回什么。
fn start_broker() -> (String, mpsc::Receiver<(ApprovalRequest, TcpStream)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            match read_handshake(&mut stream) {
                Ok(BrokerRequest::Approval { token, request }) if token == TOKEN => {
                    if tx.send((request, stream)).is_err() {
                        return;
                    }
                }
                // 认领、attach 等别的请求：探针不是完整的 broker，断开即可（reporter 当成没受理）。
                _ => {}
            }
        }
    });
    (endpoint, rx)
}

/// 插件声明的审批框规则在这一屏上命中没有。规则是给前端用的 JS 正则（`im` 标志），这里按同样的
/// 标志用 Rust 的 regex 编译；用到的语法两边通用。
fn approval_on_screen(agent: &str, screen: &str) -> Option<&'static str> {
    meowo_agent::by_id(agent)
        .unwrap()
        .attention_patterns()
        .iter()
        .filter(|rule| rule.kind == AttentionKind::Approval)
        .find(|rule| {
            rule.patterns.iter().any(|pattern| {
                regex::Regex::new(&format!("(?im){pattern}"))
                    .unwrap_or_else(|error| panic!("{} 的规则编不过：{error}", rule.id))
                    .is_match(screen)
            })
        })
        .map(|rule| rule.id)
}

fn unescape(raw: &str) -> String {
    raw.replace("\\r", "\r")
        .replace("\\x1b", "\x1b")
        .replace("\\e", "\x1b")
}

/// 屏幕通道「允许一次」的按键：取自前后端共用的 fixture（vitest 断言它等于生产卡片模型算出来的）。
fn allow_once_input(agent: &str) -> String {
    if let Ok(input) = std::env::var("PROBE_ALLOW_INPUT") {
        return unescape(&input);
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/test/fixtures/approval-frames.json");
    let fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("读不到 {}", path.display())),
    )
    .unwrap();
    fixture["frames"][agent]["allowOnceInput"]
        .as_str()
        .unwrap_or_else(|| panic!("fixture 里没有 {agent} 的 allowOnceInput；首次抓样本用 PROBE_ALLOW_INPUT"))
        .to_string()
}

#[test]
#[ignore = "启动真实 CLI 并调用模型；真机走查用"]
fn approval_is_recognized_allowed_once_and_the_cli_carries_on() {
    tui::use_bundled_conpty();
    let agent = std::env::var("PROBE_AGENT").expect("PROBE_AGENT");
    let plugin = meowo_agent::by_id(&agent).unwrap();
    let version = tui::cli_version(&agent);
    let spec = plugin
        .composer(Some(&version))
        .unwrap_or_else(|| panic!("{agent} {version} 没有输入框规格"));
    let plan = plan(&agent);
    let cwd = tui::temp_cwd("approval-probe");
    let (endpoint, requests) = start_broker();
    let mut argv = plugin.launch_argv();
    argv.extend(plan.args.iter().cloned());
    println!("===== {agent} {version} =====\n  启动参数：{:?}", plan.args);
    let mut tui = Tui::spawn(
        &argv,
        &cwd,
        &[
            ("MEOWO_PTY_ENDPOINT", endpoint),
            ("MEOWO_PTY_TOKEN", TOKEN.to_string()),
            ("MEOWO_PTY_PROTOCOL", "2".to_string()),
        ],
    );
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        walk(&agent, &version, &plan, &spec, &tui, &cwd, &requests)
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
    plan: &Plan,
    spec: &meowo_agent::chat_ui::ComposerSpec,
    tui: &Tui,
    cwd: &Path,
    requests: &mpsc::Receiver<(ApprovalRequest, TcpStream)>,
) {
    let plugin = meowo_agent::by_id(agent).unwrap();
    tui::wait_ready(tui, spec);
    if agent == "kimi" {
        tui::kimi_always_ask(tui, spec);
    }
    let evidence = cwd.join("probe-out.txt");
    let done = |tui: &Tui| {
        if plan.file_evidence {
            evidence.exists()
        } else {
            tui.text().contains("16-bit app support")
        }
    };

    tui.submit(plan.prompt);

    // 等审批出现。hook 说了算的家以 broker 收到请求为准；其余以屏幕规则命中、且画面稳住为准。
    let hook_decides = plugin.permission_hook_decides();
    let started = Instant::now();
    let mut pending: Option<(ApprovalRequest, TcpStream)> = None;
    let mut rule: Option<&'static str> = None;
    // 「稳住」= 连续两次读屏都命中同一条规则。不比整屏：有的家审批框旁边一直有动画在转。
    let mut seen: Option<&'static str> = None;
    while started.elapsed() < Duration::from_secs(150) {
        assert!(!done(tui), "{agent}：没有弹审批就直接做完了，这个权限档下这件事不需要批准");
        if let Ok(request) = requests.try_recv() {
            pending = Some(request);
            // 给 TUI 一点时间把它自己的权限框也画出来（claude 两边并行），好一起存样本。
            pause(1500);
            rule = approval_on_screen(agent, &tui.text());
            break;
        }
        if !hook_decides {
            let hit = approval_on_screen(agent, &tui.text());
            if hit.is_some() && hit == seen {
                rule = hit;
                break;
            }
            seen = hit;
        }
        pause(500);
    }
    if (hook_decides && pending.is_none()) || (!hook_decides && rule.is_none()) {
        tui.dump();
    }
    let channel = if pending.is_some() { "broker" } else { "terminal" };
    if hook_decides {
        assert!(pending.is_some(), "{agent}：150s 内 broker 没收到审批请求");
    } else {
        assert!(rule.is_some(), "{agent}：150s 内屏幕上没认出审批框");
    }
    println!(
        "  ✓ 认出审批（{:.1}s）：通道 {channel}，屏幕规则 {}",
        started.elapsed().as_secs_f32(),
        rule.unwrap_or("未命中（终端不画权限框）")
    );
    tui.dump();
    let screen = tui.trimmed();
    let request_json = pending.as_ref().map(|(request, _)| {
        println!(
            "  请求：tool={} input={} suggestions={}",
            request.tool_name,
            request.input.replace('\n', " "),
            request.permission_suggestions.len()
        );
        serde_json::to_value(request).unwrap()
    });

    // 作答「允许一次」。
    let allow_input = match pending.take() {
        Some((_, mut stream)) => {
            stream
                .write_all(format!("{}\n", ApprovalDecision::Allow.as_wire()).as_bytes())
                .unwrap();
            None
        }
        None => {
            let input = allow_once_input(agent);
            tui.send(&input);
            Some(input)
        }
    };

    // CLI 接着做完。期间同一件事再来请求（有的家会分两步问）照样允许，并记下来。
    let mut extra = 0;
    assert!(tui.wait("允许之后 CLI 接着做完", 150, |tui| {
        if let Ok((request, mut stream)) = requests.try_recv() {
            extra += 1;
            println!("  又一次请求：tool={}", request.tool_name);
            let _ = stream.write_all(format!("{}\n", ApprovalDecision::Allow.as_wire()).as_bytes());
        }
        done(tui)
    }));
    assert!(tui.wait("回合结束，输入框回到空闲", 150, |tui| {
        if let Ok((request, mut stream)) = requests.try_recv() {
            extra += 1;
            println!("  又一次请求：tool={}", request.tool_name);
            let _ = stream.write_all(format!("{}\n", ApprovalDecision::Allow.as_wire()).as_bytes());
        }
        approval_on_screen(agent, &tui.text()).is_none()
            && tui.composer(spec).is_some_and(|read| read.empty)
    }));

    let sample = serde_json::json!({
        "agent": agent,
        "version": version,
        "args": plan.args,
        "prompt": plan.prompt,
        "channel": channel,
        "rule": rule,
        "allowOnceInput": allow_input,
        "extraRequests": extra,
        "screen": screen,
        "brokerRequest": request_json,
    });
    let out = std::env::var("PROBE_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    std::fs::create_dir_all(&out).unwrap();
    let file = out.join(format!("approval-{agent}.json"));
    std::fs::write(&file, serde_json::to_string_pretty(&sample).unwrap()).unwrap();
    println!("  样本写到 {}", file.display());
    println!("===== {agent}：通过 =====");
}
