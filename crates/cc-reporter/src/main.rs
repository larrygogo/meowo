use cc_reporter::{db_path, dispatch::dispatch, hook::HookEvent};
use cc_store::Store;
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
    let ev = HookEvent::parse(&buf)?;
    let store = Store::open(db_path())?;
    let now = now_ms();
    dispatch(&store, &ev, now)?;
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
