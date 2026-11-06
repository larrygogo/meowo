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
}

struct ShellPty {
    owner: i64,
    title: String,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
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
            let _ = shell.killer.kill();
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
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let killer = child.clone_killer();
        let output = Arc::new(Mutex::new(Output {
            backlog: Vec::new(),
            channel: None,
            exit: None,
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
                if let Some((_, channel)) = &out.channel {
                    let _ = channel.send(ShellTerminalEvent::Output {
                        data: encode(&buf[..n]),
                    });
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
            writer,
            killer,
            output,
        };
        let info = ShellTerminals::info(id, &shell);
        self.shells
            .lock()
            .map_err(|e| e.to_string())?
            .insert(id, shell);
        Ok(info)
    }

    fn write(&self, id: u32, data: &str) -> Result<(), String> {
        let mut map = self.shells.lock().map_err(|e| e.to_string())?;
        let shell = map.get_mut(&id).ok_or("shell_terminal/not_found")?;
        shell
            .writer
            .write_all(data.as_bytes())
            .map_err(|e| e.to_string())?;
        shell.writer.flush().map_err(|e| e.to_string())
    }

    fn close(&self, id: u32) {
        let removed = self.shells.lock().ok().and_then(|mut map| map.remove(&id));
        if let Some(mut shell) = removed {
            let _ = shell.killer.kill();
        }
    }
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
    if !out.backlog.is_empty() {
        let _ = on_event.send(ShellTerminalEvent::Output {
            data: encode(&out.backlog),
        });
    }
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

    /// 真起一个 shell：工作目录对、能敲命令、关掉后从表里消失。
    #[test]
    fn opens_shell_in_cwd_and_runs_commands() {
        let dir = std::env::temp_dir().join(format!("meowo-shell-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cwd = dir.to_string_lossy().into_owned();
        let shells = ShellTerminals::default();
        let info = shells.open(42, &cwd, 120, 30).unwrap();
        assert_eq!(info.owner, 42);
        // `$((6*7))` 在 PowerShell 和 POSIX shell 里都算出 42；回显的命令行里没有 42，
        // 看到它就说明命令真被执行了。
        shells
            .write(info.id, "echo meowo-shell-$((6*7))\r")
            .unwrap();
        #[cfg(target_os = "windows")]
        shells.write(info.id, "(Get-Location).Path\r").unwrap();
        #[cfg(not(target_os = "windows"))]
        shells.write(info.id, "pwd\r").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let leaf = dir.file_name().unwrap().to_string_lossy().into_owned();
        loop {
            let text = backlog_text(&shells, info.id);
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
