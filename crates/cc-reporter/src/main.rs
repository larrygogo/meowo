use cc_reporter::{db_path, dispatch::dispatch, hook::HookEvent};
use cc_store::Store;
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    // 任何错误都吞掉并以 0 退出——绝不阻塞 Claude Code。
    let _ = run();
    std::process::exit(0);
}

/// 调试埋点：每次被调用都追加一行到 ~/.cc-kanban/hooks-trace.log（排查 codex/kimi 是否真的调起 hook）。
fn trace(line: &str) {
    use std::io::Write;
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        let p = std::path::Path::new(&home).join(".cc-kanban").join("hooks-trace.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;

    let argv: Vec<String> = std::env::args().skip(1).collect();
    trace(&format!(
        "[{}] argv={:?} stdin_len={} head={:?}",
        now_ms(),
        argv,
        buf.len(),
        buf.chars().take(160).collect::<String>()
    ));

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

    let ev = match HookEvent::parse(&buf) {
        Ok(e) => e,
        Err(e) => {
            trace(&format!("[{}] PARSE-FAIL: {e}", now_ms()));
            return Err(e.into());
        }
    };
    let store = Store::open(db_path())?;
    let now = now_ms();
    // agent 提供方：kimi 的 hook 命令带 `--provider kimi`；Claude 不带 → 默认 claude。
    let provider = parse_provider();
    trace(&format!(
        "[{now}] parsed event={} session={} provider={provider} cwd={:?}",
        ev.hook_event_name, ev.session_id, ev.cwd
    ));
    dispatch(&store, &ev, now, &provider)?;
    Ok(())
}

/// 从命令行解析 `--provider <name>` / `--provider=<name>`，缺省 "claude"。
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
    "claude".to_string()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
