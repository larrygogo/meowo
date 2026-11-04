//! 诊断日志：安装版把进程的标准错误输出整体接进 `~/.meowo/logs/meowo.log`。
//!
//! 为什么接 stderr 而不是逐个改调用点：后端已有 50 多处 `eprintln!` 诊断，安装版是窗口
//! 程序，stderr 没有去处，这些信息全丢了——用户报「提问卡卡死」时事后无据可查，只能等
//! 复现（2026-11-04）。接管 stderr 后，现有 `eprintln!`、panic 信息、继承了 stderr 的子进程
//! 输出都自动落盘，调用点一行不改。dev（debug 构建）不接，照旧打在终端。
//!
//! 中间隔一条管道和一个转发线程，逐行补时间戳、按大小轮换。**转发线程绝不能停**：
//! 它一停，管道写满后任何 `eprintln!` 都会阻塞（包括主线程，整个应用未响应）；读端没了
//! 则写入报错，`eprintln!` 写失败会 panic。所以写文件失败只丢这一行、继续读，线程里没有
//! 任何会 panic 的路径。

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

/// 单个日志文件的上限。超过就把当前文件挪成 `meowo.1.log`（覆盖上一份），总量约 4 MB。
const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// 前端单条诊断的长度上限：防一条超长屏幕文本把日志刷掉。
const MAX_UI_MESSAGE_CHARS: usize = 4000;

fn log_dir() -> PathBuf {
    crate::setup::meowo_dir().join("logs")
}

fn current_log() -> PathBuf {
    log_dir().join("meowo.log")
}

fn previous_log() -> PathBuf {
    log_dir().join("meowo.1.log")
}

/// 启动时调用一次。只在 release 生效；任何一步失败都退回原状（stderr 照旧丢弃），不影响启动。
pub(crate) fn install() {
    if cfg!(debug_assertions) {
        return;
    }
    let Ok((reader, writer)) = std::io::pipe() else {
        return;
    };
    // 先起转发线程再改 stderr：顺序反过来的话，线程起不来时 stderr 已指向一条没人读的管道。
    let spawned = std::thread::Builder::new()
        .name("diag-log".into())
        .spawn(move || forward(reader, &current_log(), &previous_log()));
    if spawned.is_err() {
        return;
    }
    if redirect_stderr(writer) {
        eprintln!(
            "Meowo {} 启动 · {} {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
    }
}

#[cfg(windows)]
fn redirect_stderr(writer: std::io::PipeWriter) -> bool {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::System::Console::{SetStdHandle, STD_ERROR_HANDLE};
    // 写端交给进程级 stderr 后不再归还（进程存活期间一直要用），故 into_raw 不关闭。
    // Rust 的 eprintln! 每次写都重新 GetStdHandle，改完立即生效。
    let handle = writer.into_raw_handle();
    // SAFETY: handle 是刚取得的有效管道写端，所有权移交给进程 stderr。
    unsafe { SetStdHandle(STD_ERROR_HANDLE, handle as _) != 0 }
}

#[cfg(target_os = "macos")]
fn redirect_stderr(writer: std::io::PipeWriter) -> bool {
    use std::os::fd::IntoRawFd;
    let fd = writer.into_raw_fd();
    // SAFETY: fd 是有效的管道写端；dup2 到 2 号后原 fd 不再需要。
    let ok = unsafe { libc::dup2(fd, 2) } != -1;
    unsafe { libc::close(fd) };
    ok
}

#[cfg(not(any(windows, target_os = "macos")))]
fn redirect_stderr(_writer: std::io::PipeWriter) -> bool {
    false
}

/// 转发循环：逐行读管道、补时间戳写盘、超限轮换。见模块注释「绝不能停」。
fn forward(reader: impl Read, current: &Path, previous: &Path) {
    if let Some(dir) = current.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut file = open_append(current);
    let mut written = std::fs::metadata(current).map(|m| m.len()).unwrap_or(0);
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            // 写端还在进程手里，读到 EOF 只可能是写端被关——没人会再写了，可以退。
            Ok(0) => return,
            Ok(_) => {}
            // 读错不退出：退了读端，eprintln! 写失败就会 panic。歇一下再读，免得空转。
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }
        }
        if written >= MAX_BYTES {
            drop(file.take());
            let _ = std::fs::remove_file(previous);
            let _ = std::fs::rename(current, previous);
            file = open_append(current);
            written = 0;
        }
        if !line.ends_with(b"\n") {
            line.push(b'\n');
        }
        let stamp = format!("{} ", utc_stamp(std::time::SystemTime::now()));
        if let Some(f) = file.as_mut() {
            if f.write_all(stamp.as_bytes()).and_then(|_| f.write_all(&line)).is_ok() {
                written += (stamp.len() + line.len()) as u64;
            }
        }
    }
}

fn open_append(path: &Path) -> Option<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()
}

/// `2026-11-04T07:12:03Z`。不为一个时间戳引入日期库：按 Howard Hinnant 的 civil_from_days。
fn utc_stamp(at: std::time::SystemTime) -> String {
    let secs = at
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// 前端写一条诊断（如「提问卡出现 N 秒仍没认出终端表单」）。经 stderr 走同一条落盘路径；
/// dev 下打在终端。换行压成 ` | `，一条诊断占一行，按时间戳 grep 得到。
#[tauri::command]
pub(crate) async fn diag_log(scope: String, message: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        eprintln!("[ui:{}] {}", one_line(&scope, 40), one_line(&message, MAX_UI_MESSAGE_CHARS));
    })
    .await
    .map_err(|e| e.to_string())
}

fn one_line(text: &str, max_chars: usize) -> String {
    let flat = text.replace("\r\n", "\n").replace(['\r', '\n'], " | ");
    if flat.chars().count() <= max_chars {
        flat
    } else {
        flat.chars().take(max_chars).collect::<String>() + "…"
    }
}

/// 设置 › 关于 的「导出诊断日志」：把上一份与当前日志合成一个文本文件写到用户选的位置。
#[tauri::command]
pub(crate) async fn export_diag_log(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || export_blocking(&path, &previous_log(), &current_log()))
        .await
        .map_err(|e| e.to_string())?
}

fn export_blocking(target: &str, previous: &Path, current: &Path) -> Result<(), String> {
    let lower = target.to_lowercase();
    if !(lower.ends_with(".txt") || lower.ends_with(".log")) {
        return Err("diag/bad_extension".into());
    }
    let old = std::fs::read(previous).unwrap_or_default();
    let new = std::fs::read(current).unwrap_or_default();
    if old.is_empty() && new.is_empty() {
        return Err("diag/no_log".into());
    }
    let mut out = format!(
        "Meowo {} · {} {} · 导出于 {}\n\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        utc_stamp(std::time::SystemTime::now())
    )
    .into_bytes();
    out.extend_from_slice(&old);
    out.extend_from_slice(&new);
    std::fs::write(target, out).map_err(|e| format!("diag/write_failed：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("meowo_diaglog_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn utc_stamp_formats_known_instants() {
        let at = |secs: u64| std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        assert_eq!(utc_stamp(at(0)), "1970-01-01T00:00:00Z");
        // 2024-02-29 闰日、跨月边界。
        assert_eq!(utc_stamp(at(1_709_208_000)), "2024-02-29T12:00:00Z");
        assert_eq!(utc_stamp(at(1_793_257_553)), "2026-10-29T07:05:53Z");
    }

    /// 逐行补时间戳；超过上限时当前文件挪成 .1、新开一份继续写，旧的 .1 被覆盖。
    #[test]
    fn forward_stamps_lines_and_rotates() {
        let dir = tmp_dir("fwd");
        let (current, previous) = (dir.join("meowo.log"), dir.join("meowo.1.log"));
        std::fs::write(&previous, "更早的一份\n").unwrap();
        // 预先填到上限，下一行就该触发轮换。
        std::fs::write(&current, vec![b'x'; MAX_BYTES as usize]).unwrap();
        forward(&b"first line\nno newline at end"[..], &current, &previous);
        let rotated = std::fs::read(&previous).unwrap();
        assert_eq!(rotated.len(), MAX_BYTES as usize, "满了的那份整体挪成 .1，覆盖更早的");
        let text = std::fs::read_to_string(&current).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("Z first line"), "{}", lines[0]);
        assert!(lines[1].ends_with("Z no newline at end"), "{}", lines[1]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_line_flattens_and_truncates() {
        assert_eq!(one_line("a\r\nb\nc", 100), "a | b | c");
        assert_eq!(one_line("一二三四五", 3), "一二三…");
    }

    #[test]
    fn export_merges_both_files_and_checks_extension() {
        let dir = tmp_dir("export");
        let (current, previous) = (dir.join("meowo.log"), dir.join("meowo.1.log"));
        let target = dir.join("out.txt");
        let target_str = target.to_str().unwrap();
        assert_eq!(export_blocking(target_str, &previous, &current), Err("diag/no_log".into()));
        std::fs::write(&previous, "old\n").unwrap();
        std::fs::write(&current, "new\n").unwrap();
        export_blocking(target_str, &previous, &current).unwrap();
        let text = std::fs::read_to_string(&target).unwrap();
        assert!(text.starts_with("Meowo "));
        assert!(text.ends_with("old\nnew\n"), "先旧后新：{text}");
        let bad = dir.join("out.exe");
        assert_eq!(
            export_blocking(bad.to_str().unwrap(), &previous, &current),
            Err("diag/bad_extension".into())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
