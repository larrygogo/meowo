//! worktree 会话的「原项目」：主检出的根目录。看板、侧栏、灵动岛按它把 worktree 会话归到原项目下。
//!
//! 只读文件系统，不起 git 进程：看板每次刷新都要给几十张卡各算一次，起进程（Windows 上每个
//! 30ms 起）会把列表拖慢。规则和 git 自己找公共目录一样：
//! - 往上找 `.git`。是目录 → 这是主检出（或普通仓库），不是 linked worktree，返回 None；
//! - 是文件 → 读 `gitdir: <路径>`（可能是相对 `.git` 文件所在目录的相对路径）；
//!   gitdir 里有 `commondir` → linked worktree，公共目录 = gitdir + commondir（commondir
//!   通常是相对路径 `../..`）；没有 commondir → 子模块，返回 None；
//! - 公共目录叫 `.git` → 主检出就是它的上一级；否则是 bare 仓库，没有主检出，返回 None。
//!
//! 主检出返回 None 而不是自己的根目录：主检出子目录里的会话（如 `repo/app`）原来按 cwd 单独成组，
//! 填成根目录会让它们并进 `repo` 组，改了不勾 worktree 的用户的现状。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 推算 `cwd` 的主检出根目录。不在 linked worktree 里返回 None。
pub(crate) fn origin_root_of(cwd: &Path) -> Option<PathBuf> {
    let mut dir = cwd;
    loop {
        let dot_git = dir.join(".git");
        match std::fs::symlink_metadata(&dot_git) {
            Ok(meta) if meta.is_dir() => return None,
            Ok(_) => return from_git_file(dir, &dot_git),
            Err(_) => {}
        }
        dir = dir.parent()?;
    }
}

fn from_git_file(top: &Path, dot_git: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(dot_git).ok()?;
    let gitdir = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("gitdir:"))?
        .trim();
    let gitdir = resolve(top, gitdir);
    let commondir = std::fs::read_to_string(gitdir.join("commondir")).ok()?;
    let common = resolve(&gitdir, commondir.trim());
    let common = dunce::canonicalize(&common).unwrap_or(common);
    if common.file_name()? != ".git" {
        return None;
    }
    let root = common.parent()?.to_path_buf();
    // 主检出被删、只剩公共目录的情况下，不把会话归到一个不存在的目录。
    root.is_dir().then_some(root)
}

fn resolve(base: &Path, p: &str) -> PathBuf {
    let path = PathBuf::from(p);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

/// 按 cwd 缓存 30 秒：同一批卡片反复刷新时不重复读盘；worktree 被删或新建后半分钟内自愈。
const TTL: Duration = Duration::from_secs(30);
type Entry = (Instant, Option<String>);
static CACHE: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);

/// 带缓存的版本，返回展示用的路径字符串（正斜杠，与库里 cwd 的常见写法一致）。
pub(crate) fn origin_root_cached(cwd: &str) -> Option<String> {
    if cwd.trim().is_empty() {
        return None;
    }
    {
        let guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, v)) = guard.as_ref().and_then(|m| m.get(cwd)) {
            if at.elapsed() < TTL {
                return v.clone();
            }
        }
    }
    let value = origin_root_of(Path::new(cwd)).map(|p| p.to_string_lossy().replace('\\', "/"));
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    // 防止无限增长：卡片 cwd 的种类有限，超过上限说明是长期运行积累的，整表清掉重来。
    if map.len() > 2048 {
        map.clear();
    }
    map.insert(cwd.to_string(), (Instant::now(), value.clone()));
    value
}

/// 新建 worktree 时直接记下：不用等缓存过期，也不再读盘。
pub(crate) fn remember(cwd: &str, origin: &str) {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    guard.get_or_insert_with(HashMap::new).insert(
        cwd.to_string(),
        (Instant::now(), Some(origin.replace('\\', "/"))),
    );
}
