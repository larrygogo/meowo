use super::*;
use std::fs;

struct Fx {
    _tmp: tempfile::TempDir,
    repo: PathBuf,
    home: PathBuf,
    reg: PathBuf,
}

fn fx() -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let base = strip_verbatim(tmp.path().canonicalize().unwrap());
    let repo = base.join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    let home = lead_paths::ensure_lead_home_in(&base.join("data/projects"), &repo).unwrap();
    let reg = base.join("lead-sessions.json");
    Fx { _tmp: tmp, repo, home, reg }
}

fn task(fx: &Fx, slug: &str, gate_status: Option<&str>) -> PathBuf {
    let t = fx.repo.join(".lead").join(slug);
    fs::create_dir_all(t.join("agents")).unwrap();
    fs::write(t.join("progress.md"), "phase: done\n").unwrap();
    fs::write(t.join("agents/redteam-1.md"), "## 总结论\n通过").unwrap();
    if let Some(status) = gate_status {
        fs::write(
            t.join("gate.json"),
            json!({"id":"A-1","type":"A","title":"t","summary":"s","options":[],"status":status}).to_string(),
        )
        .unwrap();
    }
    t
}

/// 文件 mtime 是刚写的；把「现在」拨到一小时后，任务就不算最近改过。
fn later() -> SystemTime {
    SystemTime::now() + Duration::from_secs(3600)
}

#[test]
fn moves_tasks_and_project_files_then_removes_legacy() {
    let fx = fx();
    let old = task(&fx, "20261101-a", None);
    fs::write(fx.repo.join(".lead/product.md"), "- 事实").unwrap();
    fs::write(&fx.reg, json!({"s1": old.to_string_lossy()}).to_string()).unwrap();

    let report = migrate_into(&fx.home, &fx.repo, &fx.reg, |_| false, &HashSet::new(), later());

    assert_eq!(report.moved, vec!["20261101-a".to_owned()]);
    assert!(fx.home.join("20261101-a/agents/redteam-1.md").is_file());
    assert_eq!(fs::read_to_string(fx.home.join("product.md")).unwrap(), "- 事实");
    assert!(!fx.repo.join(".lead").exists(), "项目里的 .lead 应被删掉");
    let reg: Value = serde_json::from_str(&fs::read_to_string(&fx.reg).unwrap()).unwrap();
    assert_eq!(reg["s1"].as_str().map(PathBuf::from), Some(fx.home.join("20261101-a")));
    assert!(legacy_dirs_in(Some(&fx.home), &fx.repo).is_empty());
}

#[test]
fn pending_gate_and_recent_tasks_stay_until_next_round() {
    let fx = fx();
    task(&fx, "20261101-waiting", Some("pending"));
    task(&fx, "20261101-answered", Some("answered"));

    let report = migrate_into(&fx.home, &fx.repo, &fx.reg, |_| false, &HashSet::new(), later());
    assert_eq!(report.moved, vec!["20261101-answered".to_owned()]);
    assert_eq!(report.skipped, vec![("20261101-waiting".to_owned(), "lead/migrate_gate_pending")]);
    assert!(fx.repo.join(".lead/20261101-waiting/gate.json").is_file(), "等答复的任务不动");
    assert_eq!(legacy_dirs_in(Some(&fx.home), &fx.repo), vec![fx.repo.join(".lead")], "没搬完的旧目录照常可读");

    task(&fx, "20261101-fresh", None);
    let report = migrate_into(&fx.home, &fx.repo, &fx.reg, |_| false, &HashSet::new(), SystemTime::now());
    assert!(report.skipped.contains(&("20261101-fresh".to_owned(), "lead/migrate_active")));
    assert!(fx.repo.join(".lead/20261101-fresh").is_dir());
}

#[test]
fn tracked_legacy_is_copied_kept_and_hidden_once_complete() {
    let fx = fx();
    task(&fx, "20261101-a", None);

    let report = migrate_into(&fx.home, &fx.repo, &fx.reg, |_| true, &HashSet::new(), later());
    assert_eq!(report.moved, vec!["20261101-a".to_owned()]);
    assert_eq!(report.kept_tracked.as_deref(), Some(fx.repo.join(".lead").as_path()));
    assert!(fx.repo.join(".lead/20261101-a/progress.md").is_file(), "被 git 跟踪的不删");
    assert!(fx.home.join("20261101-a/progress.md").is_file());
    assert!(legacy_dirs_in(Some(&fx.home), &fx.repo).is_empty(), "复制完后不再列旧目录，免得任务出现两份");

    // 再跑一轮不报冲突、不重复复制。
    let again = migrate_into(&fx.home, &fx.repo, &fx.reg, |_| true, &HashSet::new(), later());
    assert!(again.skipped.is_empty() && again.moved.is_empty(), "{again:?}");
}

#[test]
fn conflict_keeps_both_copies() {
    let fx = fx();
    task(&fx, "20261101-a", None);
    fs::create_dir_all(fx.home.join("20261101-a")).unwrap();
    fs::write(fx.home.join("20261101-a/progress.md"), "phase: implement\n").unwrap();

    let report = migrate_into(&fx.home, &fx.repo, &fx.reg, |_| false, &HashSet::new(), later());
    assert_eq!(report.skipped, vec![("20261101-a".to_owned(), "lead/migrate_conflict")]);
    assert_eq!(fs::read_to_string(fx.home.join("20261101-a/progress.md")).unwrap(), "phase: implement\n");
    assert!(fx.repo.join(".lead/20261101-a").is_dir());
}

#[test]
fn task_of_unended_session_stays_even_when_idle() {
    let fx = fx();
    let old = task(&fx, "20261101-idle", None);
    fs::write(old.join(".owner"), "s-idle").unwrap();
    let live: HashSet<String> = ["s-idle".to_owned()].into();
    let report = migrate_into(&fx.home, &fx.repo, &fx.reg, |_| false, &live, later());
    assert_eq!(report.skipped, vec![("20261101-idle".to_owned(), "lead/migrate_owner_live")]);
    assert!(old.join("progress.md").is_file());
    // 会话结束后下一轮照常搬
    let report = migrate_into(&fx.home, &fx.repo, &fx.reg, |_| false, &HashSet::new(), later());
    assert_eq!(report.moved, vec!["20261101-idle".to_owned()]);
}

#[test]
fn half_deleted_leftover_is_finished_not_a_permanent_conflict() {
    let fx = fx();
    let old = task(&fx, "20261101-left", None);
    // 上一轮已复制成功、新位置还多写过一个文件；旧目录删到一半（只剩部分原样文件）
    copy_verified(&old, &fx.home.join("20261101-left")).unwrap();
    fs::write(fx.home.join("20261101-left/extra.md"), "new").unwrap();
    fs::remove_file(old.join("agents/redteam-1.md")).unwrap();
    let report = migrate_into(&fx.home, &fx.repo, &fx.reg, |_| false, &HashSet::new(), later());
    assert_eq!(report.moved, vec!["20261101-left".to_owned()]);
    assert!(!old.exists());
    assert!(fx.home.join("20261101-left/extra.md").is_file());
    // 旧位置有一处内容不同（迁移后又被写过）：真冲突，两份都留
    fs::create_dir_all(&old).unwrap();
    fs::write(old.join("progress.md"), "phase: verify
新写的").unwrap();
    let report = migrate_into(&fx.home, &fx.repo, &fx.reg, |_| false, &HashSet::new(), later());
    assert_eq!(report.skipped, vec![("20261101-left".to_owned(), "lead/migrate_conflict")]);
    assert!(old.join("progress.md").is_file());
}
