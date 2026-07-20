use meowo_reporter::{db_path, dispatch::dispatch, hook::HookEvent};
use meowo_store::Store;
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

mod attach;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "attach") {
        if let Err(error) = attach::run(&args) {
            eprintln!("Meowo attach failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    // 任何错误都吞掉并以 0 退出——绝不阻塞 Agent。显式诊断开关只写 stderr，便于定位
    // hook 已执行但没有入库/没有进入 GUI broker 的问题，默认行为仍完全静默。
    if let Err(error) = run() {
        if std::env::var_os("MEOWO_REPORTER_DEBUG").is_some() {
            eprintln!("Meowo reporter diagnostic: {error}");
        }
    }
    std::process::exit(0);
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;

    // statusline 子命令：解析 CC statusline JSON 写入上下文用量，再把 stdin 原样透传到 stdout，
    // 供管道下游（claude-hud）照常渲染。解析/写库失败都不影响透传。
    if std::env::args().nth(1).as_deref() == Some("statusline") {
        if let Ok(store) = Store::open(db_path()) {
            meowo_reporter::statusline::record(&store, &buf, now_ms());
        }
        // 无下游时这行就是状态栏；被包装脚本链下游时其 stdout 会被丢弃，仅写库生效。
        print!("{}", meowo_reporter::statusline::minimal_line(&buf));
        return Ok(());
    }

    let ev = HookEvent::parse(&buf)?;
    let store = Store::open(db_path())?;
    let now = now_ms();
    // agent 提供方：kimi 的 hook 命令带 `--provider kimi`；Claude 不带 → 默认 claude。
    let provider = parse_provider();
    let canonical_event = meowo_agent::by_id(&provider).map_or(ev.hook_event_name.as_str(), |p| {
        p.canonical_event(&ev.hook_event_name)
    });
    // Codex 的 hook 可能继承 workspace 沙箱：能读 ~/.meowo/board.db，却不能写。审批桥不能被
    // 这次遥测写入失败短路，否则 PermissionRequest 永远只会落回终端。先记下错误，完成所有
    // 不依赖写库的 broker 工作后再返回；main 最终仍按契约吞错并以 0 退出。
    let dispatch_error = dispatch(&store, &ev, now, &provider).err();
    if canonical_event == "SessionStart" {
        if let Some(session_id) = store.find_session_id_pub(&ev.session_id)? {
            attach::notify_claim(session_id);
        }
    }
    if canonical_event == "PermissionRequest" {
        if let Some(session_id) = store.find_session_id_pub(&ev.session_id)? {
            if let Some(decision) = attach::request_approval(
                session_id,
                &provider,
                ev.tool_name.as_deref().unwrap_or("Tool"),
                ev.tool_input.as_ref(),
                &ev.permission_suggestions,
            ) {
                let output = match decision {
                    meowo_protocol::broker::ApprovalDecision::Allow => Some(serde_json::json!({
                        "hookSpecificOutput": {
                            "hookEventName": "PermissionRequest",
                            "decision": { "behavior": "allow" }
                        }
                    })),
                    meowo_protocol::broker::ApprovalDecision::AllowWithPermissions(updated) => Some(serde_json::json!({
                        "hookSpecificOutput": {
                            "hookEventName": "PermissionRequest",
                            "decision": {
                                "behavior": "allow",
                                "updatedPermissions": updated,
                            }
                        }
                    })),
                    meowo_protocol::broker::ApprovalDecision::Deny => Some(serde_json::json!({
                        "hookSpecificOutput": {
                            "hookEventName": "PermissionRequest",
                            "decision": {
                                "behavior": "deny",
                                "message": "Denied in Meowo."
                            }
                        }
                    })),
                    // GUI 消费者消失时交还 Agent 原终端；不输出 hook 决策即可恢复原生提示。
                    meowo_protocol::broker::ApprovalDecision::Pass => None,
                };
                if let Some(output) = output {
                    println!("{output}");
                }
            }
        }
    }
    if let Some(error) = dispatch_error {
        return Err(Box::new(error));
    }
    Ok(())
}

/// 从命令行解析 `--provider <name>` / `--provider=<name>` 的**原始字符串**，缺省默认 agent。
///
/// 刻意**不**归一到已注册的 `AgentId`：该参数由我们自己写进各 agent 的 hooks 命令行，但**跨版本**
/// 时更新版 meowo 可能写入本版本尚不认识的 id（如 `gemini`）。若在此回退成 `DEFAULT_ID`，dispatch
/// 就会把这个未知会话落库成默认 agent（甚至因默认不写库而落成 NULL）——等于把未知 provider 冒名成
/// claude，正是 `meowo_agent::resolve` 契约要杜绝的。
///
/// 故原样返回：dispatch 把它原样写进 `sessions.provider`（未知值也保留），仅在需要能力时才对
/// **已注册**插件做 `by_id` 查询，查不到就整段降级。缺省（claude 不带 `--provider`）返回默认 id。
fn parse_provider() -> String {
    let args: Vec<String> = std::env::args().collect();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--provider" {
            if let Some(v) = it.next() {
                return v.clone();
            }
        } else if let Some(v) = a.strip_prefix("--provider=") {
            return v.to_string();
        }
    }
    meowo_agent::DEFAULT_ID.as_str().to_string()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
