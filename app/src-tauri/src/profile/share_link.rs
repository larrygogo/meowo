//! 账号目录里的条目换成指向默认目录的链接：照 Orca（stablyai/orca c1bd89d）的
//! `merged-directory-link.ts`、`claude-profile-sharing.ts`、`claude-profile-prompt-history.ts` 移植。
//!
//! - [`link_entry_dir`]：账号里没有、或是空目录时，建一个指向默认目录的链接；
//! - [`merge_dir_into_link`]：账号里已有真实目录时，先改名成 `<目录>.meowo-profile-merge`，建好链接，
//!   再把里面的条目逐个并进默认目录。文件用「硬链接到新位置 + 删旧名」移动，不复制（1.5 GB 的
//!   会话历史也只是改名）；同名冲突一律不覆盖，留在待合并目录里，下次再试；
//! - [`merge_prompt_history`]：输入历史 `history.jsonl` 去重并进默认文件，账号里换成指向它的硬链接。
//!
//! 跨盘（硬链接与改名都做不了）时整个条目保持私有，报原因，不复制。

use std::collections::HashSet;
use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};

/// 条目等着并进共享目录时待的地方；冲突的名字留在这里（Orca 叫 `.orca-profile-merge`）。
pub(crate) const MERGE_PENDING_SUFFIX: &str = ".meowo-profile-merge";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkOutcome {
    Linked,
    Unchanged,
    /// 账号里同名的是用户自己的东西（非空目录、文件、指向别处的链接），不碰。
    UserOwned,
    /// 默认目录里没有它。
    Absent,
}

fn lstat(path: &Path) -> io::Result<Option<Metadata>> {
    match std::fs::symlink_metadata(path) {
        Ok(m) => Ok(Some(m)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// junction 在 std 里也算 symlink（名称代理型重解析点），这里统一叫「链接」。
fn is_link(meta: &Metadata) -> bool {
    meta.file_type().is_symlink()
}

fn same_place(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn err(path: &Path, e: impl std::fmt::Display) -> String {
    format!("{}: {e}", path.display())
}

/// Windows 上刚被别的进程（杀毒、索引、正在启动的 CLI）碰过的目录，改名会短暂「拒绝访问」。
fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Err(e) if cfg!(windows) && e.kind() == io::ErrorKind::PermissionDenied && attempt < 5 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            other => return other,
        }
    }
}

// ═══ 文件身份（卷 + 文件号） ═══

/// `(卷, 文件号)`：判断两个名字是不是同一个文件（硬链接），以及两处在不在同一个盘上。
/// 跟随链接。
#[cfg(windows)]
pub(crate) fn identity(path: &Path) -> io::Result<(u64, u64)> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
    };
    // 访问权限 0：只读元数据，不和正在写它的 Claude 抢；BACKUP_SEMANTICS 才能打开目录。
    let file = std::fs::OpenOptions::new()
        .access_mode(0)
        .share_mode(7)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    // SAFETY: 句柄在 file 存活期间有效；info 是纯数据结构，全零是合法初值。
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    Ok((u64::from(info.dwVolumeSerialNumber), index))
}

#[cfg(unix)]
pub(crate) fn identity(path: &Path) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path)?;
    Ok((m.dev(), m.ino()))
}

fn same_volume(a: &Path, b: &Path) -> io::Result<bool> {
    Ok(identity(a)?.0 == identity(b)?.0)
}

fn cross_volume(path: &Path) -> String {
    format!("{}: 与默认目录不在同一个盘上，保持各账号私有", path.display())
}

// ═══ 目录链接 ═══

/// Orca `linkClaudeProfileDirectory`：账号里 `target` 没有、是空目录、或是死链接时，建一个指向
/// 默认目录 `source` 的链接。链接的是 `source` 这个路径本身（不是它解析后的位置），用户以后改
/// 自己的链接也会跟着走。
pub(crate) fn link_entry_dir(source: &Path, target: &Path) -> Result<LinkOutcome, String> {
    if !source.is_dir() {
        return Ok(LinkOutcome::Absent);
    }
    if let Some(meta) = lstat(target).map_err(|e| err(target, e))? {
        if is_link(&meta) {
            if same_place(target, source) {
                return Ok(LinkOutcome::Unchanged);
            }
            if std::fs::canonicalize(target).is_ok() {
                return Ok(LinkOutcome::UserOwned);
            }
            // 死链接：拆掉重建。
            super::share::remove_dir_link(target).map_err(|e| err(target, e))?;
        } else {
            if !meta.is_dir() || std::fs::read_dir(target).map_err(|e| err(target, e))?.next().is_some() {
                return Ok(LinkOutcome::UserOwned);
            }
            std::fs::remove_dir(target).map_err(|e| err(target, e))?;
        }
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| err(parent, e))?;
    }
    super::share::link_dir(target, source)?;
    Ok(LinkOutcome::Linked)
}

#[derive(Default)]
struct MoveResult {
    retained: usize,
    failed: Vec<String>,
}

/// 把一个条目移到 `to`（`to` 不存在时才调）。目录整个改名；文件先给它在新位置起一个硬链接名、
/// 再删旧名——直接 rename 在 Windows 上会顶掉恰好同时出现的同名文件，复制则会让正在追加写的
/// 句柄写进旧副本（Orca 同理由）。硬链接做不了（跨盘、文件系统不支持）就留在原地。
fn move_entry(from: &Path, to: &Path, meta: &Metadata) -> io::Result<bool> {
    if meta.is_dir() || is_link(meta) {
        rename_with_retry(from, to)?;
        return Ok(true);
    }
    match std::fs::hard_link(from, to) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(e),
        Err(_) => return Ok(false),
    }
    std::fs::remove_file(from)?;
    Ok(true)
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(ma), Ok(mb)) if ma.len() == mb.len() => {}
        _ => return false,
    }
    matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

/// 递归把 `source` 里的条目并进 `destination`。某个条目被锁住不影响其余条目。
///
/// 冲突处理照 Orca，另加 Meowo 的三条（都是零丢失的）：
/// - 同一个文件（硬链接）→ 丢掉这个名字；
/// - 内容逐字节相同 → 丢掉这个名字（Meowo 以前给每个账号单独装过 Lead/团队文件，与默认的一样）；
/// - 指向默认目录同名条目的链接 → 拆掉链接（Meowo 以前按 skill 逐个建的 junction）；
/// - `jsonl_merge` 时同名 `*.jsonl`（transcript）→ 走「并回默认」的无损规则：一份是另一份的前缀就
///   留长的，分歧时账号那份另存成 `.merge-conflict.bak`；
/// - 其余冲突留在原地，计数。
fn move_tree(source: &Path, destination: &Path, jsonl_merge: bool, result: &mut MoveResult) {
    let mut names: Vec<_> = match std::fs::read_dir(source) {
        Ok(entries) => entries.flatten().map(|e| e.file_name()).collect(),
        Err(e) => {
            result.failed.push(err(source, e));
            return;
        }
    };
    names.sort();
    for name in names {
        let from = source.join(&name);
        let to = destination.join(&name);
        let mut step = || -> Result<(), String> {
            let item = lstat(&from).map_err(|e| err(&from, e))?.ok_or_else(|| err(&from, "已不存在"))?;
            let existing = lstat(&to).map_err(|e| err(&to, e))?;
            let Some(existing) = existing else {
                match move_entry(&from, &to, &item) {
                    Ok(true) => {}
                    Ok(false) => result.retained += 1,
                    // 另一个合并先建出了这个目录：并进去。
                    Err(_) if item.is_dir() && to.is_dir() => move_tree(&from, &to, jsonl_merge, result),
                    Err(e) => return Err(err(&from, e)),
                }
                return Ok(());
            };
            if is_link(&item) {
                if same_place(&from, &to) {
                    super::share::remove_dir_link(&from).map_err(|e| err(&from, e))?;
                } else {
                    result.retained += 1;
                }
            } else if item.is_dir() && existing.is_dir() {
                move_tree(&from, &to, jsonl_merge, result);
            } else if item.is_file() && existing.is_file() {
                let same_file = matches!((identity(&from), identity(&to)), (Ok(a), Ok(b)) if a == b);
                if same_file || same_bytes(&from, &to) {
                    std::fs::remove_file(&from).map_err(|e| err(&from, e))?;
                } else if jsonl_merge && name.to_string_lossy().ends_with(".jsonl") {
                    super::merge_jsonl_no_loss(&from, &to)?;
                    std::fs::remove_file(&from).map_err(|e| err(&from, e))?;
                } else {
                    result.retained += 1;
                }
            } else {
                result.retained += 1;
            }
            Ok(())
        };
        if let Err(e) = step() {
            result.failed.push(e);
        }
    }
    if std::fs::read_dir(source).is_ok_and(|mut it| it.next().is_none()) {
        let _ = std::fs::remove_dir(source);
    }
}

fn drain(pending: &Path, destination: &Path, jsonl_merge: bool, warnings: &mut Vec<String>) {
    let mut result = MoveResult::default();
    move_tree(pending, destination, jsonl_merge, &mut result);
    if let Some(first) = result.failed.into_iter().next() {
        warnings.push(first);
    }
    if result.retained > 0 {
        warnings.push(format!("{} 个同名冲突留在 {}，未覆盖", result.retained, pending.display()));
    }
}

/// Orca `mergeDirectoryIntoLink`：把账号里的 `source` 换成指向 `destination`（默认目录）的链接，
/// 原有条目移进 `destination`。可重入：中途失败时条目留在待合并目录，下次接着并。
pub(crate) fn merge_dir_into_link(
    source: &Path,
    destination: &Path,
    jsonl_merge: bool,
    warnings: &mut Vec<String>,
) -> Result<LinkOutcome, String> {
    let pending = PathBuf::from(format!("{}{MERGE_PENDING_SUFFIX}", source.display()));
    std::fs::create_dir_all(destination).map_err(|e| err(destination, e))?;

    if lstat(&pending).map_err(|e| err(&pending, e))?.is_some_and(|m| m.is_dir()) {
        if !same_volume(&pending, destination).map_err(|e| err(&pending, e))? {
            if lstat(source).map_err(|e| err(source, e))?.is_none() {
                rename_with_retry(&pending, source).map_err(|e| err(&pending, e))?;
            }
            return Err(cross_volume(source));
        }
        drain(&pending, destination, jsonl_merge, warnings);
    }

    let current = lstat(source).map_err(|e| err(source, e))?;
    if let Some(meta) = &current {
        if is_link(meta) {
            return Ok(if same_place(source, destination) {
                LinkOutcome::Unchanged
            } else {
                LinkOutcome::UserOwned
            });
        }
        if !meta.is_dir() {
            return Ok(LinkOutcome::UserOwned);
        }
        if !same_volume(source, destination).map_err(|e| err(source, e))? {
            return Err(cross_volume(source));
        }
        if lstat(&pending).map_err(|e| err(&pending, e))?.is_some() {
            // 上次的冲突还在待合并目录里：这次的直接并进去，剩下的放到旧冲突旁边。
            drain(source, destination, jsonl_merge, warnings);
            if source.exists() {
                move_tree(source, &pending, false, &mut MoveResult::default());
            }
            if source.exists() {
                // 两处都冲突的名字：这一份单独放一个带时间戳的待合并目录。
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or_default();
                let aside = PathBuf::from(format!("{}.{stamp}{MERGE_PENDING_SUFFIX}", source.display()));
                rename_with_retry(source, &aside).map_err(|e| err(source, e))?;
            }
        } else {
            rename_with_retry(source, &pending).map_err(|e| err(source, e))?;
        }
    }

    if let Err(e) = super::share::link_dir(source, destination) {
        if current.is_some() {
            // 改名到建链接之间有东西（比如正在启动的 CLI）重新建出了目录：把条目放回去。
            if !source.exists() {
                let _ = rename_with_retry(&pending, source);
            } else if source.is_dir() {
                move_tree(&pending, source, false, &mut MoveResult::default());
            }
        }
        return Err(e);
    }
    if current.is_some() {
        drain(&pending, destination, jsonl_merge, warnings);
    }
    Ok(LinkOutcome::Linked)
}

// ═══ 输入历史 ═══

/// 记下账号里硬链接指向的那个共享文件的身份，用来认出「共享文件被 CLI 整个重写换掉了」。
fn link_record_name(file: &str) -> String {
    format!("{file}.meowo-profile-link")
}

fn pending_name(file: &str) -> String {
    format!("{file}{MERGE_PENDING_SUFFIX}")
}

fn identity_text(id: (u64, u64)) -> String {
    format!("{}:{}", id.0, id.1)
}

fn next_free_path(base: &Path) -> PathBuf {
    let mut generation = 0;
    loop {
        let candidate = if generation == 0 {
            base.to_path_buf()
        } else {
            PathBuf::from(format!("{}-{generation}", base.display()))
        };
        if !candidate.exists() {
            return candidate;
        }
        generation += 1;
    }
}

/// 追加若干行，保证每条记录都以换行结束（Claude 下一次追加从新行开始）。
fn append_lines(destination: &Path, lines: &[String]) -> io::Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    if lines.is_empty() {
        return Ok(());
    }
    let mut file = std::fs::OpenOptions::new().read(true).append(true).open(destination)?;
    let size = file.metadata()?.len();
    let mut separator = false;
    if size > 0 {
        let mut tail = [0u8; 1];
        file.seek(SeekFrom::Start(size - 1))?;
        file.read_exact(&mut tail)?;
        separator = tail[0] != b'\n';
    }
    let mut body = String::new();
    if separator {
        body.push('\n');
    }
    for line in lines {
        body.push_str(line);
        body.push('\n');
    }
    file.write_all(body.as_bytes())
}

/// Orca `drainHistory`：把改名出来的旧文件里共享文件还没有的行追加过去。旧文件留到下一次确认
/// 没有新追加（改名前打开的句柄还可能写它）才删；游标记在 `<文件>.offset`，中断后重放靠去重保证安全。
fn drain_history(pending: &Path, destination: &Path, shared: &mut HashSet<String>) -> io::Result<()> {
    let content = std::fs::read(pending)?;
    let cursor = PathBuf::from(format!("{}.offset", pending.display()));
    let stored = std::fs::read_to_string(&cursor)
        .ok()
        .and_then(|t| t.trim().parse::<usize>().ok())
        .filter(|&n| n <= content.len());
    if stored == Some(content.len()) {
        // 先删游标：清理被打断时，重放的是去过重的行，安全。
        std::fs::remove_file(&cursor)?;
        std::fs::remove_file(pending)?;
        return Ok(());
    }
    let offset = stored.unwrap_or(0);
    let text = String::from_utf8_lossy(&content[offset..]);
    let lines: Vec<String> = text
        .split('\n')
        .filter(|l| !l.is_empty() && !shared.contains(*l))
        .map(str::to_string)
        .collect();
    append_lines(destination, &lines)?;
    shared.extend(lines);
    std::fs::write(&cursor, format!("{}\n", content.len()))
}

/// Orca `mergeClaudeProfilePromptHistory`：账号的 `file`（↑ 翻到的输入历史）并进默认目录的同名
/// 文件，账号里换成指向它的硬链接（Windows 建文件符号链接要特权，硬链接不用；同盘才行）。
///
/// CLI 改写文件（写临时文件再改名）会让硬链接断开：
/// - 断开的是账号这边（账号文件身份 ≠ 记录）：它的新行去重并回共享文件，再重新链上；
/// - 换掉的是共享文件（账号文件身份 = 记录，但共享文件身份变了）：账号这份是旧内容，并回去会让
///   用户在共享文件里删掉的条目复活，所以只改名留存、不并，报一条提醒。
pub(crate) fn merge_prompt_history(
    profile: &Path,
    home: &Path,
    file: &str,
    warnings: &mut Vec<String>,
) -> Result<LinkOutcome, String> {
    let source = profile.join(file);
    let destination = home.join(file);
    if !destination.exists() {
        std::fs::create_dir_all(home).map_err(|e| err(home, e))?;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .or_else(|e| if e.kind() == io::ErrorKind::AlreadyExists { Ok(std::fs::File::open(&destination)?) } else { Err(e) })
            .map_err(|e| err(&destination, e))?;
    }
    let pending_base = pending_name(file);
    let mut pendings: Vec<(u64, PathBuf)> = std::fs::read_dir(profile)
        .map_err(|e| err(profile, e))?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let generation = if name == pending_base {
                Some(0)
            } else {
                name.strip_prefix(&format!("{pending_base}-"))
                    .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                    .and_then(|s| s.parse().ok())
            };
            generation.map(|g| (g, e.path()))
        })
        .filter(|(_, path)| path.is_file())
        .collect();
    // 目录枚举顺序不保证时间顺序。
    pendings.sort();

    // 快路径（每次拉起会话前都会走到这里）：已经链好、没有待并的旧文件时不读共享文件（实测
    // 1.3 MB，整份读进来去重要十几毫秒）。
    if pendings.is_empty() && std::fs::symlink_metadata(&source).is_ok_and(|m| m.is_file()) {
        if let (Ok(own), Ok(shared_id)) = (identity(&source), identity(&destination)) {
            let record = profile.join(link_record_name(file));
            let text = identity_text(shared_id);
            if own == shared_id && std::fs::read_to_string(&record).is_ok_and(|t| t.trim() == text) {
                return Ok(LinkOutcome::Unchanged);
            }
        }
    }
    let mut shared: HashSet<String> = std::fs::read_to_string(&destination)
        .map_err(|e| err(&destination, e))?
        .split('\n')
        .map(str::to_string)
        .collect();
    if !pendings.is_empty() && !same_volume(profile, &destination).map_err(|e| err(profile, e))? {
        return Err(cross_volume(&source));
    }
    for (_, pending) in &pendings {
        if let Err(e) = drain_history(pending, &destination, &mut shared) {
            warnings.push(err(pending, e));
        }
    }

    let current = lstat(&source).map_err(|e| err(&source, e))?;
    if let Some(meta) = &current {
        if is_link(meta) {
            return Ok(if same_place(&source, &destination) {
                LinkOutcome::Unchanged
            } else {
                LinkOutcome::UserOwned
            });
        }
        if !meta.is_file() {
            return Ok(LinkOutcome::UserOwned);
        }
    }

    let record = profile.join(link_record_name(file));
    let shared_id = identity_text(identity(&destination).map_err(|e| err(&destination, e))?);
    // 记录读不了时不能当成没有：当成没有会把用户删掉的历史并回来。
    let recorded = match std::fs::read_to_string(&record) {
        Ok(t) => Some(t.trim().to_string()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(err(&record, e)),
    };
    let own_id = match &current {
        Some(_) => Some(identity_text(identity(&source).map_err(|e| err(&source, e))?)),
        None => None,
    };
    if own_id.as_deref() == Some(shared_id.as_str()) {
        if recorded.as_deref() != Some(shared_id.as_str()) {
            std::fs::write(&record, format!("{shared_id}\n")).map_err(|e| err(&record, e))?;
        }
        return Ok(LinkOutcome::Unchanged);
    }
    if !same_volume(profile, &destination).map_err(|e| err(profile, e))? {
        return Err(cross_volume(&source));
    }
    let replaced = own_id.is_some() && own_id == recorded;

    let aside = match current {
        Some(_) => {
            let base = if replaced {
                profile.join(format!("{file}.meowo-profile-conflict"))
            } else {
                profile.join(&pending_base)
            };
            let aside = next_free_path(&base);
            rename_with_retry(&source, &aside).map_err(|e| err(&source, e))?;
            Some(aside)
        }
        None => None,
    };
    let linked = std::fs::hard_link(&destination, &source).and_then(|()| {
        std::fs::write(&record, format!("{shared_id}\n")).inspect_err(|_| {
            // 没有记录的话，以后共享文件被换掉时会把删掉的历史并回来：宁可不链。
            let _ = std::fs::remove_file(&source);
        })
    });
    if let Err(e) = linked {
        if let Some(aside) = &aside {
            if !source.exists() {
                let _ = rename_with_retry(aside, &source);
            }
        }
        return Err(err(&source, e));
    }
    if let Some(aside) = aside {
        if replaced {
            warnings.push(format!("共享的输入历史被整个换掉过，账号里的旧副本留在 {}", aside.display()));
        } else if let Err(e) = drain_history(&aside, &destination, &mut shared) {
            warnings.push(err(&aside, e));
        }
    }
    Ok(LinkOutcome::Linked)
}
