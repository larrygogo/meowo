//! 把项目里的旧 `.lead` 搬进 Meowo 数据目录（关卡 B-20261124-lead-migrate，leader 选「自动搬走」）。
//!
//! 规则（照关卡选项原文）：先复制到新位置、核对文件一致，再删项目里那份，同时把登记表里的旧路径
//! 改成新路径；正在等关卡答复的任务先不动；被 git 跟踪的文件只复制不删，交给用户决定。
//! 另外跳过还有人在用的任务：归属会话（`.owner`）还没结束的、最近 30 分钟内改过的。agent 记着旧的绝对路径，
//! 搬走后它一写又在项目里长出半截目录（redteam-1 问题 2：只看修改时间时，闲置 30 分钟的会话照样被搬）。

use super::{lock_registry, owner_of, read_gate, slug_ok, strip_verbatim};
use meowo_agent::lead_paths;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 最近改过的任务视为还在推进，这一轮不搬。
const ACTIVE_WINDOW: Duration = Duration::from_secs(30 * 60);
/// 项目目录里记「已搬完、但因被 git 跟踪而留在原处」的旧目录，读任务时不再列它们，免得同一任务出现两份。
const LEGACY_FILE: &str = "legacy.json";

/// 本次迁移的结果，宿主写进诊断日志。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub moved: Vec<String>,
    /// （任务名，原因）
    pub skipped: Vec<(String, &'static str)>,
    /// 被 git 跟踪、复制后留在原处的旧目录。
    pub kept_tracked: Option<PathBuf>,
}

/// 项目里还没迁完的旧 `.lead`：会话所在仓库根的、worktree 对应主检出的。已登记为「被跟踪留在原处」
/// 的不算。
pub fn legacy_dirs(cwd: &Path) -> Vec<PathBuf> {
    legacy_dirs_in(lead_paths::lead_home(cwd).as_deref(), cwd)
}

fn legacy_dirs_in(home: Option<&Path>, cwd: &Path) -> Vec<PathBuf> {
    let root = lead_paths::project_root(cwd);
    let main = lead_paths::main_checkout(&root);
    let retired = home.map(retired_dirs).unwrap_or_default();
    std::iter::once(root)
        .chain(main)
        .map(|r| r.join(".lead"))
        .filter(|d| d.is_dir() && Some(d.as_path()) != home && !retired.iter().any(|r| same(r, d)))
        .collect()
}

/// 被 git 跟踪、复制后留在原处且还在的旧目录（面板提示用户自己决定删不删）。
pub fn kept_dirs(cwd: &Path) -> Vec<PathBuf> {
    lead_paths::lead_home(cwd).map(|home| retired_dirs(&home)).unwrap_or_default().into_iter().filter(|d| d.is_dir()).collect()
}

fn retired_dirs(home: &Path) -> Vec<PathBuf> {
    let Some(file) = home.parent().map(|p| p.join(LEGACY_FILE)) else {
        return Vec::new();
    };
    std::fs::read_to_string(file)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|v| v.get("kept").cloned())
        .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

fn same(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    norm(a) == norm(b)
}

/// 迁移该工作目录所属项目的旧任务。`tracked` 由调用方判断（宿主跑 `git ls-files`），这里不起子进程，
/// hook 路径也能安全调用纯文件操作的部分。
/// `live` 是还没结束的会话 id（agent 自己的会话 id，与 `.owner` 同一种），由宿主从库里取。
pub fn migrate(cwd: &Path, registry: &Path, tracked: impl Fn(&Path) -> bool, live: &HashSet<String>) -> Option<Report> {
    let home = lead_paths::ensure_lead_home(cwd)?;
    Some(migrate_into(&home, cwd, registry, tracked, live, SystemTime::now()))
}

pub(crate) fn migrate_into(
    home: &Path,
    cwd: &Path,
    registry: &Path,
    tracked: impl Fn(&Path) -> bool,
    live: &HashSet<String>,
    now: SystemTime,
) -> Report {
    let mut report = Report::default();
    for legacy in legacy_dirs_in(Some(home), cwd) {
        let keep = tracked(&legacy);
        let mut renames = Vec::new();
        let skipped_before = report.skipped.len();
        let Ok(entries) = std::fs::read_dir(&legacy) else { continue };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for from in entries {
            let name = from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let to = home.join(&name);
            if from.is_dir() {
                if !slug_ok(&name) {
                    report.skipped.push((name, "lead/migrate_name"));
                    continue;
                }
                if read_gate(&from).is_some_and(|g| g.status == "pending") {
                    report.skipped.push((name, "lead/migrate_gate_pending"));
                    continue;
                }
                let owner = owner_of(&from);
                if !owner.is_empty() && live.contains(&owner) {
                    report.skipped.push((name, "lead/migrate_owner_live"));
                    continue;
                }
                if recently_touched(&from, now) {
                    report.skipped.push((name, "lead/migrate_active"));
                    continue;
                }
                if to.exists() {
                    if keep {
                        // 被跟踪的目录上一轮已经复制过（原处保留），这次不算冲突。
                        continue;
                    }
                    // 旧目录里每个文件新位置都有、且内容一样：上一轮已经搬过，只是删旧目录时失败了
                    // （文件被占用等，redteam-1 问题 3）。按搬过处理、接着删；有一处不同才是真冲突，两份都留。
                    if !subset_identical(&from, &to) {
                        report.skipped.push((name, "lead/migrate_conflict"));
                        continue;
                    }
                    renames.push((from.clone(), to));
                    report.moved.push(name);
                    continue;
                }
                if copy_verified(&from, &to).is_err() {
                    let _ = std::fs::remove_dir_all(&to);
                    report.skipped.push((name, "lead/migrate_copy_failed"));
                    continue;
                }
                renames.push((from.clone(), to));
                report.moved.push(name);
            } else if !to.exists() {
                // 项目级约定（product.md、standards.md、harness.md、active）：新位置已有同名文件时以新位置为准。
                if std::fs::copy(&from, &to).is_err() {
                    report.skipped.push((name, "lead/migrate_copy_failed"));
                    continue;
                }
            }
        }
        // 先改登记表再删旧目录：record_owner 会丢弃指向不存在目录的条目，顺序反了正在跑的会话会丢任务。
        rewrite_registry(registry, &renames);
        if keep {
            // 全部复制完才把旧目录登记为「留在原处」：还有任务没搬时登记了，那些任务就从面板上消失了。
            if report.skipped.len() == skipped_before {
                retire(home, &legacy);
            }
            report.kept_tracked = Some(legacy);
            continue;
        }
        for (from, _) in &renames {
            let _ = std::fs::remove_dir_all(from);
        }
        remove_copied_files(&legacy, home);
        // 只剩空目录才删 .lead 本身：跳过的任务留在原处，下一轮再搬。
        let _ = std::fs::remove_dir(&legacy);
    }
    report
}

fn recently_touched(dir: &Path, now: SystemTime) -> bool {
    walk(dir).iter().any(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .is_ok_and(|t| now.duration_since(t).unwrap_or_default() < ACTIVE_WINDOW)
    })
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            // 不跟随 symlink/junction：迁移只搬任务自己的文件。
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() => out.push(p),
                _ => {}
            }
        }
    }
    out
}

/// `from` 的每个文件在 `to` 里都有同名、同内容的副本（`to` 可以多出文件：新位置之后又写过）。
fn subset_identical(from: &Path, to: &Path) -> bool {
    walk(from).iter().all(|file| {
        file.strip_prefix(from)
            .ok()
            .and_then(|rel| std::fs::read(to.join(rel)).ok())
            .zip(std::fs::read(file).ok())
            .is_some_and(|(a, b)| a == b)
    })
}

/// 复制整个任务目录，再逐个核对大小与内容，任何一处不一致都算失败（调用方清掉半成品）。
fn copy_verified(from: &Path, to: &Path) -> std::io::Result<()> {
    for file in walk(from) {
        let rel = file.strip_prefix(from).map_err(std::io::Error::other)?;
        let dest = to.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&file, &dest)?;
        if std::fs::read(&file)? != std::fs::read(&dest)? {
            return Err(std::io::Error::other("lead/migrate_verify"));
        }
    }
    std::fs::create_dir_all(to)
}

/// 删掉项目级约定文件里已经原样复制过去的那些；新位置内容不同（用户在新位置改过）时保留旧文件不删。
fn remove_copied_files(legacy: &Path, home: &Path) {
    let Ok(entries) = std::fs::read_dir(legacy) else { return };
    for from in entries.flatten().map(|e| e.path()).filter(|p| p.is_file()) {
        let Some(name) = from.file_name() else { continue };
        let same_content = std::fs::read(&from).ok().zip(std::fs::read(home.join(name)).ok()).is_some_and(|(a, b)| a == b);
        if same_content {
            let _ = std::fs::remove_file(&from);
        }
    }
}

fn rewrite_registry(registry: &Path, renames: &[(PathBuf, PathBuf)]) {
    if renames.is_empty() {
        return;
    }
    let Some(_lock) = lock_registry(registry) else { return };
    let Ok(raw) = std::fs::read_to_string(registry) else { return };
    let Ok(Value::Object(mut map)) = serde_json::from_str::<Value>(&raw) else { return };
    let mut changed = false;
    for value in map.values_mut() {
        let Some(old) = value.as_str() else { continue };
        let old = strip_verbatim(PathBuf::from(old));
        if let Some((_, to)) = renames.iter().find(|(from, _)| same(from, &old)) {
            *value = Value::String(to.to_string_lossy().into_owned());
            changed = true;
        }
    }
    if changed {
        let tmp = registry.with_extension(format!("tmp{}", std::process::id()));
        if std::fs::write(&tmp, Value::Object(map).to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, registry);
        }
    }
}

fn retire(home: &Path, legacy: &Path) {
    let Some(file) = home.parent().map(|p| p.join(LEGACY_FILE)) else { return };
    let mut kept: Vec<String> = retired_dirs(home).iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let path = legacy.to_string_lossy().into_owned();
    if !kept.iter().any(|k| same(Path::new(k), legacy)) {
        kept.push(path);
    }
    let _ = std::fs::write(file, json!({ "kept": kept }).to_string());
}

#[cfg(test)]
#[path = "migrate_tests.rs"]
mod tests;
