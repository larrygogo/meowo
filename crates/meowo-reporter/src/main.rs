use meowo_reporter::{db_path, dispatch::dispatch, hook::HookEvent};
use meowo_store::Store;
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    // 任何错误都吞掉并以 0 退出——绝不阻塞 Claude Code。
    let _ = run();
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
    dispatch(&store, &ev, now, &provider)?;
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
