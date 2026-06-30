use cc_reporter::{db_path, dispatch::dispatch, hook::HookEvent};
use cc_store::{ProviderKey, Store};
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
            cc_reporter::statusline::record(&store, &buf, now_ms());
        }
        // 无下游时这行就是状态栏；被包装脚本链下游时其 stdout 会被丢弃，仅写库生效。
        print!("{}", cc_reporter::statusline::minimal_line(&buf));
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

/// 从命令行解析 `--provider <name>` / `--provider=<name>`，缺省 claude。
fn parse_provider() -> ProviderKey {
    let args: Vec<String> = std::env::args().collect();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--provider" {
            if let Some(v) = it.next() {
                return ProviderKey::from_str(v);
            }
        } else if let Some(v) = a.strip_prefix("--provider=") {
            return ProviderKey::from_str(v);
        }
    }
    ProviderKey::Claude
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
