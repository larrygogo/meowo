//! 交给用户的文件（MCP 工具 send_file）与 AI 主动提醒（notify_user）。任务 20261123-agent-abilities。
//!
//! send_file：复制一份快照到 `<数据根>/files/sent/<内容指纹>/<原文件名>`，对话里显示成文件卡。
//! 存快照而不是记原路径：之后 agent 再改原文件，用户拿到的仍是交付那一刻的版本（Claude.ai
//! present_files 的坑是用户下载到旧版，调研 research/landscape.md 第 2 节）。同内容重复交只存一份。
//!
//! notify_user：走用户已有的通知通道（系统通知 + Bark），遵守通知总开关；「做完了 / 普通消息」在用户
//! 正看这个会话时不弹，「等你决定」照弹（与 watch.rs suppressed_by_viewing 同一非对称纪律）；同一会话
//! 一分钟内重复的合并。回执如实告诉 agent 结果，不假装成功（竞品「工具说成功、手机没收到」的坑）。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// 回执里给前端认的文件卡摘要行开头（FileCard.tsx 认这个）。
pub(crate) const FILE_MARKER: &str = "[File: ";
/// 提醒回执的摘要行开头（NotifyLine 认这个）：级别、内容、实际去向。
pub(crate) const NOTIFY_MARKER: &str = "[Notify: ";
/// 单个文件上限：再大复制一次就要好几秒、占一大块磁盘，交付物（安装包、报告、日志包）远到不了。
pub(crate) const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
/// 交付目录总量上限：超出按最近一次交付时间删最旧的。
const TOTAL_CAP: u64 = 2 * 1024 * 1024 * 1024;
const CAPTION_MAX: usize = 300;

/// 交付目录：`<数据根>/files/sent`。手机下载路由与桌面「打开 / 另存为」只认这里面的文件。
pub(crate) fn files_root() -> PathBuf {
    crate::setup::meowo_dir().join("files").join("sent")
}

pub(crate) fn send_file(args: &Value) -> Result<Value, String> {
    send_into(&files_root(), args).map(|text| json!({ "text": text }))
}

fn send_into(root: &Path, args: &Value) -> Result<String, String> {
    let raw = args.get("path").and_then(Value::as_str).map(str::trim).unwrap_or("");
    if raw.is_empty() {
        return Err("没给文件路径：path 要填本机文件的路径".into());
    }
    if raw.contains("://") {
        return Err("send_file 只交本机文件，不接受网址。要交网上的文件，先用你自己的下载工具存到本地".into());
    }
    let source = PathBuf::from(raw);
    if !source.is_absolute() {
        return Err(format!("路径要是绝对路径：{raw}"));
    }
    let meta = std::fs::metadata(&source).map_err(|_| format!("找不到这个文件：{raw}"))?;
    if meta.is_dir() {
        return Err(format!(
            "这是一个文件夹：{raw}。send_file 一次交一个文件，先把文件夹打成 zip 再交"
        ));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(format!(
            "文件有 {}，超过 {} 的上限：{raw}。压缩后再交，或者在回复里告诉用户文件在哪",
            human_size(meta.len()),
            human_size(MAX_FILE_BYTES)
        ));
    }
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(|| format!("认不出文件名：{raw}"))?;
    std::fs::create_dir_all(root).map_err(|e| format!("建交付目录失败：{e}"))?;
    let (snapshot, size, digest) = snapshot_into(root, &source, &name)?;
    prune(root, TOTAL_CAP, snapshot.parent());
    let caption = args
        .get("caption")
        .and_then(Value::as_str)
        .map(|c| c.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|c| !c.is_empty())
        .map(|c| c.chars().take(CAPTION_MAX).collect::<String>());
    let card = json!({
        "name": name,
        "size": size,
        "path": snapshot.to_string_lossy(),
        "sha256": digest,
        "caption": caption,
    });
    Ok(format!(
        "文件已交给用户：{name}（{}）。对话里会显示成文件卡，用户可以打开或下载；不用再贴路径。用户拿到的是现在这一份，之后改了原文件要重新交。\n{FILE_MARKER}{card}]",
        human_size(size)
    ))
}

/// 边复制边算指纹：先写进交付目录里的临时文件，算完再挪到 `<指纹>/<文件名>`。同内容已交过就
/// 只刷新时间（回收按它算新旧），不再存第二份。
fn snapshot_into(root: &Path, source: &Path, name: &str) -> Result<(PathBuf, u64, String), String> {
    let mut input = std::fs::File::open(source).map_err(|e| format!("读不了这个文件：{e}"))?;
    let temp = root.join(format!(".incoming-{}-{}", std::process::id(), nanos()));
    let result = (|| {
        let mut output = std::fs::File::create(&temp).map_err(|e| format!("写快照失败：{e}"))?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 256 * 1024];
        let mut size = 0u64;
        loop {
            let read = input.read(&mut buffer).map_err(|e| format!("读文件失败：{e}"))?;
            if read == 0 {
                break;
            }
            size += read as u64;
            if size > MAX_FILE_BYTES {
                return Err(format!("文件在复制时变得超过 {} 的上限", human_size(MAX_FILE_BYTES)));
            }
            hasher.update(&buffer[..read]);
            output.write_all(&buffer[..read]).map_err(|e| format!("写快照失败：{e}"))?;
        }
        output.flush().map_err(|e| format!("写快照失败：{e}"))?;
        let digest: String = hasher.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect();
        Ok((size, digest))
    })();
    let (size, digest) = match result {
        Ok(ok) => ok,
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            return Err(error);
        }
    };
    let dir = root.join(&digest);
    let target = dir.join(name);
    if target.is_file() {
        let _ = std::fs::remove_file(&temp);
    } else {
        std::fs::create_dir_all(&dir).map_err(|e| format!("建交付目录失败：{e}"))?;
        if let Err(error) = std::fs::rename(&temp, &target) {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("存快照失败：{error}"));
        }
    }
    touch(&dir);
    Ok((target, size, digest))
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// 刷新目录时间：回收按「最近一次交付」排新旧。
fn touch(dir: &Path) {
    let marker = dir.join(".delivered");
    let _ = std::fs::write(&marker, nanos().to_string());
}

fn delivered_at(dir: &Path) -> std::time::SystemTime {
    std::fs::metadata(dir.join(".delivered"))
        .and_then(|m| m.modified())
        .or_else(|_| std::fs::metadata(dir).and_then(|m| m.modified()))
        .unwrap_or(std::time::UNIX_EPOCH)
}

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

/// 总量超过上限时按最近一次交付时间删最旧的整份（`<指纹>/` 目录）；刚交的那份不删。
/// 还清掉中途失败留下的临时文件。删掉的在对话里显示「已失效」（前端读不到文件时）。
pub(crate) fn prune(root: &Path, cap: u64, keep: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    let mut dirs: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(".incoming-") {
            // 一小时以前的半截临时文件：复制中途崩了留下的。
            let stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > Duration::from_secs(3600));
            if stale {
                let _ = std::fs::remove_file(&path);
            }
            continue;
        }
        if path.is_dir() {
            dirs.push((delivered_at(&path), dir_size(&path), path));
        }
    }
    let mut total: u64 = dirs.iter().map(|d| d.1).sum();
    dirs.sort_by_key(|d| d.0);
    for (_, size, path) in dirs {
        if total <= cap {
            break;
        }
        if keep.is_some_and(|k| k == path.as_path()) {
            continue;
        }
        if std::fs::remove_dir_all(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

pub(crate) fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// 交付目录里的一个文件（桌面「打开 / 在文件夹中显示 / 另存为」、手机下载共用的校验）：
/// 展开 `..`、符号链接、junction 后必须仍在交付目录里，且是文件。
pub(crate) fn delivered_file(path: &str) -> Option<PathBuf> {
    delivered_in(&files_root(), path)
}

fn delivered_in(root: &Path, path: &str) -> Option<PathBuf> {
    let real = meowo_agent::images::resolve_within(&[root.to_path_buf()], Path::new(path))?;
    real.is_file().then_some(real)
}

#[tauri::command]
pub(crate) async fn open_delivered_file(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let file = delivered_file(&path).ok_or("delivered/missing")?;
        crate::fsutil::open_with_default_app(&file)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn reveal_delivered_file(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let file = delivered_file(&path).ok_or("delivered/missing")?;
        crate::fsutil::reveal_target(&file)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 另存为：源只认交付目录（这条命令不能变成「复制任意文件」）；目标来自系统保存框。
#[tauri::command]
pub(crate) async fn save_delivered_file(path: String, dest: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let file = delivered_file(&path).ok_or("delivered/missing")?;
        let dest = PathBuf::from(dest.trim());
        if dest.as_os_str().is_empty() || dest.is_dir() {
            return Err("delivered/invalid_dest".to_string());
        }
        std::fs::copy(&file, &dest)
            .map(|_| ())
            .map_err(|e| format!("delivered/write_failed: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 文件卡打开时问一句：快照还在不在（被回收了就显示「已失效」）。
#[tauri::command]
pub(crate) async fn delivered_file_exists(path: String) -> bool {
    tauri::async_runtime::spawn_blocking(move || delivered_file(&path).is_some())
        .await
        .unwrap_or(false)
}

// ── notify_user ────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    NeedsYou,
    Done,
    Info,
}

impl Level {
    fn parse(raw: Option<&str>) -> Level {
        match raw.map(str::trim) {
            Some("needs_you") => Level::NeedsYou,
            Some("info") => Level::Info,
            _ => Level::Done,
        }
    }
}

/// 这次提醒的去向。回执按它如实写给 agent。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Shown,
    Viewing,
    Disabled,
    Throttled,
}

/// 同一会话两次提醒的最短间隔：再密就合并（只留第一条）。等你决定的不受限：卡住了就该叫到人。
const THROTTLE: Duration = Duration::from_secs(60);
const MESSAGE_MAX: usize = 300;

static LAST_NOTIFY: Mutex<Option<HashMap<i64, Instant>>> = Mutex::new(None);

/// 纯判定（单测用）：通知开着吗、用户正看着吗、离上一条多久。
pub(crate) fn decide(level: Level, enabled: bool, viewing: bool, since_last: Option<Duration>) -> Outcome {
    if !enabled {
        return Outcome::Disabled;
    }
    if viewing && level != Level::NeedsYou {
        return Outcome::Viewing;
    }
    if level != Level::NeedsYou && since_last.is_some_and(|d| d < THROTTLE) {
        return Outcome::Throttled;
    }
    Outcome::Shown
}

pub(crate) fn notify_user(app: &tauri::AppHandle, session_id: i64, args: &Value) -> Result<Value, String> {
    use tauri::Manager;
    let message: String = args
        .get("message")
        .and_then(Value::as_str)
        .map(|m| m.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
        .chars()
        .take(MESSAGE_MAX)
        .collect();
    if message.is_empty() {
        return Err("没给提醒内容：message 要写一两句话".into());
    }
    let level = Level::parse(args.get("level").and_then(Value::as_str));
    let settings = crate::settings::load_settings();
    let chat_focused = app
        .get_window("chat")
        .and_then(|window| window.is_focused().ok())
        .unwrap_or(false);
    let viewed = app
        .try_state::<crate::AppState>()
        .map(|state| state.ptys.viewed_session_ids())
        .unwrap_or_default();
    let viewing = crate::watch::is_viewing_session(chat_focused, &viewed, session_id);
    let now = Instant::now();
    let since_last = {
        let guard = LAST_NOTIFY.lock().ok();
        guard
            .as_ref()
            .and_then(|map| map.as_ref())
            .and_then(|map| map.get(&session_id))
            .map(|at| now.duration_since(*at))
    };
    let outcome = decide(level, settings.notifications_enabled, viewing, since_last);
    if outcome == Outcome::Shown {
        if let Ok(mut guard) = LAST_NOTIFY.lock() {
            guard.get_or_insert_with(HashMap::new).insert(session_id, now);
        }
        crate::watch::show_agent_notification(app, &settings, session_id, level == Level::NeedsYou, &message);
    }
    let text = match outcome {
        Outcome::Shown => "已提醒用户（系统通知；设置了 Bark 的话手机也会收到）。",
        Outcome::Viewing => "用户正在看这个会话，没有弹通知；你的话会显示在对话里。",
        Outcome::Disabled => "用户在 Meowo 里关了通知，这条没有弹出来；你的话会显示在对话里。",
        Outcome::Throttled => "一分钟内刚提醒过，这条合并了，没有再弹。不要频繁提醒。",
    };
    let level_name = match level {
        Level::NeedsYou => "needs_you",
        Level::Done => "done",
        Level::Info => "info",
    };
    let outcome_name = match outcome {
        Outcome::Shown => "shown",
        Outcome::Viewing => "viewing",
        Outcome::Disabled => "disabled",
        Outcome::Throttled => "throttled",
    };
    let card = json!({ "level": level_name, "message": message, "outcome": outcome_name });
    Ok(json!({ "text": format!("{text}
{NOTIFY_MARKER}{card}]") }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("meowo-files-{tag}-{}-{}", std::process::id(), nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn card_of(text: &str) -> Value {
        let start = text.find(FILE_MARKER).unwrap() + FILE_MARKER.len();
        serde_json::from_str(text[start..].trim_end().trim_end_matches(']')).unwrap()
    }

    #[test]
    fn delivers_a_snapshot_that_later_edits_do_not_touch() {
        let work = temp_dir("work");
        let root = temp_dir("root");
        let source = work.join("报告.txt");
        std::fs::write(&source, b"v1").unwrap();
        let text = send_into(&root, &json!({ "path": source.to_string_lossy(), "caption": "  第一版\n报告 " })).unwrap();
        let card = card_of(&text);
        assert_eq!(card["name"], "报告.txt");
        assert_eq!(card["size"], 2);
        assert_eq!(card["caption"], "第一版 报告");
        let snapshot = PathBuf::from(card["path"].as_str().unwrap());
        assert!(snapshot.starts_with(&root));
        std::fs::write(&source, b"v2 changed").unwrap();
        assert_eq!(std::fs::read(&snapshot).unwrap(), b"v1");
        // 同内容再交一次：同一份快照，不存第二份。
        std::fs::write(&source, b"v1").unwrap();
        let again = card_of(&send_into(&root, &json!({ "path": source.to_string_lossy() })).unwrap());
        assert_eq!(again["path"], card["path"]);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    }

    #[test]
    fn rejects_folders_missing_files_urls_and_relative_paths() {
        let root = temp_dir("root2");
        let work = temp_dir("work2");
        let err = |args: Value| send_into(&root, &args).unwrap_err();
        assert!(err(json!({ "path": work.to_string_lossy() })).contains("文件夹"));
        assert!(err(json!({ "path": work.join("nope.zip").to_string_lossy() })).contains("找不到"));
        assert!(err(json!({ "path": "https://example.com/a.zip" })).contains("网址"));
        assert!(err(json!({ "path": "a.zip" })).contains("绝对路径"));
        assert!(err(json!({})).contains("path"));
    }

    /// 桌面打开 / 另存为、手机下载只认交付目录：`..` 跳出去、目录本身、目录外的文件都不认。
    #[test]
    fn only_files_inside_the_delivery_root_are_served() {
        let root = temp_dir("root4");
        let outside = temp_dir("outside4");
        std::fs::write(outside.join("secret.txt"), b"x").unwrap();
        let source = outside.join("ok.txt");
        std::fs::write(&source, b"ok").unwrap();
        let card = card_of(&send_into(&root, &json!({ "path": source.to_string_lossy() })).unwrap());
        let snapshot = card["path"].as_str().unwrap().to_string();
        assert!(delivered_in(&root, &snapshot).is_some());
        let escape = root.join("..").join(outside.file_name().unwrap()).join("secret.txt");
        assert!(delivered_in(&root, &escape.to_string_lossy()).is_none());
        assert!(delivered_in(&root, &outside.join("secret.txt").to_string_lossy()).is_none());
        assert!(delivered_in(&root, &root.to_string_lossy()).is_none());
    }

    #[test]
    fn prune_drops_the_oldest_delivery_but_keeps_the_new_one() {
        let root = temp_dir("root3");
        for (i, name) in ["old", "mid", "new"].iter().enumerate() {
            let dir = root.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("f.bin"), vec![0u8; 100]).unwrap();
            touch(&dir);
            std::thread::sleep(Duration::from_millis(20 + i as u64));
        }
        prune(&root, 150, Some(&root.join("new")));
        assert!(!root.join("old").exists());
        assert!(!root.join("mid").exists());
        assert!(root.join("new").exists());
    }

    #[test]
    fn notify_decisions_follow_settings_viewing_and_throttle() {
        assert_eq!(decide(Level::Done, false, false, None), Outcome::Disabled);
        assert_eq!(decide(Level::Done, true, true, None), Outcome::Viewing);
        assert_eq!(decide(Level::Info, true, true, None), Outcome::Viewing);
        // 等你决定：正看着也弹，也不限频。
        assert_eq!(decide(Level::NeedsYou, true, true, Some(Duration::from_secs(1))), Outcome::Shown);
        assert_eq!(decide(Level::Done, true, false, Some(Duration::from_secs(10))), Outcome::Throttled);
        assert_eq!(decide(Level::Done, true, false, Some(Duration::from_secs(61))), Outcome::Shown);
        assert_eq!(Level::parse(Some("needs_you")), Level::NeedsYou);
        assert_eq!(Level::parse(Some("whatever")), Level::Done);
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(22_235_771), "21.2 MB");
        assert_eq!(human_size(512 * 1024 * 1024), "512 MB");
    }
}
