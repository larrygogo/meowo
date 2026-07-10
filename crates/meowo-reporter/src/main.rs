use meowo_agent::AgentId;
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
    dispatch(&store, &ev, now, provider)?;
    Ok(())
}

/// 从命令行解析 `--provider <name>` / `--provider=<name>`，缺省默认 agent。
///
/// 未知 id 也回退默认并告警：该参数由我们自己写进各 agent 的 hooks 命令行，出现未知值只可能是
/// 配置被更新版 meowo 写过、又被旧版读到。此处**必须**产出一个可用的 agent——reporter 是 hook
/// 子进程，中断会阻塞 agent 本体。落库的 provider 列由 dispatch 按此 id 写，仍是已注册的那批。
fn parse_provider() -> AgentId {
    let args: Vec<String> = std::env::args().collect();
    let mut it = args.iter();
    let mut raw: Option<&str> = None;
    while let Some(a) = it.next() {
        if a == "--provider" {
            raw = it.next().map(String::as_str);
            break;
        } else if let Some(v) = a.strip_prefix("--provider=") {
            raw = Some(v);
            break;
        }
    }
    match meowo_agent::resolve(raw) {
        Some(p) => p.id(),
        None => {
            eprintln!(
                "meowo-reporter: 未知 --provider {:?}，回退默认 agent {}",
                raw.unwrap_or(""),
                meowo_agent::DEFAULT_ID
            );
            meowo_agent::DEFAULT_ID
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
