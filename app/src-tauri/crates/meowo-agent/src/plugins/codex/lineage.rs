//! codex 分页历史（`history_mode: paginated`）的续写段拼接。
//!
//! 为什么要拼：codex 0.163+ 回溯（TUI 双击 Esc 编辑上一条）不再往原 rollout 追加
//! `thread_rolled_back`，而是新建续写段 `rollout-<时间>-<线程id>_<段id>.jsonl`，其首行
//! session_meta 带 `history_base {thread_id: 父段id, end_byte_offset}`：这段会话的历史 =
//! 父段前 end_byte_offset 字节 + 本段（0.166 真机取证，tests/probe_codex_backtrack.rs；规则
//! 见 codex 源码 thread-store/src/local/rollout_lineage.rs，可多级）。原 rollout 从此不再增长，
//! 只读它对话页就停在回溯那一刻；只读续写段又丢了前面的全部历史。
//!
//! 宿主的对话管线建立在「一个只追加的 JSONL」上（增量偏移、翻页、reset），所以照 opencode
//! 的做法在插件里物化：有 history_base 时把「各父段前缀 + 当前段」拼进 `~/.meowo/cache`，当前段
//! 增长时原地追加；链变了（又回溯一次）换一代文件名，宿主按「路径变了」重置前端。没有
//! history_base 的会话（绝大多数）直接返回原文件，零开销。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// 缓存格式版本进目录名：拼法一变，旧缓存整体作废。
const CACHE_REL: &str = "cache/codex/v1";
/// 链深上限：防坏数据成环（codex 自己也做 cycle 检测）。
const MAX_DEPTH: usize = 32;

/// 首行 session_meta 里的 history_base：(父段 id, 父段截断字节)。
fn history_base(path: &Path) -> Option<(String, u64)> {
    let mut first = Vec::new();
    // 首行带整份 base_instructions，可能上百 KB，按行读完。
    BufReader::new(std::fs::File::open(path).ok()?)
        .read_until(b'\n', &mut first)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&first).ok()?;
    if value.get("type")?.as_str()? != "session_meta" {
        return None;
    }
    let base = value.pointer("/payload/history_base")?;
    Some((
        base.get("thread_id")?.as_str()?.to_string(),
        base.get("end_byte_offset")?.as_u64()?,
    ))
}

/// `…/sessions`：rollout 所在的会话根（默认目录与受管 profile 各有一个）。
fn sessions_root(path: &Path) -> Option<&Path> {
    path.ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "sessions"))
}

/// sessions 下最新的日期目录（`YYYY/MM/DD` 逐级取名字最大的）：续写段建在回溯当天。
fn newest_day_dir(root: &Path) -> Option<PathBuf> {
    let mut dir = root.to_path_buf();
    for _ in 0..3 {
        dir = std::fs::read_dir(&dir)
            .ok()?
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.path())
            .max()?;
    }
    Some(dir)
}

fn rollout_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            name.starts_with("rollout-") && name.ends_with(".jsonl")
                        })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 文件名里的段 id：`…-<线程id>.jsonl` 的段 id 就是线程 id，`…-<线程id>_<段id>.jsonl` 取下划线后。
fn belongs_to(path: &Path, thread_id: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.contains(&format!("-{thread_id}.")) || name.contains(&format!("-{thread_id}_"))
        })
}

/// 该线程当前在写的那一段：已知文件所在目录与最新日期目录里，属于该线程、文件名（时间戳打头）
/// 最大的那份。hook 记下的 transcript_path 多半是会话开头的根段，回溯后已不再增长。
pub(super) fn current_rollout(known: &Path, thread_id: &str) -> PathBuf {
    let mut dirs: Vec<PathBuf> = known.parent().map(Path::to_path_buf).into_iter().collect();
    if let Some(day) = sessions_root(known).and_then(newest_day_dir) {
        if !dirs.contains(&day) {
            dirs.push(day);
        }
    }
    dirs.iter()
        .flat_map(|dir| rollout_files(dir))
        .filter(|path| belongs_to(path, thread_id))
        .chain(std::iter::once(known.to_path_buf()))
        .max_by(|a, b| a.file_name().cmp(&b.file_name()))
        .unwrap_or_else(|| known.to_path_buf())
}

/// 按段 id 找文件：先在子段所在目录找，再整棵 sessions 树找（父段可能在更早的日期目录）。
fn segment_path(child: &Path, segment_id: &str) -> Option<PathBuf> {
    let root_name = format!("-{segment_id}.jsonl");
    let child_name = format!("_{segment_id}.jsonl");
    let matches = |path: &Path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.starts_with("rollout-")
                    && (name.ends_with(&root_name) || name.ends_with(&child_name))
            })
    };
    if let Some(found) = child
        .parent()
        .map(rollout_files)
        .unwrap_or_default()
        .into_iter()
        .find(|p| matches(p))
    {
        return Some(found);
    }
    let mut stack = vec![sessions_root(child)?.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if matches(&path) {
                return Some(path);
            }
        }
    }
    None
}

/// 截断点必须落在父段的行边界上（codex 写的是整行偏移）；不是就说明不是我们认识的格式。
fn ends_on_line(path: &Path, end: u64) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if end == 0 || end > len || file.seek(SeekFrom::Start(end - 1)).is_err() {
        return false;
    }
    let mut byte = [0u8; 1];
    file.read_exact(&mut byte).is_ok() && byte[0] == b'\n'
}

/// 对话页读的文件：当前段没有 history_base 时就是它本身；有则返回物化的拼接文件。拼不出来
/// （父段缺失、截断点不在行边界、写缓存失败）退回当前段——至少新消息照常显示。
pub(super) fn chat_path(current: &Path, thread_id: &str) -> PathBuf {
    compose(current, thread_id).unwrap_or_else(|| current.to_path_buf())
}

fn compose(current: &Path, thread_id: &str) -> Option<PathBuf> {
    // 父段链（从近到远）：(父段文件, 取它前多少字节)。
    let mut parents: Vec<(PathBuf, u64)> = Vec::new();
    let mut child = current.to_path_buf();
    while let Some((segment_id, end)) = history_base(&child) {
        if parents.len() >= MAX_DEPTH {
            return None;
        }
        let parent = segment_path(&child, &segment_id)?;
        if !ends_on_line(&parent, end) || parents.iter().any(|(path, _)| *path == parent) {
            return None;
        }
        parents.push((parent.clone(), end));
        child = parent;
    }
    if parents.is_empty() {
        return None;
    }
    parents.reverse();
    let prefix_len: u64 = parents.iter().map(|(_, end)| end).sum();

    let safe: String = thread_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(64)
        .collect();
    if safe.is_empty() {
        return None;
    }
    // 代号 = 整条链（含当前段文件名）的指纹：又回溯一次链就变，换文件名让宿主重置。
    let mut hasher = DefaultHasher::new();
    for (path, end) in &parents {
        path.file_name().hash(&mut hasher);
        end.hash(&mut hasher);
    }
    current.file_name().hash(&mut hasher);
    let dir = crate::meowo_data_root()?.join(CACHE_REL);
    std::fs::create_dir_all(&dir).ok()?;
    let target = dir.join(format!("{safe}.{:016x}.jsonl", hasher.finish()));
    // dev 与安装版共享 ~/.meowo（AGENTS.md「多实例」）：文件锁把「量长度 → 追加」串行化。
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(format!("{safe}.lock")))
        .ok()?;
    lock.lock().ok()?;

    let current_len = std::fs::metadata(current).ok()?.len();
    let have = std::fs::metadata(&target).map(|m| m.len()).ok();
    let copied = have
        .and_then(|len| len.checked_sub(prefix_len))
        .filter(|copied| *copied <= current_len);
    let copied = match copied {
        Some(copied) => copied,
        None => {
            // 新一代（或旧缓存对不上）：先写父段前缀，旧代删掉。
            let mut out = std::fs::File::create(&target).ok()?;
            for (path, end) in &parents {
                let mut part = Vec::with_capacity(*end as usize);
                std::fs::File::open(path)
                    .ok()?
                    .take(*end)
                    .read_to_end(&mut part)
                    .ok()?;
                out.write_all(&part).ok()?;
            }
            for entry in std::fs::read_dir(&dir).ok()?.flatten() {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if path != target
                    && name.starts_with(&format!("{safe}."))
                    && name.ends_with(".jsonl")
                {
                    let _ = std::fs::remove_file(&path);
                }
            }
            0
        }
    };
    if copied < current_len {
        let mut file = std::fs::File::open(current).ok()?;
        file.seek(SeekFrom::Start(copied)).ok()?;
        let mut fresh = Vec::new();
        file.take(current_len - copied)
            .read_to_end(&mut fresh)
            .ok()?;
        // 只搬完整行：半行留到下一轮，缓存文件始终整行结尾。
        if let Some(cut) = fresh.iter().rposition(|b| *b == b'\n') {
            let mut out = std::fs::OpenOptions::new()
                .append(true)
                .open(&target)
                .ok()?;
            out.write_all(&fresh[..=cut]).ok()?;
        }
    }
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0.166 真机形态：根段 + 回溯后的续写段（history_base 指根段的行边界）。拼出「根段前缀 +
    /// 续写段」，续写段增长时原地追加，再回溯一次（新续写段挂在上一段下）换一代文件。
    #[test]
    fn composes_root_prefix_with_continuation_and_follows_new_segments() {
        let _env = crate::env_guard();
        let home = std::env::temp_dir().join(format!("meowo_codex_lineage_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let day = home.join("sessions/2026/10/30");
        std::fs::create_dir_all(&day).unwrap();
        let old_db = std::env::var("MEOWO_DB").ok();
        std::env::set_var("MEOWO_DB", home.join(".meowo").join("board.db"));

        let tid = "01a18c24-1440-78e3-a758-570b2095570d";
        let meta = |base: Option<(&str, usize)>| {
            let base = base.map(|(id, end)| serde_json::json!({"thread_id": id, "end_ordinal_exclusive": 1, "end_byte_offset": end}));
            format!(
                "{}\n",
                serde_json::json!({"type": "session_meta", "payload": {"id": tid, "history_mode": "paginated", "history_base": base}})
            )
        };
        let line = |text: &str| {
            format!(
                "{}\n",
                serde_json::json!({"type": "event_msg", "payload": {"type": "user_message", "message": text}})
            )
        };
        let root = day.join(format!("rollout-2026-10-30T17-28-57-{tid}.jsonl"));
        let kept = format!("{}{}", meta(None), line("先做甲"));
        std::fs::write(&root, format!("{kept}{}", line("再做乙"))).unwrap();
        let seg1 = day.join(format!("rollout-2026-10-30T17-29-17-{tid}_seg-1.jsonl"));
        std::fs::write(
            &seg1,
            format!("{}{}", meta(Some((tid, kept.len()))), line("改做丙")),
        )
        .unwrap();

        let current = current_rollout(&root, tid);
        assert_eq!(current, seg1);
        let composed = chat_path(&current, tid);
        let body = std::fs::read_to_string(&composed).unwrap();
        assert!(body.contains("先做甲") && body.contains("改做丙") && !body.contains("再做乙"));

        // 续写段增长 → 同一文件原地追加。
        std::fs::OpenOptions::new()
            .append(true)
            .open(&seg1)
            .unwrap()
            .write_all(line("丙好了").as_bytes())
            .unwrap();
        assert_eq!(chat_path(&seg1, tid), composed);
        assert!(std::fs::read_to_string(&composed)
            .unwrap()
            .ends_with(&line("丙好了")));

        // 再回溯一次：新续写段挂在 seg-1 下（截在「改做丙」之后）→ 换一代，旧代删除。
        let seg1_keep = format!("{}{}", meta(Some((tid, kept.len()))), line("改做丙"));
        let seg2 = day.join(format!("rollout-2026-10-30T17-35-00-{tid}_seg-2.jsonl"));
        std::fs::write(
            &seg2,
            format!(
                "{}{}",
                meta(Some(("seg-1", seg1_keep.len()))),
                line("最后做丁")
            ),
        )
        .unwrap();
        let current = current_rollout(&root, tid);
        assert_eq!(current, seg2);
        let next = chat_path(&current, tid);
        assert_ne!(next, composed);
        assert!(!composed.exists());
        let body = std::fs::read_to_string(&next).unwrap();
        assert!(body.contains("先做甲") && body.contains("改做丙") && body.contains("最后做丁"));
        assert!(!body.contains("再做乙") && !body.contains("丙好了"));

        // 没有 history_base 的普通会话原样返回，不物化。
        let plain = day.join("rollout-2026-10-30T10-00-00-plain.jsonl");
        std::fs::write(&plain, meta(None)).unwrap();
        assert_eq!(chat_path(&plain, "plain"), plain);

        match old_db {
            Some(value) => std::env::set_var("MEOWO_DB", value),
            None => std::env::remove_var("MEOWO_DB"),
        }
        let _ = std::fs::remove_dir_all(&home);
    }
}
