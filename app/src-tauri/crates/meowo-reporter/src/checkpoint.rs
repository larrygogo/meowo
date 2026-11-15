//! 回合文件快照只用独立 index 与隐藏 ref；绝不 stash、reset 或改写用户的 HEAD/index。

use meowo_store::{Store, TurnCheckpoint};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);
const DIFF_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointFile {
    pub path: String,
    pub status: String,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointPatch {
    pub diff: String,
    pub truncated: bool,
}

fn run(cwd: &Path, args: &[&str], index: Option<&Path>) -> Result<Output, String> {
    let mut command = Command::new("git");
    command
        .current_dir(cwd)
        .args(["-c", "core.fsmonitor=false", "-c", "core.splitIndex=false"])
        .args(args)
        .env("GIT_AUTHOR_NAME", "Meowo")
        .env("GIT_AUTHOR_EMAIL", "meowo@localhost")
        .env("GIT_COMMITTER_NAME", "Meowo")
        .env("GIT_COMMITTER_EMAIL", "meowo@localhost");
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    command.output().map_err(|e| format!("checkpoint/git_spawn:{e}"))
}

fn output(cwd: &Path, args: &[&str], index: Option<&Path>) -> Result<Vec<u8>, String> {
    let out = run(cwd, args, index)?;
    if !out.status.success() {
        return Err(format!("checkpoint/git_{}:{}", args[0].replace('-', "_"), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out.stdout)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

pub fn repo_root(cwd: &Path) -> Result<Option<PathBuf>, String> {
    if !cwd.is_dir() {
        return Ok(None);
    }
    let out = run(cwd, &["rev-parse", "--show-toplevel"], None)?;
    if !out.status.success() {
        return Ok(None);
    }
    let root = text(&out.stdout);
    Ok((!root.is_empty()).then(|| PathBuf::from(root)))
}

fn head(repo: &Path) -> Option<String> {
    run(repo, &["rev-parse", "--verify", "HEAD"], None)
        .ok()
        .filter(|out| out.status.success())
        .map(|out| text(&out.stdout))
        .filter(|value| !value.is_empty())
}

struct TempIndex(PathBuf);
impl Drop for TempIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(self.0.with_extension("lock"));
    }
}

fn temp_index() -> TempIndex {
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    TempIndex(std::env::temp_dir().join(format!("meowo-checkpoint-index-{}-{seq}", std::process::id())))
}

fn checkpoint_ref(session_id: i64, turn: i64, phase: &str) -> String {
    format!("refs/meowo/checkpoints/{session_id}/turn/{turn}/{phase}")
}

pub fn capture(repo: &Path, session_id: i64, turn: i64, phase: &str) -> Result<String, String> {
    let head = head(repo);
    let index = temp_index();
    if head.is_some() {
        output(repo, &["read-tree", "HEAD"], Some(&index.0))?;
    } else {
        output(repo, &["read-tree", "--empty"], Some(&index.0))?;
    }
    // add 只写临时 index；默认排除 ignored，同时收进 tracked 删除和未跟踪文件。
    output(repo, &["add", "-A", "--", "."], Some(&index.0))?;
    let tree = text(&output(repo, &["write-tree"], Some(&index.0))?);
    if tree.is_empty() {
        return Err("checkpoint/empty_tree".into());
    }
    let mut args = vec!["commit-tree", tree.as_str()];
    if let Some(head) = head.as_deref() {
        args.extend(["-p", head]);
    }
    args.extend(["-m", "Meowo turn checkpoint"]);
    let commit = text(&output(repo, &args, Some(&index.0))?);
    if commit.is_empty() {
        return Err("checkpoint/empty_commit".into());
    }
    let reference = checkpoint_ref(session_id, turn, phase);
    output(repo, &["update-ref", &reference, &commit], None)?;
    Ok(reference)
}

pub fn start_turn(store: &Store, session_id: i64, turn: i64, cwd: &Path, now_ms: i64) {
    let Some(repo) = repo_root(cwd).ok().flatten() else {
        return;
    };
    start_repo(store, session_id, turn, &repo, now_ms);
}

pub fn start_repo(store: &Store, session_id: i64, turn: i64, repo: &Path, now_ms: i64) {
    let root = repo.to_string_lossy().replace('\\', "/");
    if store.turn_checkpoint_repos(session_id, turn).ok().is_some_and(|rows| rows.iter().any(|row| row.repo_root == root && !row.start_ref.is_empty())) {
        return;
    }
    match capture(repo, session_id, turn, "start") {
        Ok(reference) => {
            let _ = store.checkpoint_start(session_id, turn, &root, &reference, now_ms);
        }
        Err(error) => {
            let _ = store.checkpoint_error(session_id, turn, &root, &error, now_ms);
        }
    }
}

pub fn before_tool(store: &Store, session_id: i64, turn: i64, cwd: &Path, input: Option<&serde_json::Value>, now_ms: i64) {
    let Some(input) = input else {
        return;
    };
    let path = ["file_path", "path"].into_iter().find_map(|key| input.get(key).and_then(|value| value.as_str()));
    if let Some(path) = path {
        let path = Path::new(path);
        let full = if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
        let mut start = Some(full.as_path());
        while let Some(candidate) = start {
            if candidate.is_dir() {
                break;
            }
            start = candidate.parent();
        }
        if let Some(start) = start {
            if let Ok(Some(repo)) = repo_root(start) {
                start_repo(store, session_id, turn, &repo, now_ms);
            }
        }
    }
    // 优先用工具自带的 cwd/workdir；命令里的相对 cd 也必须在写文件前发现目标仓库。
    let base = ["cwd", "workdir"].into_iter().find_map(|key| input.get(key).and_then(|value| value.as_str())).map(PathBuf::from).unwrap_or_else(|| cwd.to_path_buf());
    if let Ok(Some(repo)) = repo_root(&base) {
        start_repo(store, session_id, turn, &repo, now_ms);
    }
    if let Some(command) = input.get("command").and_then(|value| value.as_str()) {
        let tokens: Vec<&str> = command.split_whitespace().take(64).collect();
        for (index, raw) in tokens.iter().enumerate() {
            let token = raw.trim_matches(|ch: char| matches!(ch, '\'' | '"' | ';' | '&' | '|'));
            let relative_cd = index > 0 && matches!(tokens[index - 1], "cd" | "pushd");
            let path = Path::new(token);
            if !path.is_absolute() && !relative_cd {
                continue;
            }
            let full = if path.is_absolute() { path.to_path_buf() } else { base.join(path) };
            let start = if full.is_dir() { Some(full.as_path()) } else { full.parent() };
            if let Some(repo) = start.and_then(|dir| repo_root(dir).ok().flatten()) {
                start_repo(store, session_id, turn, &repo, now_ms);
            }
        }
    }
}

pub fn finish_turn(store: &Store, session_id: i64, turn: i64, now_ms: i64) {
    let Ok(rows) = store.turn_checkpoint_repos(session_id, turn) else {
        return;
    };
    for row in rows.into_iter().filter(|row| !row.start_ref.is_empty()) {
        match capture(Path::new(&row.repo_root), session_id, turn, "end") {
            Ok(reference) => {
                let _ = store.checkpoint_finish(session_id, turn, &row.repo_root, &reference, now_ms);
            }
            Err(error) => {
                let _ = store.checkpoint_error(session_id, turn, &row.repo_root, &error, now_ms);
            }
        }
    }
}

fn split_z(bytes: &[u8]) -> Vec<String> {
    bytes.split(|byte| *byte == 0).filter(|part| !part.is_empty()).map(|part| String::from_utf8_lossy(part).into_owned()).collect()
}

pub fn changed_files(row: &TurnCheckpoint) -> Result<Vec<CheckpointFile>, String> {
    let end = row.end_ref.as_deref().ok_or("checkpoint/not_finished")?;
    let repo = Path::new(&row.repo_root);
    let statuses = split_z(&output(repo, &["diff", "--no-renames", "--name-status", "-z", &row.start_ref, end], None)?);
    let nums = split_z(&output(repo, &["diff", "--no-renames", "--numstat", "-z", &row.start_ref, end], None)?);
    let mut counts = BTreeMap::new();
    for entry in nums {
        let mut fields = entry.splitn(3, '\t');
        let added = fields.next().unwrap_or("").parse::<u32>().ok();
        let deleted = fields.next().unwrap_or("").parse::<u32>().ok();
        if let Some(path) = fields.next() {
            counts.insert(path.to_string(), (added, deleted));
        }
    }
    let mut files = Vec::new();
    let mut index = 0;
    while index < statuses.len() {
        let status = statuses[index].clone();
        let (status, path) = if let Some((code, path)) = status.split_once('\t') {
            (code.to_string(), path.to_string())
        } else {
            index += 1;
            (status, statuses.get(index).cloned().unwrap_or_default())
        };
        if path.is_empty() {
            continue;
        }
        let (additions, deletions) = counts.get(&path).copied().unwrap_or((None, None));
        files.push(CheckpointFile { path, status, additions, deletions });
        index += 1;
    }
    Ok(files)
}

fn safe_relative(path: &str) -> Result<PathBuf, String> {
    let rel = Path::new(path);
    if rel.is_absolute() || rel.components().any(|part| !matches!(part, Component::Normal(_))) {
        return Err("checkpoint/path_outside".into());
    }
    Ok(rel.to_path_buf())
}

pub fn file_diff(row: &TurnCheckpoint, path: &str) -> Result<CheckpointPatch, String> {
    safe_relative(path)?;
    if !changed_files(row)?.iter().any(|file| file.path == path) {
        return Err("checkpoint/path_not_in_turn".into());
    }
    let end = row.end_ref.as_deref().ok_or("checkpoint/not_finished")?;
    let bytes = output(Path::new(&row.repo_root), &["diff", "--no-ext-diff", "--no-renames", &row.start_ref, end, "--", path], None)?;
    let truncated = bytes.len() > DIFF_LIMIT;
    let mut diff = String::from_utf8_lossy(&bytes[..bytes.len().min(DIFF_LIMIT)]).into_owned();
    if diff.contains("Binary files ") || diff.contains("\nGIT binary patch\n") {
        diff = "(binary file)".into();
    }
    Ok(CheckpointPatch { diff, truncated })
}

fn paths_from(bytes: &[u8]) -> BTreeSet<String> {
    split_z(bytes).into_iter().filter(|path| safe_relative(path).is_ok()).collect()
}

pub fn restore(repo: &Path, reference: &str) -> Result<(), String> {
    output(repo, &["rev-parse", "--verify", &format!("{reference}^{{commit}}")], None)?;
    let current = paths_from(&output(repo, &["ls-files", "--cached", "--others", "--exclude-standard", "-z"], None)?);
    let target = paths_from(&output(repo, &["ls-tree", "-r", "--name-only", "-z", reference], None)?);
    for path in current.difference(&target) {
        let full = repo.join(safe_relative(path)?);
        match std::fs::remove_file(&full) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("checkpoint/remove_failed:{path}:{error}")),
        }
    }
    let index = temp_index();
    output(repo, &["read-tree", reference], Some(&index.0))?;
    let mut command = Command::new("git");
    command.current_dir(repo).env("GIT_INDEX_FILE", &index.0).env("GIT_WORK_TREE", repo).args(["checkout-index", "--all", "--force"]);
    let out = command.output().map_err(|e| format!("checkpoint/git_spawn:{e}"))?;
    if !out.status.success() {
        return Err(format!("checkpoint/checkout_failed:{}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use meowo_store::RestoreCheckpoint;

    fn git(repo: &Path, args: &[&str]) -> String {
        let out = Command::new("git").current_dir(repo).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        text(&out.stdout)
    }

    fn fixture() -> (PathBuf, Store) {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("meowo-checkpoint-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@localhost"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        std::fs::write(root.join("tracked.txt"), "old\n").unwrap();
        std::fs::write(root.join("delete.txt"), "delete\n").unwrap();
        std::fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "base"]);
        (root, Store::open_in_memory().unwrap())
    }

    #[test]
    fn snapshots_cover_worktree_without_touching_head_or_real_index() {
        let (repo, store) = fixture();
        let head = git(&repo, &["rev-parse", "HEAD"]);
        let index = std::fs::read(repo.join(".git/index")).unwrap();
        start_turn(&store, 7, 1, &repo, 100);
        std::fs::write(repo.join("tracked.txt"), "new\n").unwrap();
        std::fs::remove_file(repo.join("delete.txt")).unwrap();
        std::fs::write(repo.join("new.txt"), "created\n").unwrap();
        std::fs::write(repo.join("binary.dat"), [0, 1, 2, 255]).unwrap();
        std::fs::write(repo.join("ignored.txt"), "leave me\n").unwrap();
        finish_turn(&store, 7, 1, 200);
        let row = store.turn_checkpoint_repos(7, 1).unwrap().remove(0);
        let files = changed_files(&row).unwrap();
        let paths: BTreeSet<_> = files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, BTreeSet::from(["binary.dat", "delete.txt", "new.txt", "tracked.txt"]));
        assert!(files.iter().find(|file| file.path == "binary.dat").is_some_and(|file| file.additions.is_none()));
        assert_eq!(file_diff(&row, "binary.dat").unwrap().diff, "(binary file)");
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
        // 回合结束后的提交/拉取不属于这一回合：起止 ref 不变，新增文件不能混进历史 diff。
        std::fs::write(repo.join("later.txt"), "later\n").unwrap();
        git(&repo, &["add", "later.txt"]);
        git(&repo, &["commit", "-qm", "later"]);
        assert_eq!(changed_files(&row).unwrap().iter().map(|file| file.path.as_str()).collect::<BTreeSet<_>>(), paths);
    }

    #[test]
    fn restore_start_preserves_later_safety_snapshot_and_ignored_files() {
        let (repo, store) = fixture();
        start_turn(&store, 7, 1, &repo, 100);
        std::fs::write(repo.join("tracked.txt"), "agent\n").unwrap();
        std::fs::write(repo.join("new.txt"), "agent new\n").unwrap();
        finish_turn(&store, 7, 1, 200);
        let row = store.turn_checkpoint_repos(7, 1).unwrap().remove(0);
        std::fs::write(repo.join("tracked.txt"), "user later\n").unwrap();
        std::fs::write(repo.join("ignored.txt"), "untouched\n").unwrap();
        let safety = capture(&repo, 7, 1, "restore-before").unwrap();
        store.create_checkpoint_restore(7, &[RestoreCheckpoint { repo_root: row.repo_root.clone(), snapshot_ref: safety.clone() }], 300).unwrap();
        restore(&repo, &row.start_ref).unwrap();
        assert_eq!(std::fs::read_to_string(repo.join("tracked.txt")).unwrap(), "old\n");
        assert!(!repo.join("new.txt").exists());
        assert_eq!(std::fs::read_to_string(repo.join("ignored.txt")).unwrap(), "untouched\n");
        restore(&repo, &safety).unwrap();
        assert_eq!(std::fs::read_to_string(repo.join("tracked.txt")).unwrap(), "user later\n");
        assert_eq!(std::fs::read_to_string(repo.join("new.txt")).unwrap(), "agent new\n");
        assert!(file_diff(&row, "../outside").is_err());
    }

    #[test]
    fn linked_worktree_uses_its_own_files_and_shared_hidden_refs() {
        let (repo, store) = fixture();
        let worktree = repo.with_extension("wt");
        git(&repo, &["worktree", "add", "-qb", "checkpoint-test", worktree.to_str().unwrap()]);
        start_turn(&store, 8, 1, &worktree, 100);
        std::fs::write(worktree.join("tracked.txt"), "worktree\n").unwrap();
        finish_turn(&store, 8, 1, 200);
        let row = store.turn_checkpoint_repos(8, 1).unwrap().remove(0);
        assert_eq!(changed_files(&row).unwrap()[0].path, "tracked.txt");
        restore(&repo, &row.start_ref).unwrap();
        assert_eq!(std::fs::read_to_string(repo.join("tracked.txt")).unwrap(), "old\n");
        git(&repo, &["worktree", "remove", "--force", worktree.to_str().unwrap()]);
    }
}


#[cfg(test)]
mod performance_tests {
    use super::*;

    #[test]
    fn large_untracked_directory_and_long_diff_have_bounded_output() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let repo = std::env::temp_dir().join(format!("meowo-checkpoint-perf-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git").current_dir(&repo).args(args).output().unwrap();
            assert!(out.status.success());
        };
        git(&["init", "-q"]);
        git(&["config", "user.name", "Test"]);
        git(&["config", "user.email", "test@localhost"]);
        git(&["config", "core.autocrlf", "false"]);
        std::fs::write(repo.join("a.txt"), "before\n").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-qm", "base"]);
        let store = Store::open_in_memory().unwrap();
        start_turn(&store, 9, 1, &repo, 1);
        let started = std::time::Instant::now();
        std::fs::create_dir_all(repo.join("generated")).unwrap();
        for index in 0..200 {
            std::fs::write(repo.join(format!("generated/{index}.txt")), format!("file {index}\n")).unwrap();
        }
        std::fs::write(repo.join("a.txt"), "after\n".repeat(150_000)).unwrap();
        finish_turn(&store, 9, 1, 2);
        let row = store.turn_checkpoint_repos(9, 1).unwrap().remove(0);
        assert_eq!(changed_files(&row).unwrap().len(), 201);
        let patch = file_diff(&row, "a.txt").unwrap();
        assert!(patch.truncated);
        assert!(patch.diff.len() <= DIFF_LIMIT);
        eprintln!("checkpoint 200 files + 1.2MB diff: {:?}", started.elapsed());
    }
}


#[cfg(test)]
mod discovery_tests {
    use super::*;

    #[test]
    fn bash_relative_cd_registers_the_target_repo_before_its_writes() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("meowo-relative-cd-{}-{unique}", std::process::id()));
        let a = root.join("a");
        let b = root.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        for repo in [&a, &b] {
            let out = Command::new("git").current_dir(repo).args(["init", "-q"]).output().unwrap();
            assert!(out.status.success());
        }
        let store = Store::open_in_memory().unwrap();
        before_tool(&store, 12, 1, &a, Some(&serde_json::json!({"command":"cd ../b && printf changed > file.txt"})), 1);
        let rows = store.turn_checkpoint_repos(12, 1).unwrap();
        let roots = rows.iter().map(|row| row.repo_root.as_str()).collect::<Vec<_>>();
        let a_root = repo_root(&a).unwrap().unwrap().to_string_lossy().replace('\\', "/");
        let b_root = repo_root(&b).unwrap().unwrap().to_string_lossy().replace('\\', "/");
        assert_eq!(rows.len(), 2);
        assert!(roots.contains(&a_root.as_str()));
        assert!(roots.contains(&b_root.as_str()));
        let structured = Store::open_in_memory().unwrap();
        before_tool(&structured, 13, 1, &a, Some(&serde_json::json!({"workdir":b,"command":"printf changed > file.txt"})), 2);
        let structured_rows = structured.turn_checkpoint_repos(13, 1).unwrap();
        assert_eq!(structured_rows.len(), 1);
        assert_eq!(structured_rows[0].repo_root, b_root);
    }
}
