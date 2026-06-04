/// 抓取「拥有本进程的 Claude Code 会话」PID：取本进程父进程；
/// 若父进程名是已知 shell 包装，则再往上找一层（CC 直接 spawn hook 时父即 claude.exe）。
pub fn owner_pid() -> Option<u32> {
    use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::new()),
    );
    let cur = sysinfo::get_current_pid().ok()?;
    let parent = sys.process(cur)?.parent()?;
    let is_shell = |pid: Pid| -> bool {
        sys.process(pid)
            .map(|p| {
                let n = p.name().to_string_lossy().to_ascii_lowercase();
                ["cmd.exe", "powershell.exe", "pwsh.exe", "bash.exe", "sh.exe", "zsh.exe", "conhost.exe"]
                    .iter()
                    .any(|s| n == *s)
            })
            .unwrap_or(false)
    };
    let target = if is_shell(parent) {
        sys.process(parent).and_then(|p| p.parent()).unwrap_or(parent)
    } else {
        parent
    };
    Some(target.as_u32())
}
