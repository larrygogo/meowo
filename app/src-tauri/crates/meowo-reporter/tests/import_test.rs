use filetime::{set_file_mtime, FileTime};
use meowo_reporter::import::{import_from_dir, ImportOpts};
use meowo_store::Store;
use std::fs;
use std::path::Path;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// 在 projects_dir/<dir>/<session>.jsonl 写可选 cwd + 可选 ai-title 的 transcript，并设置 mtime。
/// cwd 用 unix 风格路径避免反斜杠转义。
fn write_transcript(
    projects_dir: &Path,
    dir: &str,
    session: &str,
    cwd: Option<&str>,
    ai_title: Option<&str>,
    mtime_secs: i64,
) {
    let d = projects_dir.join(dir);
    fs::create_dir_all(&d).unwrap();
    let path = d.join(format!("{session}.jsonl"));
    let mut lines: Vec<String> = Vec::new();
    if let Some(c) = cwd {
        lines.push(format!(r#"{{"type":"user","cwd":"{c}"}}"#));
    }
    if let Some(t) = ai_title {
        lines.push(format!(r#"{{"type":"ai-title","aiTitle":"{t}"}}"#));
    }
    if lines.is_empty() {
        lines.push("{}".to_string());
    }
    fs::write(&path, lines.join("\n")).unwrap();
    set_file_mtime(&path, FileTime::from_unix_time(mtime_secs, 0)).unwrap();
}

#[test]
fn imports_only_recent_and_marks_ended() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path();
    let now_s = now_secs();
    let now_ms = now_s * 1000;
    write_transcript(
        proj,
        "dirA",
        "recent",
        Some("/home/me/foo"),
        Some("我的标题"),
        now_s - 3600,
    );
    write_transcript(
        proj,
        "dirB",
        "old",
        Some("/home/me/bar"),
        None,
        now_s - 10 * 24 * 3600,
    );

    let store = Store::open_in_memory().unwrap();
    let n = import_from_dir(proj, &store, now_ms, ImportOpts::default()).unwrap();
    assert_eq!(n, 1);

    let sid = store.find_session_id_pub("recent").unwrap().unwrap();
    let s = store.get_session(sid).unwrap();
    assert_eq!(s.status, "ended");
    assert_eq!(
        store.session_cwd(sid).unwrap(),
        Some("/home/me/foo".to_string())
    );

    let tid = store.task_id_of_session_pub(sid).unwrap();
    let t = store.get_task(tid).unwrap();
    assert_eq!(t.title, "我的标题");
    assert_eq!(t.column, "done");

    let names: Vec<String> = store
        .list_projects()
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert!(names.contains(&"foo".to_string()));

    assert!(store.find_session_id_pub("old").unwrap().is_none());
}

#[test]
fn respects_max_count_newest_first() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path();
    let now_s = now_secs();
    for i in 0..5 {
        write_transcript(
            proj,
            &format!("d{i}"),
            &format!("s{i}"),
            Some("/home/me/p"),
            None,
            now_s - (i as i64) * 60,
        );
    }
    let store = Store::open_in_memory().unwrap();
    let opts = ImportOpts {
        within_ms: 7 * DAY_MS,
        max_count: 3,
    };
    let n = import_from_dir(proj, &store, now_s * 1000, opts).unwrap();
    assert_eq!(n, 3);
    for i in 0..3 {
        assert!(
            store
                .find_session_id_pub(&format!("s{i}"))
                .unwrap()
                .is_some(),
            "s{i} 应被导入"
        );
    }
    for i in 3..5 {
        assert!(
            store
                .find_session_id_pub(&format!("s{i}"))
                .unwrap()
                .is_none(),
            "s{i} 不应被导入"
        );
    }
}

#[test]
fn does_not_overwrite_existing_session() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path();
    let now_s = now_secs();
    write_transcript(
        proj,
        "dirX",
        "dup",
        Some("/home/me/x"),
        Some("导入标题"),
        now_s - 100,
    );

    let store = Store::open_in_memory().unwrap();
    let p = store
        .upsert_project_by_root("/home/me/x", "x", now_s * 1000)
        .unwrap();
    let (sid, _) = store.start_session(p, "dup", now_s * 1000).unwrap();

    let n = import_from_dir(proj, &store, now_s * 1000, ImportOpts::default()).unwrap();
    assert_eq!(n, 0);
    assert_eq!(store.get_session(sid).unwrap().status, "running");
}

#[test]
fn fallback_project_name_without_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path();
    let now_s = now_secs();
    write_transcript(
        proj,
        "C--Users-me-myproj",
        "nocwd",
        None,
        Some("标题X"),
        now_s - 50,
    );

    let store = Store::open_in_memory().unwrap();
    let n = import_from_dir(proj, &store, now_s * 1000, ImportOpts::default()).unwrap();
    assert_eq!(n, 1);
    let names: Vec<String> = store
        .list_projects()
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert!(names.contains(&"myproj".to_string()));
}
