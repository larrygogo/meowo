/// 抓取「拥有本进程的 Claude Code 会话」PID。
///
/// hook 由 claude.exe 触发，但 Windows 上 reporter 的直接父进程往往是一闪而过的
/// 包装进程（cmd / conhost / 某 launcher），hook 跑完就退出——它的 PID 不稳定。
/// 所以**向上遍历进程树，返回第一个名字含 "claude" 的祖先**（= 会话本体 claude.exe，
/// 终端关掉才会退出）。找不到则返回 None（宁可不记 PID，也不记错的）。
pub fn owner_pid() -> Option<u32> {
    use sysinfo::{ProcessRefreshKind, RefreshKind, System};
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::new()),
    );
    let mut cur = sysinfo::get_current_pid().ok()?;
    // 最多向上走 16 层，避免异常环导致死循环。
    for _ in 0..16 {
        let parent = sys.process(cur)?.parent()?;
        let name = sys
            .process(parent)
            .map(|p| p.name().to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if name.contains("claude") {
            return Some(parent.as_u32());
        }
        cur = parent;
    }
    None
}
