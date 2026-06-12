//! 首次启动时导入 ~/.claude/projects 下近期的历史会话（标记为 ended）。
//! 复用 cc-store 的标题解析与本 crate 的项目命名逻辑。

use crate::dispatch::project_root_and_name;
use cc_store::{Store, StoreError};
use std::path::Path;

/// 导入参数。
#[derive(Debug, Clone, Copy)]
pub struct ImportOpts {
    /// 仅导入 mtime 距 now 不超过该毫秒数的会话。
    pub within_ms: i64,
    /// 最多导入条数（按 mtime 倒序取最新）。
    pub max_count: usize,
}

impl Default for ImportOpts {
    fn default() -> Self {
        ImportOpts {
            within_ms: 7 * 24 * 60 * 60 * 1000, // 7 天
            max_count: 30,
        }
    }
}

/// 从 ~/.claude/projects 导入近期历史会话。返回新导入条数。
/// HOME 不可解析或目录不存在时返回 Ok(0)。
pub fn import_recent(store: &Store, now_ms: i64, opts: ImportOpts) -> Result<usize, StoreError> {
    let Some(dir) = claude_projects_dir() else {
        return Ok(0);
    };
    import_from_dir(&dir, store, now_ms, opts)
}

/// 从指定 projects 目录导入（测试可注入 tempdir）。
pub fn import_from_dir(
    projects_dir: &Path,
    store: &Store,
    now_ms: i64,
    opts: ImportOpts,
) -> Result<usize, StoreError> {
    // 收集 (mtime_ms, cc_session_id, transcript_path, 编码目录名)
    let mut found: Vec<(i64, String, std::path::PathBuf, String)> = Vec::new();
    let Ok(dirs) = std::fs::read_dir(projects_dir) else {
        return Ok(0);
    };
    for dir in dirs.flatten() {
        if !dir.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let dir_name = dir.file_name().to_string_lossy().to_string();
        let Ok(files) = std::fs::read_dir(dir.path()) else {
            continue;
        };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(mtime) = mtime_ms(&path) else {
                continue;
            };
            if now_ms - mtime > opts.within_ms {
                continue;
            }
            found.push((mtime, stem.to_string(), path.clone(), dir_name.clone()));
        }
    }
    // 最新优先，取上限。
    found.sort_by_key(|e| std::cmp::Reverse(e.0));
    found.truncate(opts.max_count);

    let mut imported = 0usize;
    for (mtime, cc_session_id, path, dir_name) in found {
        let title = path
            .to_str()
            .and_then(cc_store::title::title_from_transcript)
            .unwrap_or_else(|| "(未命名会话)".to_string());
        let cwd = path.to_str().and_then(cc_store::title::cwd_from_transcript);
        let (root, name) = match cwd.as_deref() {
            Some(c) => project_root_and_name(c),
            None => fallback_project(&dir_name),
        };
        let project_id = store.upsert_project_by_root(&root, &name, mtime)?;
        if store.import_session(&cc_session_id, project_id, &title, cwd.as_deref(), mtime)? {
            imported += 1;
        }
    }
    Ok(imported)
}

fn claude_projects_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    Some(Path::new(&home).join(".claude").join("projects"))
}

/// 文件 mtime 转 Unix 毫秒。
fn mtime_ms(path: &Path) -> Option<i64> {
    let mt = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(mt.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as i64)
}

/// 无 cwd 兜底：root 用编码目录名本身，name 取其 '-' 分隔的末段非空片段。
///
/// 权衡：编码目录名无法还原真实路径（`-` 与原字符不可逆），用它当 root_path 可能与
/// 同一项目的真实路径并存为两个项目行；但 sessions.project_id 为 NOT NULL，跳过项目
/// 创建就得整条丢弃该会话。无 cwd 的 transcript 很罕见，宁可多一行兜底项目也不丢历史会话。
fn fallback_project(dir_name: &str) -> (String, String) {
    let name = dir_name.rsplit('-').find(|s| !s.is_empty()).unwrap_or(dir_name);
    (dir_name.to_string(), name.to_string())
}
