//! 真机探针共用：进程快照（核对「TUI 不在时作答不起任何后台进程」）。只在测试里编译。
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone)]
pub(crate) struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub cmd: String,
}

/// 全机进程表（Win32_Process）。拿不到时返回空表，调用方据此判「无法核对」。
pub(crate) fn snapshot() -> BTreeMap<u32, Proc> {
    let out = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,CommandLine | ConvertTo-Json -Compress",
        ])
        .output();
    let Ok(out) = out else {
        return BTreeMap::new();
    };
    let list: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some((
                p["ProcessId"].as_u64()? as u32,
                Proc {
                    pid: p["ProcessId"].as_u64()? as u32,
                    ppid: p["ParentProcessId"].as_u64().unwrap_or(0) as u32,
                    name: p["Name"].as_str().unwrap_or_default().to_owned(),
                    cmd: p["CommandLine"].as_str().unwrap_or_default().to_owned(),
                },
            ))
        })
        .collect()
}

/// `after` 里新出现、且与本次探针有关的进程：是本测试进程的后代，或命令行里带隔离目录 `marker`。
/// 用户机器上同时跑着别的会话，全机新进程不能直接算到被测功能头上。
pub(crate) fn new_related(
    before: &BTreeMap<u32, Proc>,
    after: &BTreeMap<u32, Proc>,
    marker: &str,
) -> Vec<Value> {
    let me = std::process::id();
    let mut ours: HashSet<u32> = HashSet::from([me]);
    // 进程表里父子关系可能乱序：迭代到不动点。
    loop {
        let grown = after
            .values()
            .filter(|p| ours.contains(&p.ppid) && !ours.contains(&p.pid))
            .map(|p| p.pid)
            .collect::<Vec<_>>();
        if grown.is_empty() {
            break;
        }
        ours.extend(grown);
    }
    let marker = marker.to_lowercase();
    after
        .values()
        .filter(|p| !before.contains_key(&p.pid))
        .filter(|p| {
            (ours.contains(&p.pid) && p.pid != me)
                || (!marker.is_empty() && p.cmd.to_lowercase().contains(&marker))
        })
        // 快照本身起的 powershell 不算。
        .filter(|p| !p.cmd.contains("Get-CimInstance Win32_Process"))
        .map(|p| serde_json::json!({"pid": p.pid, "ppid": p.ppid, "name": p.name, "cmd": p.cmd}))
        .collect()
}
