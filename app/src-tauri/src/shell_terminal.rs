//! 对话窗右侧面板里的「终端」页签：在会话目录开一个普通 shell，给人手动跑命令、看日志。
//!
//! 跟托管终端（pty.rs 的 PtyBroker）刻意分开：那边一个会话一条 agent 进程，挂着屏幕识别、
//! 审批 broker、接续链和 scoped token；这里只是一个裸 shell，什么都不认，也不进会话表。
//! 混进 broker 会让它的「一会话一 PTY」假设和关窗兜底全部要改，得不偿失。
//! 形态参照 Kimi Code 1.3.1 的右侧面板（2026-11-06 拆包与实操：`kimi:terminal-create`
//! 一类独立通道，页签标题就是 shell 名 `pwsh`）。
//!
//! 生命周期归后端：页签切走、面板收起、换会话都只是前端 detach，shell 照跑（`npm run dev`
//! 开着切去看别的会话再回来，不能被杀）；重新 attach 时先回放环形缓冲里的历史输出。
//! 只有用户关页签（close）或对话窗销毁时才结束进程。
//!
//! 手机端一律不给：在电脑上开 shell 属于宿主执行类，命令全不进 remote.rs 白名单。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use base64::Engine;
use portable_pty::{native_pty_system, Child, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::ipc::Channel;

/// 重新 attach 时回放的历史输出上限。够看一屏多的日志，又不至于每次切页签都搬几 MB。
const BACKLOG_CAP: usize = 512 * 1024;

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ShellTerminalEvent {
    /// base64 编码的原始字节：PTY 输出可能在多字节字符中间断开，按字符串传会被替换成乱码。
    Output {
        data: String,
        /// 这段是前端已经显示过的历史，重新挂上时再放一遍。里面可能有 shell 当初发的
        /// 终端查询（如设备属性 `ESC[c`），那时已经答过了；重放时前端不能再答，否则答案
        /// 会当成键盘输入落进 shell，提示符后面多出一串 `[?1;2c`（2026-11-06 实拍）。
        replay: bool,
    },
    Exit {
        code: Option<u32>,
    },
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShellTerminalInfo {
    id: u32,
    /// 归属的会话 id：页签跟着会话走，换会话看到的是那个会话自己的几个终端。
    owner: i64,
    title: String,
    exited: bool,
}

struct Output {
    backlog: Vec<u8>,
    /// 当前 attach 的前端通道与它的代次；detach 只清自己那一代，防止旧组件卸载时
    /// 把新挂上的通道顺手摘掉（React 重挂时先挂新的、后卸旧的）。
    channel: Option<(u64, Channel<ShellTerminalEvent>)>,
    exit: Option<Option<u32>>,
    /// 累计产出的字节数，和其中已经送到过前端的字节数。两者之差是没人看时攒下的新输出，
    /// 重新挂上时这部分照常当实时输出处理（里面的终端查询还没人答）。
    produced: u64,
    delivered: u64,
}

impl Output {
    /// 挂上新通道时要发的两段：已经显示过的（重放）和还没显示过的（当实时）。
    fn split_for_attach(&self) -> (&[u8], &[u8]) {
        let unseen = (self.produced - self.delivered).min(self.backlog.len() as u64) as usize;
        self.backlog.split_at(self.backlog.len() - unseen)
    }
}

/// 结束一个 shell：先收它的子孙进程（shell 里跑着的 dev server 之类），再杀 shell。
/// 只杀 shell 的话，从它起的进程可能成孤儿继续占着端口。
fn stop(shell: &mut ShellPty) {
    if let Some(pid) = shell.pid {
        super::proc::kill_descendants(pid);
    }
    let _ = shell.killer.kill();
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShellProcess {
    pid: u32,
    name: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShellBusy {
    id: u32,
    title: String,
    processes: Vec<ShellProcess>,
}

impl ShellBusy {
    /// 「标题 · 程序 (PID n), ...」：与关窗确认（chat/windowClose.ts confirmCloseShells）同一写法。
    pub(crate) fn describe(&self) -> String {
        let processes: Vec<String> = self
            .processes
            .iter()
            .map(|p| format!("{} (PID {})", if p.name.is_empty() { "?" } else { &p.name }, p.pid))
            .collect();
        format!("{} · {}", self.title, processes.join(", "))
    }
}

/// 进程表：pid -> (父 pid, 进程名)。
fn process_table() -> HashMap<u32, (u32, String)> {
    #[cfg(target_os = "windows")]
    {
        super::proc::snapshot_processes()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut map = HashMap::new();
        let Ok(out) = std::process::Command::new("ps")
            .args(["-axo", "pid=,ppid=,comm="])
            .output()
        else {
            return map;
        };
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let mut it = line.split_whitespace();
            let (Some(pid), Some(ppid)) = (
                it.next().and_then(|p| p.parse().ok()),
                it.next().and_then(|p| p.parse().ok()),
            ) else {
                continue;
            };
            let name = it.collect::<Vec<_>>().join(" ");
            let name = std::path::Path::new(&name)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(name);
            map.insert(pid, (ppid, name));
        }
        map
    }
}

/// shell 底下正在跑的程序（不含 shell 自己）。空 = 终端空闲，可以直接关。
fn running_under(root: u32, table: &HashMap<u32, (u32, String)>) -> Vec<ShellProcess> {
    let parents: HashMap<u32, u32> = table.iter().map(|(pid, (ppid, _))| (*pid, *ppid)).collect();
    let mut list: Vec<ShellProcess> = super::proc::descendant_pids(&parents, root)
        .into_iter()
        .map(|pid| ShellProcess {
            pid,
            name: table
                .get(&pid)
                .map(|(_, name)| name.clone())
                .unwrap_or_default(),
        })
        // ConPTY 自己的宿主不算「用户在跑的程序」。
        .filter(|p| {
            !matches!(
                p.name.to_ascii_lowercase().as_str(),
                "conhost.exe" | "openconsole.exe"
            )
        })
        .collect();
    list.sort_by_key(|p| p.pid);
    list
}

struct ShellPty {
    owner: i64,
    title: String,
    master: Box<dyn MasterPty + Send>,
    /// 输入交给每个终端自己的写线程：PTY 写满（shell 不读输入时粘一大段）会阻塞，
    /// 在命令里直接写就会卡住调用它的线程，还攥着全部终端共用的表锁。通道保证按键顺序。
    input: std::sync::mpsc::Sender<Vec<u8>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    /// shell 自己的 pid：判断「终端里有没有程序在跑」和关闭时收子孙进程都靠它。
    pid: Option<u32>,
    output: Arc<Mutex<Output>>,
}

#[derive(Default)]
pub(crate) struct ShellTerminals {
    next_id: Mutex<u32>,
    next_gen: Mutex<u64>,
    shells: Mutex<HashMap<u32, ShellPty>>,
}

impl ShellTerminals {
    fn info(id: u32, shell: &ShellPty) -> ShellTerminalInfo {
        let exited = shell
            .output
            .lock()
            .map(|o| o.exit.is_some())
            .unwrap_or(true);
        ShellTerminalInfo {
            id,
            owner: shell.owner,
            title: shell.title.clone(),
            exited,
        }
    }

    /// 对话窗销毁时调用：窗口没了就没人能再看到或关掉这些 shell，留着只会变成孤儿进程。
    pub(crate) fn kill_all(&self) {
        let drained: Vec<ShellPty> = match self.shells.lock() {
            Ok(mut map) => map.drain().map(|(_, shell)| shell).collect(),
            Err(_) => return,
        };
        for mut shell in drained {
            stop(&mut shell);
        }
    }
}

/// 选哪个 shell。Windows 优先 PowerShell 7，没有就用系统自带的 5.1；
/// 其他平台用登录 shell（`-l`），GUI 启动的进程 PATH 不全，登录 shell 会把用户配置读进来。
fn shell_command() -> (CommandBuilder, String) {
    #[cfg(target_os = "windows")]
    {
        let exe = if super::terminal::pwsh_available() {
            "pwsh.exe"
        } else {
            "powershell.exe"
        };
        let mut command = CommandBuilder::new(exe);
        command.arg("-NoLogo");
        (command, exe.trim_end_matches(".exe").to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "/bin/zsh".into());
        let title = std::path::Path::new(&shell)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "shell".into());
        let mut command = CommandBuilder::new(&shell);
        command.arg("-l");
        (command, title)
    }
}

fn append_backlog(backlog: &mut Vec<u8>, chunk: &[u8]) {
    backlog.extend_from_slice(chunk);
    if backlog.len() > BACKLOG_CAP {
        let drop = backlog.len() - BACKLOG_CAP;
        backlog.drain(..drop);
    }
}

fn encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[tauri::command]
pub(crate) async fn shell_terminal_open(
    state: tauri::State<'_, ShellTerminals>,
    owner: i64,
    cwd: String,
    cols: u16,
    rows: u16,
) -> Result<ShellTerminalInfo, String> {
    state.open(owner, &cwd, cols, rows)
}

impl ShellTerminals {
    fn open(
        &self,
        owner: i64,
        cwd: &str,
        cols: u16,
        rows: u16,
    ) -> Result<ShellTerminalInfo, String> {
        if !std::path::Path::new(&cwd).is_dir() {
            return Err(format!("目录不存在: {cwd}"));
        }
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: rows.max(2),
                cols: cols.max(10),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
        let (mut command, title) = shell_command();
        command.cwd(cwd);
        command.env_remove("NO_COLOR");
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        let mut child: Box<dyn Child + Send + Sync> = pair
            .slave
            .spawn_command(command)
            .map_err(|e| e.to_string())?;
        // slave 端不放掉的话，shell 退出后 reader 读不到 EOF，退出事件永远不来。
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let (input, inbox) = std::sync::mpsc::channel::<Vec<u8>>();
        // 终端关掉时 ShellPty 连同 Sender 一起 drop，recv 失败，线程自己退出。
        std::thread::spawn(move || {
            while let Ok(bytes) = inbox.recv() {
                if writer.write_all(&bytes).and_then(|()| writer.flush()).is_err() {
                    break;
                }
            }
        });
        let killer = child.clone_killer();
        let pid = child.process_id();
        let output = Arc::new(Mutex::new(Output {
            backlog: Vec::new(),
            channel: None,
            exit: None,
            produced: 0,
            delivered: 0,
        }));

        let id = {
            let mut next = self.next_id.lock().map_err(|e| e.to_string())?;
            *next += 1;
            *next
        };

        let reader_output = output.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let Ok(mut out) = reader_output.lock() else {
                    break;
                };
                append_backlog(&mut out.backlog, &buf[..n]);
                out.produced += n as u64;
                if let Some((_, channel)) = &out.channel {
                    let _ = channel.send(ShellTerminalEvent::Output {
                        data: encode(&buf[..n]),
                        replay: false,
                    });
                    out.delivered = out.produced;
                }
            }
        });
        let waiter_output = output.clone();
        std::thread::spawn(move || {
            let code = child.wait().ok().map(|status| status.exit_code());
            if let Ok(mut out) = waiter_output.lock() {
                out.exit = Some(code);
                if let Some((_, channel)) = &out.channel {
                    let _ = channel.send(ShellTerminalEvent::Exit { code });
                }
            }
        });

        let shell = ShellPty {
            owner,
            title,
            master: pair.master,
            input,
            killer,
            pid,
            output,
        };
        let info = ShellTerminals::info(id, &shell);
        self.shells
            .lock()
            .map_err(|e| e.to_string())?
            .insert(id, shell);
        Ok(info)
    }

    /// 会话换了编号（新会话认领、/clear 换代）：终端归属跟过去。
    pub(crate) fn reassign_owner(&self, from: i64, to: i64) {
        if let Ok(mut map) = self.shells.lock() {
            for shell in map.values_mut().filter(|shell| shell.owner == from) {
                shell.owner = to;
            }
        }
    }

    /// 会话结束 / 关闭时收掉它在面板里开的所有终端（连同终端里跑着的程序）。
    pub(crate) fn close_owner(&self, owner: i64) {
        let drained: Vec<ShellPty> = match self.shells.lock() {
            Ok(mut map) => {
                let ids: Vec<u32> = map
                    .iter()
                    .filter(|(_, shell)| shell.owner == owner)
                    .map(|(id, _)| *id)
                    .collect();
                ids.into_iter().filter_map(|id| map.remove(&id)).collect()
            }
            Err(_) => return,
        };
        for mut shell in drained {
            stop(&mut shell);
        }
    }

    fn write(&self, id: u32, data: &str) -> Result<(), String> {
        let map = self.shells.lock().map_err(|e| e.to_string())?;
        let shell = map.get(&id).ok_or("shell_terminal/not_found")?;
        shell
            .input
            .send(data.as_bytes().to_vec())
            .map_err(|_| "shell_terminal/exited".to_string())
    }

    fn close(&self, id: u32) {
        let removed = self.shells.lock().ok().and_then(|mut map| map.remove(&id));
        if let Some(mut shell) = removed {
            stop(&mut shell);
        }
    }
}

/// 关页签前问一句：这些终端里还有没有程序在跑（参照 Kimi Code 的「关闭正在运行的终端？」）。
/// 只返回有程序在跑的那些；已经退出或空闲的不出现。
#[tauri::command]
pub(crate) async fn shell_terminal_busy(
    state: tauri::State<'_, ShellTerminals>,
    ids: Vec<u32>,
) -> Result<Vec<ShellBusy>, String> {
    let roots = state.live_roots(Some(&ids))?;
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let table = tauri::async_runtime::spawn_blocking(process_table)
        .await
        .map_err(|e| e.to_string())?;
    Ok(busy_from(roots, &table))
}

impl ShellTerminals {
    /// 还活着的终端（编号, shell pid, 标题）；`ids` 为 None 时取全部。
    fn live_roots(&self, ids: Option<&[u32]>) -> Result<Vec<(u32, u32, String)>, String> {
        let map = self.shells.lock().map_err(|e| e.to_string())?;
        let mut roots: Vec<(u32, u32, String)> = map
            .iter()
            .filter(|(id, _)| ids.is_none_or(|ids| ids.contains(id)))
            .filter_map(|(id, shell)| {
                let exited = shell.output.lock().map(|o| o.exit.is_some()).unwrap_or(true);
                if exited {
                    return None;
                }
                Some((*id, shell.pid?, shell.title.clone()))
            })
            .collect();
        roots.sort_by_key(|(id, ..)| *id);
        Ok(roots)
    }

    /// 全部面板终端里有程序在跑的那些（托盘退出前问一句用，同步取进程表，调用方放 blocking 池）。
    pub(crate) fn busy_all(&self) -> Vec<ShellBusy> {
        let roots = self.live_roots(None).unwrap_or_default();
        if roots.is_empty() {
            return Vec::new();
        }
        busy_from(roots, &process_table())
    }
}

fn busy_from(roots: Vec<(u32, u32, String)>, table: &HashMap<u32, (u32, String)>) -> Vec<ShellBusy> {
    roots
        .into_iter()
        .filter_map(|(id, pid, title)| {
            let processes = running_under(pid, table);
            (!processes.is_empty()).then_some(ShellBusy { id, title, processes })
        })
        .collect()
}

#[tauri::command]
pub(crate) fn shell_terminal_list(
    state: tauri::State<'_, ShellTerminals>,
    owner: i64,
) -> Vec<ShellTerminalInfo> {
    let Ok(map) = state.shells.lock() else {
        return Vec::new();
    };
    let mut list: Vec<ShellTerminalInfo> = map
        .iter()
        .filter(|(_, shell)| shell.owner == owner)
        .map(|(id, shell)| ShellTerminals::info(*id, shell))
        .collect();
    list.sort_by_key(|info| info.id);
    list
}

/// 全部会话的面板终端。关整扇对话窗前用：窗口 Destroyed 时 kill_all 收的是这里的全部，前端
/// 只按当前会话列（shell_terminal_list）的话，「重新加载」后别的会话的终端不在它账上，关窗
/// 就不弹确认、直接杀掉（做空审查）。
#[tauri::command]
pub(crate) fn shell_terminal_list_all(state: tauri::State<'_, ShellTerminals>) -> Vec<ShellTerminalInfo> {
    state.list_all()
}

impl ShellTerminals {
    fn list_all(&self) -> Vec<ShellTerminalInfo> {
        let Ok(map) = self.shells.lock() else {
            return Vec::new();
        };
        let mut list: Vec<ShellTerminalInfo> = map.iter().map(|(id, shell)| Self::info(*id, shell)).collect();
        list.sort_by_key(|info| info.id);
        list
    }
}

/// 挂上前端通道：先在同一把锁里把历史输出作为第一帧发出去，之后的输出接着走同一条通道，
/// 回放与实时输出之间不会漏字节也不会重复。返回代次，detach 时带回来。
#[tauri::command]
pub(crate) fn shell_terminal_attach(
    state: tauri::State<'_, ShellTerminals>,
    id: u32,
    on_event: Channel<ShellTerminalEvent>,
) -> Result<u64, String> {
    let output = {
        let map = state.shells.lock().map_err(|e| e.to_string())?;
        map.get(&id)
            .ok_or("shell_terminal/not_found")?
            .output
            .clone()
    };
    let generation = {
        let mut next = state.next_gen.lock().map_err(|e| e.to_string())?;
        *next += 1;
        *next
    };
    let mut out = output.lock().map_err(|e| e.to_string())?;
    let (seen, unseen) = out.split_for_attach();
    for (part, replay) in [(seen, true), (unseen, false)] {
        if !part.is_empty() {
            let _ = on_event.send(ShellTerminalEvent::Output {
                data: encode(part),
                replay,
            });
        }
    }
    out.delivered = out.produced;
    if let Some(code) = out.exit {
        let _ = on_event.send(ShellTerminalEvent::Exit { code });
    }
    out.channel = Some((generation, on_event));
    Ok(generation)
}

#[tauri::command]
pub(crate) fn shell_terminal_detach(
    state: tauri::State<'_, ShellTerminals>,
    id: u32,
    generation: u64,
) {
    let output = match state.shells.lock() {
        Ok(map) => map.get(&id).map(|shell| shell.output.clone()),
        Err(_) => None,
    };
    if let Some(output) = output {
        if let Ok(mut out) = output.lock() {
            if out
                .channel
                .as_ref()
                .is_some_and(|(current, _)| *current == generation)
            {
                out.channel = None;
            }
        }
    }
}

#[tauri::command]
pub(crate) fn shell_terminal_write(
    state: tauri::State<'_, ShellTerminals>,
    id: u32,
    data: String,
) -> Result<(), String> {
    state.write(id, &data)
}

#[tauri::command]
pub(crate) fn shell_terminal_resize(
    state: tauri::State<'_, ShellTerminals>,
    id: u32,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let map = state.shells.lock().map_err(|e| e.to_string())?;
    let shell = map.get(&id).ok_or("shell_terminal/not_found")?;
    shell
        .master
        .resize(PtySize {
            rows: rows.max(2),
            cols: cols.max(10),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
}

/// 关页签 = 结束这个 shell。从表里摘掉后再杀，锁外做，杀进程可能要等一下。
#[tauri::command]
pub(crate) fn shell_terminal_close(state: tauri::State<'_, ShellTerminals>, id: u32) {
    state.close(id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_splits_seen_history_from_new_output() {
        let history = b"prompt> \x1b[c";
        let mut out = Output {
            backlog: history.to_vec(),
            channel: None,
            exit: None,
            produced: history.len() as u64,
            delivered: history.len() as u64,
        };
        // 全部显示过：整段都是重放，里面的 ESC[c 不能再答。
        assert_eq!(out.split_for_attach(), (&history[..], &b""[..]));
        // 没人看时又来了 4 个字节：这 4 个字节当实时输出。
        out.backlog.extend_from_slice(b"log\n");
        out.produced += 4;
        assert_eq!(out.split_for_attach(), (&history[..], &b"log\n"[..]));
        // 从没挂过前端：全是新输出。
        out.delivered = 0;
        assert_eq!(out.split_for_attach().0, &b""[..]);
    }

    #[test]
    fn busy_lists_programs_under_the_shell_but_not_the_console_host() {
        let table: HashMap<u32, (u32, String)> = [
            (10, (1, "pwsh.exe".to_string())),
            (11, (10, "node.exe".to_string())),
            (12, (11, "esbuild.exe".to_string())),
            (13, (10, "conhost.exe".to_string())),
            (20, (1, "other.exe".to_string())),
        ]
        .into_iter()
        .collect();
        let names: Vec<String> = running_under(10, &table)
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["node.exe", "esbuild.exe"]);
        assert!(running_under(20, &table).is_empty());
    }

    /// 会话结束只收它自己的终端，别的会话的不动。
    #[test]
    fn close_owner_only_closes_that_sessions_shells() {
        let cwd = std::env::temp_dir().to_string_lossy().into_owned();
        let shells = ShellTerminals::default();
        shells.open(1, &cwd, 80, 24).unwrap();
        shells.open(1, &cwd, 80, 24).unwrap();
        let keep = shells.open(-2, &cwd, 80, 24).unwrap();
        // 新会话认领真实编号：-2 → 2，之后按 2 关。
        shells.reassign_owner(-2, 2);
        shells.close_owner(1);
        let left: Vec<u32> = shells.shells.lock().unwrap().keys().copied().collect();
        assert_eq!(left, vec![keep.id]);
        shells.close_owner(2);
        assert!(shells.shells.lock().unwrap().is_empty());
    }

    #[test]
    fn backlog_keeps_only_the_tail() {
        let mut backlog = Vec::new();
        append_backlog(&mut backlog, &vec![b'a'; BACKLOG_CAP]);
        append_backlog(&mut backlog, b"xyz");
        assert_eq!(backlog.len(), BACKLOG_CAP);
        assert!(backlog.ends_with(b"xyz"));
        assert_eq!(backlog[0], b'a');
    }

    fn backlog_text(shells: &ShellTerminals, id: u32) -> String {
        let map = shells.shells.lock().unwrap();
        let out = map[&id].output.lock().unwrap();
        String::from_utf8_lossy(&out.backlog).into_owned()
    }

    /// 测试里充当终端：ConPTY 启动时发光标位置查询 `ESC[6n`（portable-pty 建伪终端带
    /// PSEUDOCONSOLE_INHERIT_CURSOR）和设备属性查询 `ESC[c`，等终端回话。
    #[derive(Default)]
    struct StartupQueries {
        cursor: usize,
        device: usize,
    }

    impl StartupQueries {
        /// 新出现的查询各答一次。
        fn answers(&mut self, text: &str) -> Vec<&'static str> {
            let (cursor, device) = (text.matches("\x1b[6n").count(), text.matches("\x1b[c").count());
            let mut out = Vec::new();
            out.extend(std::iter::repeat_n("\x1b[1;1R", cursor.saturating_sub(self.cursor)));
            out.extend(std::iter::repeat_n("\x1b[?1;0c", device.saturating_sub(self.device)));
            (self.cursor, self.device) = (cursor.max(self.cursor), device.max(self.device));
            out
        }

        /// shell 已经画出了可见字符（提示符之类），不再只有控制序列。
        fn settled(&self, text: &str) -> bool {
            let mut chars = text.chars().peekable();
            while let Some(c) = chars.next() {
                match c {
                    '\x1b' => match chars.next() {
                        // CSI：参数直到 0x40–0x7e 的终止字节。
                        Some('[') => {
                            for c in chars.by_ref() {
                                if ('\x40'..='\x7e').contains(&c) {
                                    break;
                                }
                            }
                        }
                        // OSC：直到 BEL 或 ST（ESC \）。
                        Some(']') => {
                            while let Some(c) = chars.next() {
                                if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                                    break;
                                }
                            }
                        }
                        _ => {}
                    },
                    c if !c.is_whitespace() && !c.is_control() => return true,
                    _ => {}
                }
            }
            false
        }
    }

    #[test]
    fn startup_queries_answered_once_and_settle_on_visible_text() {
        let mut queries = StartupQueries::default();
        let head = "\x1b[1t\x1b[6n\x1b[c\x1b[?1004h\x1b[?9001h\x1b]0;C:\\pwsh.exe\x1b\\";
        assert_eq!(queries.answers(head), ["\x1b[1;1R", "\x1b[?1;0c"]);
        assert!(queries.answers(head).is_empty());
        assert!(!queries.settled(head));
        assert!(queries.settled(&format!("{head}\x1b[1;1HPS C:\\> ")));
    }

    /// 真起一个 shell：工作目录对、能敲命令、关掉后从表里消失。
    #[test]
    fn opens_shell_in_cwd_and_runs_commands() {
        let dir = std::env::temp_dir().join(format!("meowo-shell-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cwd = dir.to_string_lossy().into_owned();
        let shells = ShellTerminals::default();
        let info = shells.open(42, &cwd, 120, 30).unwrap();
        assert_eq!(info.owner, 42);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let leaf = dir.file_name().unwrap().to_string_lossy().into_owned();
        let mut queries = StartupQueries::default();
        let mut typed = false;
        loop {
            let text = backlog_text(&shells, info.id);
            // 像真终端一样答 ConPTY 的启动查询（产品里由前端 xterm 答）。不答的话 ConPTY 要等到
            // 自己超时才放 shell 往下跑：二期实测本机空载 8 轮 3.9–10.1s，答了 0.8–1.9s；
            // 全量并发跑时叠上负载就超过 20s（CI 失败日志里 backlog 正停在这几个查询加标题）。
            for answer in queries.answers(&text) {
                shells.write(info.id, answer).unwrap();
            }
            // 握手答完再敲命令，等同用户看到提示符才打字。
            if !typed && queries.settled(&text) {
                // `$((6*7))` 在 PowerShell 和 POSIX shell 里都算出 42；回显的命令行里没有 42，
                // 看到它就说明命令真被执行了。
                shells.write(info.id, "echo meowo-shell-$((6*7))\r").unwrap();
                #[cfg(target_os = "windows")]
                shells.write(info.id, "(Get-Location).Path\r").unwrap();
                #[cfg(not(target_os = "windows"))]
                shells.write(info.id, "pwd\r").unwrap();
                typed = true;
            }
            if text.contains("meowo-shell-42") && text.contains(&leaf) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "shell 没有输出预期内容：{text}"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        shells.close(info.id);
        assert!(shells.shells.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
