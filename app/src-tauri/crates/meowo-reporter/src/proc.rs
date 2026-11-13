/// 抓取「拥有本次 hook 的 agent 会话」PID。
///
/// hook 由 agent 触发，但 reporter 的直接父进程往往是一闪而过的包装进程
/// （Windows: cmd/conhost/某 launcher；Unix: 可能经 sh -c）。hook 跑完就退出——它的 PID 不稳定。
/// 所以**向上遍历进程树，返回第一个属于该 agent 的祖先**（= 会话本体，终端关掉才会退出）。
/// 精确匹配进程名（非子串），避免名字恰好含 "claude"/"kimi" 的包装进程被误认。
/// 找不到则返回 None（宁可不记 PID，也不记错的）。
///
/// **只认 `provider` 自己的进程名**，不是「任一 agent」。区别自 gemini 起变得要命：它没有自己的
/// 可执行，会话本体就是个 node 进程，故 `node` 进了它的白名单——而全局判定下 `node.exe` 从此对
/// **所有** agent 为真。用全局判定上溯，任何 agent 的 hook 都可能在父链上撞见一个无关的 node
/// 祖先（VS Code 的集成终端就是），把它当成自己的会话本体；两个不同 agent 的会话于是认领到同一个
/// pid，而 `set_session_pid` 的 pid 独占语义会让后来者把前者收尾成 `ended`——两个活着的会话互相抹杀。
///
/// 实测确认过：gemini 与 opencode 的会话正是这样抢到同一个 node 祖先、互相顶掉的。
///
/// 未注册的 provider → None：不认识它，就不该替它猜一个 PID。
#[cfg(target_os = "windows")]
pub fn owner_pid(provider: &str) -> Option<u32> {
    use sysinfo::{ProcessRefreshKind, RefreshKind, System};
    let agent = meowo_agent::by_id(provider)?;
    let sys =
        System::new_with_specifics(RefreshKind::new().with_processes(ProcessRefreshKind::new()));
    let mut cur = sysinfo::get_current_pid().ok()?;
    // 最多向上走 16 层，避免异常环导致死循环。
    for _ in 0..16 {
        let parent = sys.process(cur)?.parent()?;
        let name = sys
            .process(parent)
            .map(|p| p.name().to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if agent.owns_process(&name) {
            return Some(parent.as_u32());
        }
        cur = parent;
    }
    None
}

/// 仅在需要归属/重启时读目标命令行；不可读、插件未声明都保守为 Unknown。
/// 此处也复核旧数据库中的 PID，不能仅相信升级后新增的 shared 标记。
pub fn process_scope(pid: u32) -> meowo_agent::registry::ProcessScope {
    use meowo_agent::registry::ProcessScope;
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let target = sysinfo::Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[target]),
        false,
        ProcessRefreshKind::new().with_cmd(UpdateKind::Always),
    );
    let Some(process) = sys.process(target) else {
        return ProcessScope::Unknown;
    };
    let argv: Vec<String> = process
        .cmd()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    scope_for_command(&process.name().to_string_lossy(), &argv)
}

fn scope_for_command(name: &str, argv: &[String]) -> meowo_agent::registry::ProcessScope {
    merge_scopes(
        meowo_agent::all()
            .iter()
            .filter(|plugin| plugin.owns_process(name))
            .map(|plugin| plugin.process_scope(argv)),
    )
}

fn merge_scopes(
    scopes: impl Iterator<Item = meowo_agent::registry::ProcessScope>,
) -> meowo_agent::registry::ProcessScope {
    use meowo_agent::registry::ProcessScope;
    let mut known = None;
    for scope in scopes.filter(|scope| *scope != ProcessScope::Unknown) {
        if known.is_some_and(|previous| previous != scope) {
            return ProcessScope::Unknown;
        }
        known = Some(scope);
    }
    known.unwrap_or(ProcessScope::Unknown)
}

/// 共享服务的多个会话合法持有同 PID，不能走独占 CLI 的换代驱逐。
pub fn record_owner_pid(
    store: &meowo_store::Store,
    session_id: i64,
    provider: &str,
    now_ms: i64,
) -> Result<(), meowo_store::StoreError> {
    use meowo_agent::registry::ProcessScope;
    if let Some(pid) = owner_pid(provider) {
        match process_scope(pid) {
            ProcessScope::Exclusive => store.set_session_pid(session_id, i64::from(pid), now_ms)?,
            ProcessScope::Shared => {
                store.set_session_shared_pid(session_id, i64::from(pid), now_ms)?
            }
            // 认不出作用域时仍要记下进程号：不记则判活失败，「接管」看不到原进程会再起一个
            // 同会话进程（做空复现：gemini 带 --max-old-space-size 重启自身时）。按共享记，
            // 判活照常、又不会被当成独占进程去结束或驱逐别的会话。
            ProcessScope::Unknown => {
                store.set_session_shared_pid(session_id, i64::from(pid), now_ms)?
            }
        }
    }
    Ok(())
}

/// Unix（macOS/Linux）实现：用 `ps` 上溯进程树，而非 sysinfo。
///
/// 实测 macOS 上 sysinfo 的 `parent()` 不可靠——会在某些进程（如 login）过早返回 None，
/// 导致父链断裂、找不到 claude 祖先（贴纸因此恒显示「断开」）。`ps -o ppid=` 给的父 pid 则准确，
/// 故 Unix 下改用 ps 取 ppid + comm 上溯，返回第一个 comm 为 claude 的祖先
/// （comm 可能是全路径，取 basename 后精确比较，不做子串包含）。
#[cfg(not(target_os = "windows"))]
pub fn owner_pid(provider: &str) -> Option<u32> {
    let agent = meowo_agent::by_id(provider)?;
    let mut pid = ps_ppid(std::process::id())?; // 从 reporter 的父进程起
    for _ in 0..16 {
        if pid <= 1 {
            return None; // 到 launchd/init 边界仍没找到该 agent
        }
        if ps_comm(pid).is_some_and(|c| agent.owns_process(&c)) {
            return Some(pid);
        }
        pid = ps_ppid(pid)?;
    }
    None
}

/// `ps -o ppid= -p <pid>` → 父进程 pid。
#[cfg(not(target_os = "windows"))]
fn ps_ppid(pid: u32) -> Option<u32> {
    let out = std::process::Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// `ps -o comm= -p <pid>` → 进程命令名（macOS 可能是可执行文件全路径，含 "claude" 即可匹配）。
#[cfg(not(target_os = "windows"))]
fn ps_comm(pid: u32) -> Option<String> {
    // -ww：Darwin 的 ps 非 tty 输出按 79 列截断，而 macOS 的 comm 是全路径。
    let out = std::process::Command::new("ps")
        .args(["-ww", "-o", "comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    use meowo_agent::registry::ProcessScope;

    #[test]
    fn unknown_name_candidate_does_not_mask_a_later_script_match() {
        assert_eq!(
            merge_scopes([ProcessScope::Unknown, ProcessScope::Exclusive].into_iter()),
            ProcessScope::Exclusive
        );
        assert_eq!(
            merge_scopes([ProcessScope::Shared, ProcessScope::Exclusive].into_iter()),
            ProcessScope::Unknown
        );
        let args = [
            "node.exe",
            "C:/npm/node_modules/@google/gemini-cli/bundle/gemini.js",
        ]
        .map(str::to_owned);
        assert_eq!(
            scope_for_command("node.exe", &args),
            ProcessScope::Exclusive
        );
        assert_eq!(
            scope_for_command("node.exe", &["node.exe".into(), "unrelated.js".into()]),
            ProcessScope::Unknown
        );
        assert_eq!(
            scope_for_command(
                "kimi.exe",
                &["C:/Users/test/.kimi-code/bin/kimi.exe".into()]
            ),
            ProcessScope::Exclusive
        );
    }
}
