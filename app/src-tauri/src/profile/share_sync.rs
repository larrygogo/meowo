//! 把默认账号里用户配置的东西带进某个账号：执行 [`meowo_agent::profile::ShareSurface`] 的声明，
//! 照 Orca（stablyai/orca c1bd89d）的多账号做法（2026-11-27 leader 决定完全照做）。
//!
//! 同步规则写在 `ShareSurface` 的文档里，这里只实现。按条目同步的记录存在账号根的 [`RECORD_FILE`]，
//! 只存摘要不存值（设置里可能有密钥，不在别处再存一份）。目录链接与历史合并见 [`super::share_link`]。
//!
//! 何时调用：建号、Meowo 启动（补老账号）、以及**每次在该账号里拉起 agent 之前**——用户在默认
//! 目录里改了设置，下一个会话就能用上，不必重启 Meowo。所以这里要快：无变化时只读不写。
//!
//! 写进默认目录的只有两种（都是 leader 授权、Orca 同样做的）：账号的会话历史与输入历史并进来，
//! 以及首次对齐时账号里已有的共享目录（skills、plugins…）的条目并进来。冲突一律不覆盖。
//! 读不懂的文件（坏 JSON/TOML）整份跳过，不拿空值去覆盖。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use meowo_agent::profile::{ShareFormat, SharePick, ShareSurface};
use serde_json::{Map, Value};

use super::share_link::{self, LinkOutcome};

/// 同步记录：`{ 面: { 条目: 摘要 } }`。放在账号根，agent 不认识它，也不会动它。
pub(crate) const RECORD_FILE: &str = ".meowo-share.json";

/// 第一次改写账号里某个结构化文件之前留的备份后缀（只留一次，之后不再覆盖）。
const BACKUP_SUFFIX: &str = ".meowo-share.bak";

/// 条目路径的分隔符。项目路径里可能有 `/`、`\`、`.`，用控制字符才不会撞。
const SEP: char = '\u{1f}';

/// 记录里的元信息面：`setup` 记首次对齐做过没有（Orca 的 setup marker）。
const META: &str = "meta";
/// 记录里「默认目录的文件复制到账号」那部分：文件名 → 上次写进账号的内容摘要。
const COPIED: &str = "copied";
const SETUP_DONE: &str = "orca-1";

/// 同一时刻只让一个同步在跑：两个会话同时在同一账号下启动时，各自读改写同一份 settings 会互相覆盖。
static SYNC_LOCK: Mutex<()> = Mutex::new(());

/// 一次同步的结果。`changed` 写进日志，便于排查「为什么账号里多了/少了某项」。
#[derive(Debug, Default)]
pub(crate) struct SyncReport {
    pub(crate) changed: Vec<String>,
    pub(crate) errors: Vec<String>,
    /// 改写了哪些结构化文件（相对数据目录）。改了 settings.json 的话 hooks 可能被默认的整份替换，
    /// 调用方要重新接一次 Meowo 的线。
    pub(crate) rewritten: Vec<&'static str>,
}

/// 同步时要用到的几处路径。
pub(crate) struct SyncDirs<'a> {
    /// 用户 home（`.claude.json` 这类放在 home 根上的默认文件相对它）。
    pub(crate) home: &'a Path,
    /// 默认账号的数据目录（如 `~/.claude`）。
    pub(crate) default_data: &'a Path,
    /// 该账号的数据目录。
    pub(crate) profile_data: &'a Path,
    /// 同步记录文件。
    pub(crate) record: &'a Path,
}

type Record = BTreeMap<String, BTreeMap<String, String>>;

pub(crate) fn sync(surfaces: &[ShareSurface], dirs: &SyncDirs<'_>) -> SyncReport {
    let _guard = SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut report = SyncReport::default();
    let original = load_record(dirs.record);
    let mut record = original.clone();
    let first_setup = record.get(META).and_then(|m| m.get("setup")).map(String::as_str) != Some(SETUP_DONE);
    let mut setup_ok = true;

    for surface in surfaces {
        match *surface {
            ShareSurface::AllEntries { exclude, exclude_prefixes, import } => {
                setup_ok &= sync_all_entries(exclude, exclude_prefixes, import, first_setup, dirs, &mut record, &mut report);
            }
            ShareSurface::HistoryDirs { dirs: names } => {
                for name in names {
                    let mut warnings = Vec::new();
                    let source = dirs.profile_data.join(name);
                    let destination = dirs.default_data.join(name);
                    let started = std::time::Instant::now();
                    match share_link::merge_dir_into_link(&source, &destination, true, &mut warnings) {
                        Ok(LinkOutcome::Linked) => report.changed.push(format!(
                            "会话历史 {name} 并入默认目录并改成链接（{} ms）",
                            started.elapsed().as_millis()
                        )),
                        Ok(LinkOutcome::UserOwned) => {
                            report.errors.push(format!("{}: 是指向别处的链接或文件，未共享", source.display()))
                        }
                        Ok(_) => {}
                        Err(e) => report.errors.push(e),
                    }
                    report.errors.extend(warnings);
                }
            }
            ShareSurface::PromptHistory { file } => {
                let mut warnings = Vec::new();
                match share_link::merge_prompt_history(dirs.profile_data, dirs.default_data, file, &mut warnings) {
                    Ok(LinkOutcome::Linked) => report.changed.push(format!("输入历史 {file} 并入默认目录并改成硬链接")),
                    Ok(LinkOutcome::UserOwned) => {
                        report.errors.push(format!("{}: 是链接或目录，未共享", dirs.profile_data.join(file).display()))
                    }
                    Ok(_) => {}
                    Err(e) => report.errors.push(e),
                }
                report.errors.extend(warnings);
            }
            ShareSurface::Entries { dir, skip } => sync_entries(dir, skip, dirs, &mut record, &mut report),
            ShareSurface::File { path } => {
                let desired = read_file(&dirs.default_data.join(path));
                sync_file(&format!("file:{path}"), &dirs.profile_data.join(path), desired, &mut record, &mut report);
            }
            ShareSurface::ClaudeState { file, default_from_home, keys, project_keys } => sync_claude_state(
                &dirs.home.join(default_from_home),
                &dirs.profile_data.join(file),
                file,
                keys,
                project_keys,
                &mut report,
            ),
            // 结构化文件按文件分组处理（一个文件上可能有好几个面），见下面。
            ShareSurface::Keys { .. } => {}
        }
    }

    let mut files: Vec<&'static str> = Vec::new();
    for surface in surfaces {
        if let ShareSurface::Keys { file, .. } = *surface {
            if !files.contains(&file) {
                files.push(file);
            }
        }
    }
    for file in files {
        let group: Vec<&ShareSurface> = surfaces
            .iter()
            .filter(|s| matches!(**s, ShareSurface::Keys { file: f, .. } if f == file))
            .collect();
        sync_structured(file, &group, dirs, &mut record, &mut report);
    }

    if first_setup && setup_ok && surfaces.iter().any(|s| matches!(s, ShareSurface::AllEntries { .. })) {
        record.entry(META.to_string()).or_default().insert("setup".to_string(), SETUP_DONE.to_string());
    }
    if record != original {
        if let Err(e) = save_record(dirs.record, &record) {
            report.errors.push(format!("{}: {e}", dirs.record.display()));
        }
    }
    report
}

// ═══ 默认目录的全部条目（Orca provisionClaudeProfile） ═══

/// 返回首次对齐是否完整做完（目录合并出错时下次还要再做）。
fn sync_all_entries(
    exclude: &[&str],
    exclude_prefixes: &[&str],
    import: &[&str],
    first_setup: bool,
    dirs: &SyncDirs<'_>,
    record: &mut Record,
    report: &mut SyncReport,
) -> bool {
    let mut ok = true;
    let mut names: Vec<String> = match std::fs::read_dir(dirs.default_data) {
        Ok(entries) => entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return true,
        Err(e) => {
            report.errors.push(format!("{}: {e}", dirs.default_data.display()));
            return false;
        }
    };
    names.retain(|name| {
        !name.starts_with('.')
            && !exclude.contains(&name.as_str())
            && !exclude_prefixes.iter().any(|p| name.starts_with(p))
            // 我们自己留下的待合并目录、备份。
            && !name.contains(share_link::MERGE_PENDING_SUFFIX)
            && !name.ends_with(BACKUP_SUFFIX)
    });
    names.sort();
    for name in names {
        let source = dirs.default_data.join(&name);
        let target = dirs.profile_data.join(&name);
        // 跟随链接判断类型：用户的条目本身常是指向别处的链接。
        if source.is_dir() {
            match share_link::link_entry_dir(&source, &target) {
                Ok(LinkOutcome::Linked) => report.changed.push(format!("链接 {}", target.display())),
                // 首次对齐时账号里的真实目录是 CLI 或旧版 Meowo 建的（比如插件清单、逐个链接的 skill），
                // 不是用户有意给这个账号单独放的：并进默认目录再链（Orca 同样只在首次这样做）。
                Ok(LinkOutcome::UserOwned) if first_setup && target.is_dir() && !is_link(&target) => {
                    let mut warnings = Vec::new();
                    match share_link::merge_dir_into_link(&target, &source, false, &mut warnings) {
                        Ok(LinkOutcome::Linked) => {
                            report.changed.push(format!("{} 的条目并入默认目录并改成链接", target.display()))
                        }
                        Ok(_) => {}
                        Err(e) => {
                            ok = false;
                            report.errors.push(e);
                        }
                    }
                    report.errors.extend(warnings);
                }
                Ok(_) => {}
                Err(e) => {
                    ok = false;
                    report.errors.push(e);
                }
            }
        } else if source.is_file() {
            let copied = record.entry(COPIED.to_string()).or_default();
            if import.contains(&name.as_str()) {
                let line = format!("@{}", import_path(&source, dirs.home));
                copy_import(&target, &line, &name, copied, report);
            } else if let Some(desired) = read_file(&source) {
                copy_file(&target, &desired, &name, copied, report);
            }
        }
    }
    ok
}

/// `<文件>.meowo-profile-merge`，被占了就加序号。
fn free_aside(target: &Path) -> PathBuf {
    let base = format!("{}{}", target.display(), share_link::MERGE_PENDING_SUFFIX);
    let mut n = 0;
    loop {
        let candidate = if n == 0 { PathBuf::from(&base) } else { PathBuf::from(format!("{base}-{n}")) };
        if std::fs::symlink_metadata(&candidate).is_err() {
            return candidate;
        }
        n += 1;
    }
}

fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// Orca `copyClaudeProfileFile`：内容不同就用默认的覆盖（文件不用链接：编辑器「写临时文件再改名」
/// 的保存会让链接断开）。账号里同名的是链接时那是用户自己的，不碰。
///
/// 与 Orca 不同：只有账号里的内容还是上次 Meowo 写进去的那份（`copied` 记着摘要）才直接覆盖；
/// 否则是用户在这个账号里改过（比如 `/keybindings`），或首次对齐时账号原有的文件，先改名留存再写。
/// 红队 1 复现：Orca 的做法每次开会话都把账号里改过的快捷键静默写回默认的；真实账号副本上还会
/// 丢掉账号自己的 `settings.json.bak-*`（默认目录里恰好有同名备份）。
fn copy_file(target: &Path, desired: &[u8], name: &str, copied: &mut BTreeMap<String, String>, report: &mut SyncReport) {
    match std::fs::symlink_metadata(target) {
        Ok(m) if !m.is_file() => return,
        Ok(_) => match read_file(target) {
            Some(current) if current == desired => {
                copied.insert(name.to_string(), digest(desired));
                return;
            }
            Some(current) if copied.get(name) == Some(&digest(&current)) => {}
            _ => {
                if !set_aside(target, report) {
                    return;
                }
            }
        },
        Err(_) => {}
    }
    match write_bytes_atomic(target, desired) {
        Ok(()) => {
            copied.insert(name.to_string(), digest(desired));
            report.changed.push(format!("写入 {}", target.display()));
        }
        Err(e) => report.errors.push(format!("{}: {e}", target.display())),
    }
}

/// 把账号里的文件改名留存。失败就不覆盖，返回 false。
fn set_aside(target: &Path, report: &mut SyncReport) -> bool {
    let aside = free_aside(target);
    if let Err(e) = std::fs::rename(target, &aside) {
        report.errors.push(format!("{}: 留存原文件失败，不覆盖：{e}", target.display()));
        return false;
    }
    report.changed.push(format!("账号原有的 {} 留存为 {}", target.display(), aside.display()));
    true
}

/// CLAUDE.md 这类：账号里放一行 `@默认文件` 引用。已有这一行就不动——用户在账号里用 `/memory`
/// 追加的规则跟在后面，两份都加载（红队 1：之前每次都写回只剩引用行，账号自己的规则丢了）。
/// 没有这一行时按 `copy_file` 的规则：上次写的原样就补成引用行，用户写过的先留存。
fn copy_import(target: &Path, line: &str, name: &str, copied: &mut BTreeMap<String, String>, report: &mut SyncReport) {
    if let Some(current) = read_file(target) {
        let text = String::from_utf8_lossy(&current);
        if text.lines().any(|l| l.trim() == line) {
            copied.insert(name.to_string(), digest(&current));
            return;
        }
    }
    copy_file(target, format!("{line}
").as_bytes(), name, copied, report);
}

// ═══ 默认改了才同步 ═══

#[derive(Debug, PartialEq, Eq)]
enum Action {
    /// 账号保持原样。
    Leave,
    /// 把账号的值换成默认的。
    Set,
    /// 从账号里删掉。
    Delete,
}

/// 见 `ShareSurface` 文档的同步规则（Orca `syncClaudeProfileSettings`）。参数都是摘要：`d` 默认当前值、
/// `r` 上次同步记下的默认值（`None` = 没记过）、`p` 账号当前值。返回动作，记录一律更新为 `d`。
fn decide(d: Option<&str>, r: Option<&str>, p: Option<&str>) -> Action {
    if r.is_some() && d == r {
        return Action::Leave;
    }
    match (d, p) {
        (Some(d), Some(p)) if d == p => Action::Leave,
        (Some(_), _) => Action::Set,
        (None, Some(_)) => Action::Delete,
        (None, None) => Action::Leave,
    }
}

fn update_record(record: &mut Record, surface: &str, item: &str, digest: Option<String>) {
    match digest {
        Some(d) => {
            record.entry(surface.to_string()).or_default().insert(item.to_string(), d);
        }
        // 面的记录清空了也保留这个面：它在不在决定了下次算不算「首次」。
        None => {
            if let Some(items) = record.get_mut(surface) {
                items.remove(item);
            }
        }
    }
}

/// FNV-1a 64 位。要跨 Meowo 版本稳定（标准库的哈希不保证），又不值得为它加依赖。
fn digest(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

// ═══ 文件与目录条目 ═══

fn read_file(path: &Path) -> Option<Vec<u8>> {
    if !path.is_file() {
        return None;
    }
    std::fs::read(path).ok()
}

fn sync_file(surface: &str, target: &Path, desired: Option<Vec<u8>>, record: &mut Record, report: &mut SyncReport) {
    let item = "";
    let d = desired.as_deref().map(digest);
    let r = record.get(surface).and_then(|m| m.get(item)).cloned();
    // 账号里同名的是目录或链接：那是用户自己的东西，不碰。
    if std::fs::symlink_metadata(target).is_ok_and(|m| !m.is_file()) {
        update_record(record, surface, item, d);
        return;
    }
    let current = read_file(target);
    let p = current.as_deref().map(digest);
    match decide(d.as_deref(), r.as_deref(), p.as_deref()) {
        Action::Leave => {}
        Action::Set => {
            let body = desired.as_deref().unwrap_or_default();
            match write_bytes_atomic(target, body) {
                Ok(()) => report.changed.push(format!("写入 {}", target.display())),
                Err(e) => {
                    report.errors.push(format!("{}: {e}", target.display()));
                    return; // 没写成就别记，下次再试
                }
            }
        }
        Action::Delete => match std::fs::remove_file(target) {
            Ok(()) => report.changed.push(format!("删除 {}", target.display())),
            Err(e) => {
                report.errors.push(format!("{}: {e}", target.display()));
                return;
            }
        },
    }
    update_record(record, surface, item, d);
}

/// 目录里逐条：子目录建链接，文件按规则复制；默认里删掉的条目，账号里同步来的那份也删。
fn sync_entries(dir: &str, skip: &[&str], dirs: &SyncDirs<'_>, record: &mut Record, report: &mut SyncReport) {
    let src = dirs.default_data.join(dir);
    let dst = dirs.profile_data.join(dir);
    let surface = format!("entries:{dir}");

    let mut files: Vec<String> = Vec::new();
    let mut subdirs: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&src) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || skip.contains(&name.as_str()) {
                continue;
            }
            // 跟随链接判断：用户的条目本身常是指向别处的链接。
            let path = entry.path();
            if path.is_dir() {
                subdirs.push(name);
            } else if path.is_file() {
                files.push(name);
            }
        }
    }
    files.sort();
    subdirs.sort();

    let mut names: Vec<String> = files.clone();
    for known in record.get(&surface).map(|m| m.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        if !names.contains(&known) {
            names.push(known);
        }
    }
    if (!files.is_empty() || !subdirs.is_empty()) && !dst.exists() {
        if let Err(e) = std::fs::create_dir_all(&dst) {
            report.errors.push(format!("{}: {e}", dst.display()));
            return;
        }
    }
    for name in &names {
        let desired = if files.contains(name) { read_file(&src.join(name)) } else { None };
        let item_surface = surface.clone();
        let target = dst.join(name);
        let d = desired.as_deref().map(digest);
        let r = record.get(&item_surface).and_then(|m| m.get(name)).cloned();
        if std::fs::symlink_metadata(&target).is_ok_and(|m| !m.is_file()) {
            update_record(record, &item_surface, name, d);
            continue;
        }
        let p = read_file(&target).as_deref().map(digest);
        match decide(d.as_deref(), r.as_deref(), p.as_deref()) {
            Action::Leave => {}
            Action::Set => {
                if let Err(e) = write_bytes_atomic(&target, desired.as_deref().unwrap_or_default()) {
                    report.errors.push(format!("{}: {e}", target.display()));
                    continue;
                }
                report.changed.push(format!("写入 {}", target.display()));
            }
            Action::Delete => {
                if let Err(e) = std::fs::remove_file(&target) {
                    report.errors.push(format!("{}: {e}", target.display()));
                    continue;
                }
                report.changed.push(format!("删除 {}", target.display()));
            }
        }
        update_record(record, &item_surface, name, d);
    }

    for name in &subdirs {
        let link = dst.join(name);
        if std::fs::symlink_metadata(&link).is_ok() {
            continue; // 已有链接，或账号自己的同名目录
        }
        let target = src.join(name);
        match super::share::link_dir(&link, &target) {
            Ok(()) => report.changed.push(format!("链接 {}", link.display())),
            Err(e) => report.errors.push(e),
        }
    }
    // 只拆自己建的、指向默认目录下这个目录、但已不再共享的链接。
    if let Err(e) = super::share::prune_links(&dst, &src, &subdirs) {
        report.errors.push(e);
    }
}

/// 引用里写的路径：在 home 下就写成 `~/...`（与 Claude 文档的写法一致，实测可用），否则写绝对路径。
/// 一律用 `/`：Claude 的 @ 引用把 `\` 当转义。
fn import_path(source: &Path, home: &Path) -> String {
    let rel = source.strip_prefix(home).ok().map(|rel| {
        let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        format!("~/{}", parts.join("/"))
    });
    rel.unwrap_or_else(|| source.to_string_lossy().replace('\\', "/"))
}

fn write_bytes_atomic(target: &Path, body: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file_name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = target.with_file_name(format!(".{file_name}.{}.meowo-tmp", std::process::id()));
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, target).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

// ═══ 结构化文件（JSON / TOML） ═══

fn sync_structured(file: &'static str, group: &[&ShareSurface], dirs: &SyncDirs<'_>, record: &mut Record, report: &mut SyncReport) {
    let format = group
        .iter()
        .find_map(|s| match **s {
            ShareSurface::Keys { format, .. } => Some(format),
            _ => None,
        })
        .unwrap_or(ShareFormat::Json);
    let source = dirs.default_data.join(file);
    let target = dirs.profile_data.join(file);

    let default_text = match std::fs::read_to_string(&source) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            report.errors.push(format!("{}: {e}", source.display()));
            return;
        }
    };
    // 默认文件读不懂：整份跳过。当成「默认是空的」会把账号里同步来的东西全删掉。
    let default_tree = match (&default_text, format) {
        (None, _) => Tree::Json(Value::Object(Map::new())),
        (Some(text), ShareFormat::Json) => match parse_json(text) {
            Some(v) => Tree::Json(v),
            None => {
                report.errors.push(format!("{}: 不是有效的 JSON，跳过", source.display()));
                return;
            }
        },
        (Some(text), ShareFormat::Toml) => match text.parse::<toml_edit::DocumentMut>() {
            Ok(doc) => Tree::Toml(doc),
            Err(e) => {
                report.errors.push(format!("{}: {e}", source.display()));
                return;
            }
        },
    };
    // 默认账号没有这个文件、账号里也没有：什么都不用做（也不建空文件）。
    if default_text.is_none() && !target.exists() && !group.iter().any(|s| has_record(record, s)) {
        return;
    }

    let profile_text = match std::fs::read_to_string(&target) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            report.errors.push(format!("{}: {e}", target.display()));
            return;
        }
    };
    let mut profile_tree = match (&profile_text, format) {
        (None, ShareFormat::Json) => Tree::Json(Value::Object(Map::new())),
        (None, ShareFormat::Toml) => Tree::Toml(toml_edit::DocumentMut::new()),
        (Some(text), ShareFormat::Json) => match parse_json(text) {
            Some(v) if v.is_object() => Tree::Json(v),
            _ => {
                report.errors.push(format!("{}: 不是有效的 JSON 对象，跳过（不覆盖）", target.display()));
                return;
            }
        },
        (Some(text), ShareFormat::Toml) => match text.parse::<toml_edit::DocumentMut>() {
            Ok(doc) => Tree::Toml(doc),
            Err(e) => {
                report.errors.push(format!("{}: {e}（不覆盖）", target.display()));
                return;
            }
        },
    };

    let first_time = !group.iter().any(|s| has_record(record, s));
    let mut changed: Vec<String> = Vec::new();
    for surface in group {
        if let ShareSurface::Keys { at, pick, exact_first, .. } = **surface {
            sync_keys(file, at, pick, exact_first, &default_tree, &mut profile_tree, record, &mut changed)
        }
    }
    if changed.is_empty() {
        return;
    }
    let body = match &profile_tree {
        Tree::Json(v) => match serde_json::to_string_pretty(v) {
            Ok(s) => s + "\n",
            Err(e) => {
                report.errors.push(format!("{}: {e}", target.display()));
                return;
            }
        },
        Tree::Toml(doc) => doc.to_string(),
    };
    if first_time {
        if let Some(text) = &profile_text {
            let backup = PathBuf::from(format!("{}{BACKUP_SUFFIX}", target.display()));
            if !backup.exists() {
                if let Err(e) = std::fs::write(&backup, text) {
                    report.errors.push(format!("{}: 备份失败，不改写：{e}", backup.display()));
                    return;
                }
            }
        }
    }
    match meowo_agent::fsutil::write_atomic(&target, &body) {
        Ok(()) => {
            report.rewritten.push(file);
            report
                .changed
                .extend(changed.into_iter().map(|c| format!("{} {c}", target.display())))
        }
        Err(e) => {
            report.errors.push(format!("{}: {e}", target.display()));
            // 写失败：撤回本轮对记录的改动，下次重来。
            for surface in group {
                if let Some(id) = surface_id(file, surface) {
                    match load_record_entry(dirs.record, &id) {
                        Some(items) => {
                            record.insert(id, items);
                        }
                        None => {
                            record.remove(&id);
                        }
                    }
                }
            }
        }
    }
}

fn has_record(record: &Record, surface: &ShareSurface) -> bool {
    let file = match *surface {
        ShareSurface::Keys { file, .. } => file,
        _ => return false,
    };
    surface_id(file, surface).is_some_and(|id| record.contains_key(&id))
}

fn surface_id(file: &str, surface: &ShareSurface) -> Option<String> {
    match *surface {
        ShareSurface::Keys { at, .. } => Some(format!("keys:{file}:{}", at.join("/"))),
        _ => None,
    }
}

enum Tree {
    Json(Value),
    Toml(toml_edit::DocumentMut),
}

fn parse_json(text: &str) -> Option<Value> {
    // 有的编辑器会存成带 BOM 的 UTF-8。
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// 按键排序后序列化，作为比较与摘要的规范形式（键序不同不算改动）。
fn canonical(v: &Value) -> String {
    fn sort(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                let mut out = Map::new();
                for k in keys {
                    out.insert(k.clone(), sort(&m[k]));
                }
                Value::Object(out)
            }
            Value::Array(a) => Value::Array(a.iter().map(sort).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string(&sort(v)).unwrap_or_default()
}

/// 把 TOML 的值转成 JSON 值，只为比较和摘要（写回仍用 toml_edit 原样搬条目）。
fn toml_item_json(item: &toml_edit::Item) -> Value {
    match item {
        toml_edit::Item::None => Value::Null,
        toml_edit::Item::Value(v) => toml_value_json(v),
        toml_edit::Item::Table(t) => {
            Value::Object(t.iter().map(|(k, v)| (k.to_string(), toml_item_json(v))).collect())
        }
        toml_edit::Item::ArrayOfTables(a) => Value::Array(
            a.iter()
                .map(|t| Value::Object(t.iter().map(|(k, v)| (k.to_string(), toml_item_json(v))).collect()))
                .collect(),
        ),
    }
}

fn toml_value_json(v: &toml_edit::Value) -> Value {
    match v {
        toml_edit::Value::String(s) => Value::String(s.value().clone()),
        toml_edit::Value::Integer(i) => Value::from(*i.value()),
        toml_edit::Value::Float(f) => serde_json::Number::from_f64(*f.value()).map(Value::Number).unwrap_or(Value::Null),
        toml_edit::Value::Boolean(b) => Value::Bool(*b.value()),
        toml_edit::Value::Datetime(d) => Value::String(d.value().to_string()),
        toml_edit::Value::Array(a) => Value::Array(a.iter().map(toml_value_json).collect()),
        toml_edit::Value::InlineTable(t) => {
            Value::Object(t.iter().map(|(k, v)| (k.to_string(), toml_value_json(v))).collect())
        }
    }
}

/// 一层对象上按键同步。条目名 = 具体路径（展开通配后）+ 键，用 [`SEP`] 连接。
#[allow(clippy::too_many_arguments)]
fn sync_keys(
    file: &str,
    at: &[&str],
    pick: SharePick,
    exact_first: bool,
    default_tree: &Tree,
    profile_tree: &mut Tree,
    record: &mut Record,
    changed: &mut Vec<String>,
) {
    let surface = format!("keys:{file}:{}", at.join("/"));
    // 要看的条目：默认里现有的，加上记录里有的（默认删掉的也得看一眼，才能把删除传过去）。
    let mut items: Vec<Vec<String>> = Vec::new();
    for (path, keys) in objects_at(default_tree, at) {
        for key in keys {
            if pick.includes(&key) {
                let mut item = path.clone();
                item.push(key);
                items.push(item);
            }
        }
    }
    for known in record.get(&surface).map(|m| m.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        let item: Vec<String> = known.split(SEP).map(str::to_string).collect();
        if item.len() == at.len() + 1 && !items.contains(&item) {
            items.push(item);
        }
    }
    // 首次：账号这一层独有的键也要看（默认里没有 → 删掉），账号与默认一致。
    let first = !record.contains_key(&surface);
    if first && exact_first {
        for (path, keys) in objects_at(profile_tree, at) {
            for key in keys {
                let mut item = path.clone();
                item.push(key);
                if pick.includes(item.last().map(String::as_str).unwrap_or_default()) && !items.contains(&item) {
                    items.push(item);
                }
            }
        }
    }
    // 一个键都没有时也把这个面记下，下次就不再算首次。
    record.entry(surface.clone()).or_default();

    for item in items {
        let name = item.join(&SEP.to_string());
        let d_value = value_at(default_tree, &item);
        let d = d_value.as_ref().map(|v| digest(canonical(v).as_bytes()));
        let r = record.get(&surface).and_then(|m| m.get(&name)).cloned();
        let p = value_at(profile_tree, &item).map(|v| digest(canonical(&v).as_bytes()));
        let action = decide(d.as_deref(), r.as_deref(), p.as_deref());
        let applied = match action {
            Action::Leave => true,
            Action::Set => set_from_default(default_tree, profile_tree, &item),
            Action::Delete => delete_at(profile_tree, &item),
        };
        if !applied {
            continue; // 账号里这一层不是对象之类：不硬改，也不记
        }
        match action {
            Action::Set => changed.push(format!("同步 {}", item.join("."))),
            Action::Delete => changed.push(format!("移除 {}", item.join("."))),
            Action::Leave => {}
        }
        update_record(record, &surface, &name, d);
    }
}

/// 默认树里匹配 `at` 的每个对象：(具体路径, 它的键)。
fn objects_at(tree: &Tree, at: &[&str]) -> Vec<(Vec<String>, Vec<String>)> {
    match tree {
        Tree::Json(root) => {
            let mut out = Vec::new();
            fn walk(v: &Value, at: &[&str], path: &mut Vec<String>, out: &mut Vec<(Vec<String>, Vec<String>)>) {
                let Value::Object(m) = v else { return };
                match at.split_first() {
                    None => out.push((path.clone(), m.keys().cloned().collect())),
                    Some((&"*", rest)) => {
                        for (k, child) in m {
                            path.push(k.clone());
                            walk(child, rest, path, out);
                            path.pop();
                        }
                    }
                    Some((seg, rest)) => {
                        if let Some(child) = m.get(*seg) {
                            path.push((*seg).to_string());
                            walk(child, rest, path, out);
                            path.pop();
                        }
                    }
                }
            }
            walk(root, at, &mut Vec::new(), &mut out);
            out
        }
        Tree::Toml(doc) => {
            // TOML 面不支持通配（声明处有测试钉着）。
            let mut table: &dyn toml_edit::TableLike = doc.as_table();
            for seg in at {
                match table.get(seg).and_then(|i| i.as_table_like()) {
                    Some(t) => table = t,
                    None => return Vec::new(),
                }
            }
            let keys = table.iter().map(|(k, _)| k.to_string()).collect();
            vec![(at.iter().map(|s| (*s).to_string()).collect(), keys)]
        }
    }
}

fn value_at(tree: &Tree, item: &[String]) -> Option<Value> {
    match tree {
        Tree::Json(root) => {
            let mut v = root;
            for seg in item {
                v = v.as_object()?.get(seg)?;
            }
            Some(v.clone())
        }
        Tree::Toml(doc) => {
            let (key, parents) = item.split_last()?;
            let mut table: &dyn toml_edit::TableLike = doc.as_table();
            for seg in parents {
                table = table.get(seg)?.as_table_like()?;
            }
            Some(toml_item_json(table.get(key)?))
        }
    }
}

fn set_from_default(default_tree: &Tree, profile_tree: &mut Tree, item: &[String]) -> bool {
    let Some((key, parents)) = item.split_last() else { return false };
    match (default_tree, profile_tree) {
        (Tree::Json(_), Tree::Json(root)) => {
            let Some(value) = value_at(default_tree, item) else { return false };
            let mut obj = match root.as_object_mut() {
                Some(o) => o,
                None => return false,
            };
            for seg in parents {
                let child = obj.entry(seg.clone()).or_insert_with(|| Value::Object(Map::new()));
                obj = match child.as_object_mut() {
                    Some(o) => o,
                    None => return false,
                };
            }
            obj.insert(key.clone(), value);
            true
        }
        (Tree::Toml(src), Tree::Toml(doc)) => {
            let mut from: &dyn toml_edit::TableLike = src.as_table();
            for seg in parents {
                match from.get(seg).and_then(|i| i.as_table_like()) {
                    Some(t) => from = t,
                    None => return false,
                }
            }
            let Some(value) = from.get(key).cloned() else { return false };
            let mut table: &mut dyn toml_edit::TableLike = doc.as_table_mut();
            for seg in parents {
                if table.get(seg).is_none() {
                    let mut t = toml_edit::Table::new();
                    t.set_implicit(true);
                    table.insert(seg, toml_edit::Item::Table(t));
                }
                table = match table.get_mut(seg).and_then(|i| i.as_table_like_mut()) {
                    Some(t) => t,
                    None => return false,
                };
            }
            table.insert(key, value);
            true
        }
        _ => false,
    }
}

fn delete_at(profile_tree: &mut Tree, item: &[String]) -> bool {
    let Some((key, parents)) = item.split_last() else { return false };
    match profile_tree {
        Tree::Json(root) => {
            let mut v = root;
            for seg in parents {
                v = match v.as_object_mut().and_then(|o| o.get_mut(seg)) {
                    Some(c) => c,
                    None => return true, // 已经没有了
                };
            }
            if let Some(o) = v.as_object_mut() {
                o.remove(key);
            }
            true
        }
        Tree::Toml(doc) => {
            let mut table: &mut dyn toml_edit::TableLike = doc.as_table_mut();
            for seg in parents {
                table = match table.get_mut(seg).and_then(|i| i.as_table_like_mut()) {
                    Some(t) => t,
                    None => return true,
                };
            }
            table.remove(key);
            true
        }
    }
}

// ═══ Claude 的 .claude.json（Orca claude-profile-state.ts） ═══

/// 默认 `.claude.json` 里要叠到账号上的部分（Orca `selectSharedClaudeProfileState`）。
fn shared_claude_state(source: &Map<String, Value>, keys: &[&str], project_keys: &[&str]) -> Map<String, Value> {
    let mut shared: Map<String, Value> =
        keys.iter().filter_map(|k| source.get(*k).map(|v| ((*k).to_string(), v.clone()))).collect();
    if let Some(Value::Object(projects)) = source.get("projects") {
        let mut out = Map::new();
        for (path, entry) in projects {
            let Value::Object(entry) = entry else { continue };
            let picked: Map<String, Value> = project_keys
                .iter()
                .filter_map(|k| entry.get(*k).map(|v| ((*k).to_string(), v.clone())))
                .collect();
            if !picked.is_empty() {
                out.insert(path.clone(), Value::Object(picked));
            }
        }
        if !out.is_empty() {
            shared.insert("projects".to_string(), Value::Object(out));
        }
    }
    if source.get("hasCompletedOnboarding") == Some(&Value::Bool(true)) {
        shared.insert("hasCompletedOnboarding".to_string(), Value::Bool(true));
    }
    if let Some(rejected) = source
        .get("customApiKeyResponses")
        .and_then(|v| v.get("rejected"))
        .and_then(Value::as_array)
    {
        let rejected: Vec<Value> = rejected.iter().filter(|v| v.is_string()).cloned().collect();
        if !rejected.is_empty() {
            let mut m = Map::new();
            m.insert("rejected".to_string(), Value::Array(rejected));
            shared.insert("customApiKeyResponses".to_string(), Value::Object(m));
        }
    }
    shared
}

/// Orca `mergeClaudeProfileState`：把 `shared` 叠到账号的 `.claude.json` 上，账号自己的键都留着。
fn merge_claude_state(current: &Map<String, Value>, shared: &Map<String, Value>) -> Map<String, Value> {
    let mut config = current.clone();
    for (key, value) in shared {
        match (key.as_str(), value) {
            ("hasCompletedOnboarding", _) => {
                config.entry(key.clone()).or_insert(Value::Bool(true));
            }
            ("customApiKeyResponses", Value::Object(source)) => {
                let merged = match current.get(key) {
                    None => Some(Map::new()),
                    Some(Value::Object(own)) => Some(own.clone()),
                    Some(_) => None,
                };
                let (Some(mut merged), Some(Value::Array(incoming))) = (merged, source.get("rejected")) else {
                    continue;
                };
                let mut rejected = match merged.get("rejected") {
                    None => Vec::new(),
                    Some(Value::Array(a)) => a.clone(),
                    Some(_) => continue,
                };
                for v in incoming {
                    if !rejected.contains(v) {
                        rejected.push(v.clone());
                    }
                }
                merged.insert("rejected".to_string(), Value::Array(rejected));
                config.insert(key.clone(), Value::Object(merged));
            }
            ("projects", Value::Object(source)) => {
                let mut projects = match current.get(key) {
                    None => Map::new(),
                    Some(Value::Object(own)) => own.clone(),
                    Some(_) => continue,
                };
                for (path, entry) in source {
                    let Value::Object(entry) = entry else { continue };
                    let mut own = match projects.get(path) {
                        None => Map::new(),
                        Some(Value::Object(own)) => own.clone(),
                        Some(_) => continue,
                    };
                    let trusted = own.get("hasTrustDialogAccepted") == Some(&Value::Bool(true));
                    for (k, v) in entry {
                        own.insert(k.clone(), v.clone());
                    }
                    if trusted {
                        own.insert("hasTrustDialogAccepted".to_string(), Value::Bool(true));
                    }
                    projects.insert(path.clone(), Value::Object(own));
                }
                config.insert(key.clone(), Value::Object(projects));
            }
            ("mcpServers", Value::Object(source)) if current.get(key).is_some_and(Value::is_object) => {
                let mut servers = current.get(key).and_then(Value::as_object).cloned().unwrap_or_default();
                for (name, server) in source {
                    servers.insert(name.clone(), server.clone());
                }
                config.insert(key.clone(), Value::Object(servers));
            }
            _ => {
                config.insert(key.clone(), value.clone());
            }
        }
    }
    config
}

fn sync_claude_state(
    source: &Path,
    target: &Path,
    file: &'static str,
    keys: &[&str],
    project_keys: &[&str],
    report: &mut SyncReport,
) {
    // 账号里的是链接：用户自己的安排，不碰。
    if std::fs::symlink_metadata(target).is_ok_and(|m| m.file_type().is_symlink()) {
        return;
    }
    let default_root = match std::fs::read_to_string(source) {
        Ok(text) => match parse_json(&text) {
            Some(Value::Object(m)) => m,
            _ => {
                report.errors.push(format!("{}: 不是有效的 JSON 对象，跳过", source.display()));
                return;
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            report.errors.push(format!("{}: {e}", source.display()));
            return;
        }
    };
    let shared = shared_claude_state(&default_root, keys, project_keys);
    let Some(_lock) = ConfigLock::acquire(target) else {
        report.errors.push(format!("{}: 等不到 Claude 的配置锁，下次再同步", target.display()));
        return;
    };
    let (current_text, current) = match std::fs::read_to_string(target) {
        Ok(text) => match parse_json(&text) {
            Some(Value::Object(m)) => (Some(text), m),
            _ => {
                report.errors.push(format!("{}: 不是有效的 JSON 对象，跳过（不覆盖）", target.display()));
                return;
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, Map::new()),
        Err(e) => {
            report.errors.push(format!("{}: {e}", target.display()));
            return;
        }
    };
    let merged = merge_claude_state(&current, &shared);
    if current_text.is_some() && merged == current {
        return;
    }
    let body = match serde_json::to_string_pretty(&Value::Object(merged)) {
        Ok(s) => s + "\n",
        Err(e) => {
            report.errors.push(format!("{}: {e}", target.display()));
            return;
        }
    };
    if let Some(text) = &current_text {
        let backup = PathBuf::from(format!("{}{BACKUP_SUFFIX}", target.display()));
        if !backup.exists() {
            if let Err(e) = std::fs::write(&backup, text) {
                report.errors.push(format!("{}: 备份失败，不改写：{e}", backup.display()));
                return;
            }
        }
    }
    match meowo_agent::fsutil::write_atomic(target, &body) {
        Ok(()) => {
            report.rewritten.push(file);
            report.changed.push(format!("合并 {}", target.display()));
        }
        Err(e) => report.errors.push(format!("{}: {e}", target.display())),
    }
}

// ═══ agent 的配置锁 ═══

/// Claude 用 proper-lockfile 锁 `.claude.json`：锁就是 `<文件>.lock` 这个**目录**，拿锁 = mkdir，
/// 超过 10 秒没刷新 mtime 视为陈旧（proper-lockfile 默认 stale）。我们按同一约定拿锁，运行中的
/// Claude 就不会在我们读改写的间隙写这份文件（2.1.332 二进制：`lockfilePath:`${y}.lock``）。
struct ConfigLock(PathBuf);

impl ConfigLock {
    fn acquire(file: &Path) -> Option<ConfigLock> {
        let lock = PathBuf::from(format!("{}.lock", file.display()));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            match std::fs::create_dir(&lock) {
                Ok(()) => return Some(ConfigLock(lock)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = std::fs::metadata(&lock)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > std::time::Duration::from_secs(10));
                    if stale {
                        let _ = std::fs::remove_dir(&lock);
                        continue;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // 账号目录还没有：没人会和我们抢。
                    if let Some(parent) = file.parent() {
                        if std::fs::create_dir_all(parent).is_err() {
                            return None;
                        }
                        continue;
                    }
                    return None;
                }
                Err(_) => return None,
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

impl Drop for ConfigLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

// ═══ 记录 ═══

fn load_record(path: &Path) -> Record {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn load_record_entry(path: &Path, id: &str) -> Option<BTreeMap<String, String>> {
    load_record(path).remove(id)
}

fn save_record(path: &Path, record: &Record) -> std::io::Result<()> {
    let body = serde_json::to_string_pretty(record).map_err(std::io::Error::other)?;
    meowo_agent::fsutil::write_atomic(path, &(body + "\n"))
}

#[cfg(test)]
#[path = "share_sync_tests.rs"]
mod tests;
