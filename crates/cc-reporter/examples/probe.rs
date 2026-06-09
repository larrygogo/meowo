//! 临时诊断：看 sysinfo 在本平台对 claude 进程返回的 name/exe/cmd 与父链。
//! 用法：cargo run -p cc-reporter --example probe <pid>
//!   <pid> 传一个活着的 claude 会话进程 pid（用 `ps axo pid,comm,args | grep claude` 找）。
//! 输出 1：所有 name/exe/cmd 含 "claude" 的进程；输出 2：从该 pid 向上的父链（每层 name/exe）。

use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System, UpdateKind};

fn main() {
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(
            ProcessRefreshKind::new()
                .with_exe(UpdateKind::Always)
                .with_cmd(UpdateKind::Always),
        ),
    );

    println!("=== (1) 所有 name/exe/cmd 含 'claude' 的进程 ===");
    let mut hit = 0;
    for (pid, p) in sys.processes() {
        let name = p.name().to_string_lossy().into_owned();
        let exe = p
            .exe()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        let cmd = p
            .cmd()
            .iter()
            .map(|c| c.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        let l = format!("{name} {exe} {cmd}").to_ascii_lowercase();
        if l.contains("claude") {
            hit += 1;
            println!(
                "pid={pid} parent={:?}\n    name={name:?}\n    exe ={exe:?}\n    cmd ={cmd:?}",
                p.parent()
            );
        }
    }
    if hit == 0 {
        println!("（无匹配——sysinfo 可能根本读不到 claude 进程的 name/exe/cmd）");
    }

    let Some(pid) = std::env::args().nth(1).and_then(|s| s.parse::<u32>().ok()) else {
        println!("\n（未传 pid，跳过父链诊断。用法：... --example probe <pid>）");
        return;
    };
    println!("\n=== (2) 从 pid {pid} 向上的父链 ===");
    let mut cur = Some(Pid::from_u32(pid));
    for depth in 0..16 {
        let Some(c) = cur else {
            println!("[{depth}] parent=None（链断了）");
            break;
        };
        match sys.process(c) {
            Some(p) => {
                let name = p.name().to_string_lossy().into_owned();
                let exe = p
                    .exe()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default();
                println!("[{depth}] pid={c} name={name:?} exe={exe:?} parent={:?}", p.parent());
                cur = p.parent();
            }
            None => {
                println!("[{depth}] pid={c} <不在快照里>");
                break;
            }
        }
    }
}
