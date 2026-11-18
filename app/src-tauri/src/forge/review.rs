//! PR / MR 阅读、评论与持久监控。经统一的平台接口（forge/api.rs）：GitHub 的 gh 调用继承 forge 的注入点与
//! 超时，GitLab / Gitea / Bitbucket 走各自的接口。

use super::*;
use meowo_protocol::ipc::{PrCommentDto, PrFileDto, PrMonitorDto, PrMonitorEventDto, PrReviewDto, QueueDraftDto};
use meowo_store::{PrMonitor, Store};
use tauri::Emitter;
use std::collections::BTreeSet;
use std::future::Future;

fn string(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_string)
}
fn number(value: &serde_json::Value, key: &str) -> Option<i64> {
    value.get(key)?.as_i64()
}
fn author(value: &serde_json::Value) -> String {
    value.get("author").and_then(|v| v.get("login")).and_then(|v| v.as_str()).or_else(|| value.get("user").and_then(|v| v.get("login")).and_then(|v| v.as_str())).unwrap_or("unknown").to_string()
}
fn comment(value: &serde_json::Value, review: bool) -> Option<PrCommentDto> {
    let id = string(value, "id").or_else(|| number(value, "id").map(|id| id.to_string()))?;
    Some(PrCommentDto {
        id: if review { format!("review:{id}") } else { format!("comment:{id}") },
        author: author(value), body: string(value, "body").unwrap_or_default(),
        created_at: string(value, "createdAt").or_else(|| string(value, "created_at")).or_else(|| string(value, "submittedAt")).unwrap_or_default(),
        url: string(value, "url").or_else(|| string(value, "html_url")), state: string(value, "state"),
        path: string(value, "path"), line: number(value, "line").or_else(|| number(value, "original_line")), side: string(value, "side"),
    })
}
fn array_values(value: &serde_json::Value) -> Vec<&serde_json::Value> {
    match value {
        serde_json::Value::Array(items) => items.iter().flat_map(|item| array_values(item)).collect(),
        _ => vec![value],
    }
}
fn comments(value: &serde_json::Value, review: bool) -> Vec<PrCommentDto> {
    array_values(value).into_iter().filter_map(|item| comment(item, review)).collect()
}
fn files(value: &serde_json::Value) -> Vec<PrFileDto> {
    array_values(value).into_iter().filter_map(|item| {
        Some(PrFileDto {
            path: string(item, "path").or_else(|| string(item, "filename"))?, status: string(item, "status").unwrap_or_else(|| "modified".into()),
            additions: number(item, "additions").unwrap_or(0).max(0) as u32, deletions: number(item, "deletions").unwrap_or(0).max(0) as u32,
            patch: string(item, "patch"),
        })
    }).collect()
}

pub(crate) fn parse_review(value: &serde_json::Value, file_value: &serde_json::Value, inline: &serde_json::Value) -> Option<PrReviewDto> {
    let pr = parse_pr_full(&serde_json::to_string(value).ok()?)?;
    let mut all_comments = comments(value.get("comments")?, false);
    all_comments.extend(comments(inline, false));
    Some(PrReviewDto {
        pr, comments: all_comments, reviews: comments(value.get("reviews")?, true), files: files(file_value), monitor: None,
    })
}

fn read_with(ctx: &Ctx, cwd: &str) -> Result<Option<PrReviewDto>, String> {
    let dir = existing_dir(cwd)?;
    let info = repo_info(dir, ctx.env, &ctx.prefs, ctx.forges).ok_or("worktree/not_repo")?;
    let forge = usable(ctx, &info)?;
    let branch = info.branch.as_deref().ok_or("git/detached")?;
    let slug = info.gh.slug().map(str::to_string).ok_or("gh/ambiguous_repo")?;
    forge.review(dir, &slug, branch)
}

static REVIEW_CACHE: std::sync::LazyLock<Mutex<HashMap<String, (Instant, PrReviewDto)>>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// GitHub 连接变了：评审缓存作废。
pub(crate) fn invalidate_cache() {
    lock(&REVIEW_CACHE).clear();
}

fn read_cached_with(ctx: &Ctx, cwd: &str) -> Result<Option<PrReviewDto>, String> {
    if let Some((at, review)) = lock(&REVIEW_CACHE).get(cwd).cloned() {
        if at.elapsed() < Duration::from_secs(15) { return Ok(Some(review)); }
    }
    let review = read_with(ctx, cwd)?;
    if let Some(review) = review.as_ref() {
        lock(&REVIEW_CACHE).insert(cwd.to_string(), (Instant::now(), review.clone()));
    }
    Ok(review)
}

fn monitor_dto(store: &Store, monitor: PrMonitor) -> Result<PrMonitorDto, String> {
    let events = store.pr_events(&monitor.key).map_err(|e| e.to_string())?.into_iter().map(|event| PrMonitorEventDto {
        id: event.id, kind: event.kind, summary: event.summary, created_at: event.created_at, wake_state: event.wake_state,
    }).collect();
    Ok(PrMonitorDto { key: monitor.key, enabled: monitor.enabled, last_error: monitor.last_error, events })
}

#[tauri::command]
pub(crate) async fn pr_review(state: tauri::State<'_, crate::AppState>, cwd: String, _session_id: i64) -> Result<Option<PrReviewDto>, String> {
    let db = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut review = read_cached_with(&Ctx::real(), &cwd)?;
        if let Some(review) = review.as_mut() {
            let store = crate::open_store(&db).map_err(|e| e.to_string())?;
            let key = pr_key(&review.pr);
            if let Some(monitor) = store.pr_monitor(&key).map_err(|e| e.to_string())?.or(store.pr_monitor_for(&cwd, review.pr.number).map_err(|e| e.to_string())?) {
                review.monitor = Some(monitor_dto(&store, monitor)?);
            }
        }
        Ok(review)
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn pr_comment(cwd: String, number: u32, body: String, review: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || comment_with(&Ctx::real(), &cwd, number, &body, review)).await.map_err(|e| e.to_string())?
}
fn comment_with(ctx: &Ctx, cwd: &str, number: u32, body: &str, review: bool) -> Result<(), String> {
    if body.trim().is_empty() { return Err("pr/empty_comment".into()); }
    let info = repo_info(cwd, ctx.env, &ctx.prefs, ctx.forges).ok_or("worktree/not_repo")?;
    let forge = usable(ctx, &info)?;
    let slug = info.gh.slug().ok_or("gh/ambiguous_repo")?;
    forge.comment(cwd, slug, number, body, review)?;
    lock(&REVIEW_CACHE).remove(cwd);
    Ok(())
}

#[tauri::command]
pub(crate) async fn pr_line_comment(cwd: String, number: u32, path: String, line: i64, side: String, body: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || line_comment_with(&Ctx::real(), &cwd, number, &path, line, &side, &body)).await.map_err(|e| e.to_string())?
}
fn line_comment_with(ctx: &Ctx, cwd: &str, number: u32, path: &str, line: i64, side: &str, body: &str) -> Result<(), String> {
    if body.trim().is_empty() { return Err("pr/empty_comment".into()); }
    if !matches!(side, "NEW" | "OLD") || line <= 0 { return Err("pr/invalid_line".into()); }
    let info = repo_info(cwd, ctx.env, &ctx.prefs, ctx.forges).ok_or("worktree/not_repo")?;
    let forge = usable(ctx, &info)?;
    let slug = info.gh.slug().ok_or("gh/ambiguous_repo")?;
    forge.line_comment(cwd, slug, number, path, line, side, body)?;
    lock(&REVIEW_CACHE).remove(cwd);
    Ok(())
}

/// 监控的键：github.com 是 `owner/repo#号`（与改版前一致，老监控照用）；别的主机 `主机/路径#号`。
/// 认不出链接时只剩 `#号`，调用方据此拒绝。
fn pr_key(pr: &PullRequestDto) -> String {
    let slug = super::platform::parse_pr_url(&pr.url).map(|(host, slug, _, _)| super::platform::repo_key(&host, &slug)).unwrap_or_default();
    format!("{slug}#{}", pr.number)
}

fn disable_pr_monitor(store: &Store, key: &str, session_id: i64) -> Result<(), String> {
    if store.set_pr_monitor_enabled(key, session_id, false, crate::now_ms()).map_err(|e| e.to_string())? {
        Ok(())
    } else {
        Err("pr/monitor_not_found".into())
    }
}

#[tauri::command]
pub(crate) async fn pr_monitor_set(state: tauri::State<'_, crate::AppState>, cwd: String, session_id: i64, enabled: bool, key: Option<String>) -> Result<(), String> {
    let db = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = crate::open_store(&db).map_err(|e| e.to_string())?;
        // 停止是纯本地操作；离线、无 gh 或 PR 已消失都不能把用户困在监控里。
        if !enabled {
            if let Some(key) = key {
                disable_pr_monitor(&store, &key, session_id)?;
            }
            return Ok(());
        }
        let review = read_with(&Ctx::real(), &cwd)?.ok_or("pr/not_found")?;
        let key = pr_key(&review.pr);
        if key.starts_with('#') { return Err("gh/ambiguous_repo".into()); }
        store.upsert_pr_monitor(meowo_store::PrMonitorInput { key: &key, cwd: &cwd, session_id, slug: key.split('#').next().unwrap_or(""), number: review.pr.number, enabled }, crate::now_ms()).map_err(|e| e.to_string())
    }).await.map_err(|e| e.to_string())?
}

fn poll_with(ctx: &Ctx, store: &Store, monitor: &PrMonitor, app: Option<&tauri::AppHandle>) -> Result<(), String> {
    let review = match read_with(ctx, &monitor.cwd) {
        Ok(Some(review)) => review,
        Ok(None) => return Err("pr/not_found".into()),
        Err(error) => return Err(error),
    };
    let viewer = repo_info(&monitor.cwd, ctx.env, &ctx.prefs, ctx.forges)
        .and_then(|info| ctx.forge_for(info.gh.base.as_ref()).ok())
        .and_then(|forge| forge.viewer(&monitor.cwd))
        .unwrap_or_default();
    let mut known: BTreeSet<String> = serde_json::from_str(&monitor.known_comments).unwrap_or_default();
    let current: BTreeSet<String> = review.comments.iter().chain(&review.reviews).map(|comment| comment.id.clone()).collect();
    let now = crate::now_ms();
    if monitor.seeded {
        let checks = review.pr.checks_summary.as_deref().unwrap_or("none");
        if checks == "fail" && monitor.checks_state.as_deref() != Some("fail") {
            store.add_pr_event(&monitor.key, &format!("checks:fail:{}", review.pr.head_oid.as_deref().unwrap_or("")), "checks_failed", "CI failed", now).map_err(|e| e.to_string())?;
        }
        if checks == "pass" && monitor.checks_state.as_deref() != Some("pass") {
            store.add_pr_event(&monitor.key, &format!("checks:pass:{}", review.pr.head_oid.as_deref().unwrap_or("")), "checks_passed", "All checks passed", now).map_err(|e| e.to_string())?;
        }
        let conflict = review.pr.mergeable.as_deref() == Some("CONFLICTING") || review.pr.merge_state.as_deref() == Some("DIRTY");
        if conflict && monitor.mergeable.as_deref() != Some("CONFLICTING") && monitor.mergeable.as_deref() != Some("DIRTY") {
            store.add_pr_event(&monitor.key, &format!("conflict:{}", review.pr.head_oid.as_deref().unwrap_or("")), "conflict", "Branch has conflicts", now).map_err(|e| e.to_string())?;
        }
        for comment in review.comments.iter().chain(&review.reviews).filter(|comment| !known.contains(&comment.id) && comment.author != viewer) {
            let summary = format!("{}: {}", comment.author, comment.body.chars().take(160).collect::<String>());
            store.add_pr_event(&monitor.key, &comment.id, "comment", &summary, now).map_err(|e| e.to_string())?;
        }
    }
    known.extend(current);
    let known_json = serde_json::to_string(&known.into_iter().collect::<Vec<_>>()).map_err(|e| e.to_string())?;
    store.save_pr_poll(meowo_store::PrPollInput { key: &monitor.key, checks: review.pr.checks_summary.as_deref().unwrap_or("none"), mergeable: review.pr.mergeable.as_deref().or(review.pr.merge_state.as_deref()).unwrap_or(""), head: review.pr.head_oid.as_deref().unwrap_or(""), known: &known_json, error: None }, now).map_err(|e| e.to_string())?;
    if let Some(app) = app {
        let _ = app.emit("pr-monitor-changed", &monitor.key);
        for event in store.pr_events(&monitor.key).map_err(|e| e.to_string())?.into_iter().filter(|event| event.created_at == now) {
            let settings = crate::settings::load_settings();
            crate::push::push_session(&settings, crate::push::PushMessage { title: monitor_title(monitor), body: event.summary.clone(), session_id: monitor.session_id, urgent: false });
            if let Ok(header) = store.session_header(monitor.session_id) {
                let lang = crate::settings::ui_lang(&settings);
                crate::watch::show_session_notification(app, monitor_title(monitor), event.summary, monitor.session_id, header.pid.unwrap_or(0), header.title.clone().unwrap_or_default(), header.cwd, None, false, crate::watch::NotifyKind::Waiting, lang.to_string(), settings.notification_sound);
            }
        }
    }
    Ok(())
}

/// 通知标题：GitLab 的叫合并请求，编号写 `!号`（键里带主机，见 [`pr_key`]）。
fn monitor_title(monitor: &PrMonitor) -> String {
    let gitlab = monitor.key.split('#').next().and_then(|slug| slug.split('/').next()).is_some_and(|host| host == "gitlab.com" || crate::forge_accounts::ACCOUNTS.platform(host) == Some(super::platform::Platform::Gitlab));
    if gitlab { format!("MR !{}", monitor.number) } else { format!("PR #{}", monitor.number) }
}

pub(crate) fn start_monitor_worker(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut failures: HashMap<String, u32> = HashMap::new();
        let mut next: HashMap<String, Instant> = HashMap::new();
        loop {
            let Ok(store) = crate::open_store(&crate::db_path()) else { return; };
            if let Ok(monitors) = store.enabled_pr_monitors() {
                for monitor in monitors {
                    if next.get(&monitor.key).is_some_and(|at| *at > Instant::now()) { continue; }
                    match poll_with(&Ctx::real(), &store, &monitor, Some(&app)) {
                        Ok(()) => { failures.remove(&monitor.key); next.remove(&monitor.key); }
                        Err(error) => {
                            let count = failures.get(&monitor.key).copied().unwrap_or(0) + 1;
                            failures.insert(monitor.key.clone(), count);
                            let delay = Duration::from_secs(60 * 2u64.pow(count.min(6) - 1));
                            next.insert(monitor.key.clone(), Instant::now() + delay);
                            let _ = store.pr_monitor_error(&monitor.key, &error, crate::now_ms());
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(60));
        }
    });
}

async fn wake_with<Q, W, QR, WR>(db: &Path, session_id: i64, monitor_key: &str, event_id: &str, queue: Q, write: W) -> Result<String, String>
where
    Q: FnOnce(String) -> QR,
    W: FnOnce(String) -> WR,
    QR: Future<Output = Result<(), String>>,
    WR: Future<Output = Result<(), String>>,
{
    let store = crate::open_store(db).map_err(|e| e.to_string())?;
    let event = store.pr_event_for_session(session_id, monitor_key, event_id).map_err(|e| e.to_string())?.ok_or("pr/event_not_found")?;
    if event.wake_state != "pending" { return Ok(event.wake_state); }
    if !store.mark_pr_event_wake(session_id, monitor_key, event_id, "pending", "queued").map_err(|e| e.to_string())? { return Ok("queued".into()); }
    let session = store.session_header(session_id).map_err(|e| e.to_string())?;
    let prompt = format!("GitHub PR 监控：{}\n请查看并处理这条新变化。", event.summary);
    drop(store);
    if session.status == "running" || session.pending_review.is_some() {
        if let Err(error) = queue(prompt).await {
            crate::open_store(db).map_err(|e| e.to_string())?.mark_pr_event_wake(session_id, monitor_key, event_id, "queued", "failed").map_err(|e| e.to_string())?;
            return Err(error);
        }
        return Ok("queued".into());
    }
    let result = write(format!("{prompt}\r")).await;
    let state_name = if result.is_ok() { "delivered" } else { "failed" };
    crate::open_store(db).map_err(|e| e.to_string())?.mark_pr_event_wake(session_id, monitor_key, event_id, "queued", state_name).map_err(|e| e.to_string())?;
    result.map(|_| state_name.to_string())
}

#[tauri::command]
pub(crate) async fn pr_monitor_wake(app: tauri::AppHandle, state: tauri::State<'_, crate::AppState>, session_id: i64, monitor_key: String, event_id: String) -> Result<String, String> {
    let queue_app = app.clone();
    let queue_state = state.clone();
    let queue = move |prompt| {
        let draft = QueueDraftDto { prompt, attachments: Vec::new() };
        let app = queue_app.clone();
        let state = queue_state.clone();
        async move { crate::prompt_queue::prompt_queue_enqueue(app, state, session_id, draft).map(|_| ()) }
    };
    let write_state = state.clone();
    let write = move |prompt| async move { crate::managed_terminal::write_managed_terminal(write_state, session_id, prompt).await };
    wake_with(&state.db_path, session_id, &monitor_key, &event_id, queue, write).await
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn ok(stdout: String) -> Option<CmdOut> {
        Some(CmdOut { code: Some(0), stdout, stderr: String::new(), timed_out: false })
    }
    fn git(repo: &Path, args: &[&str]) {
        let out = Command::new("git").current_dir(repo).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
    fn repo() -> PathBuf {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let repo = std::env::temp_dir().join(format!("meowo-review-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["remote", "add", "origin", "https://github.com/acme/repo.git"]);
        repo
    }
    fn pr_json(comments: serde_json::Value, checks: &str, head: &str) -> String {
        serde_json::json!({
            "number": 12, "state": "OPEN", "isDraft": false, "url": "https://github.com/acme/repo/pull/12", "title": "Fix",
            "baseRefName": "main", "headRefName": "fix", "headRefOid": head, "mergeable": "MERGEABLE", "mergeStateStatus": "CLEAN",
            "statusCheckRollup": [{"name":"test","status":"COMPLETED","conclusion":if checks == "pass" { "SUCCESS" } else { "FAILURE" }}],
            "comments": comments, "reviews": [{"id":"r1","author":{"login":"reviewer"},"body":"Looks good","state":"APPROVED","submittedAt":"2026-01-01T00:00:00Z"}]
        }).to_string()
    }

    #[test]
    fn recorded_review_payload_maps_discussion_reviews_files_and_inline_lines() {
        let value: serde_json::Value = serde_json::from_str(&pr_json(serde_json::json!([{"id":"c1","author":{"login":"alice"},"body":"hello","createdAt":"2026-01-01T00:00:00Z"}]), "pass", "abc")).unwrap();
        let review = parse_review(&value, &serde_json::json!([[{"filename":"src/a.ts","status":"modified","additions":2,"deletions":1,"patch":"@@ -1 +1 @@\n-old\n+new"}]]), &serde_json::json!([[{"id":9,"user":{"login":"alice"},"body":"line","path":"src/a.ts","line":1,"side":"NEW"}]])).unwrap();
        assert_eq!(review.pr.number, 12);
        assert_eq!(review.reviews[0].state.as_deref(), Some("APPROVED"));
        assert_eq!(review.files[0].path, "src/a.ts");
        assert_eq!(review.comments.iter().map(|comment| comment.id.as_str()).collect::<Vec<_>>(), ["comment:c1", "comment:9"]);
    }

    #[test]
    fn poll_seeds_first_then_dedupes_comments_and_check_transitions() {
        let repo = repo();
        let phase = std::sync::Arc::new(AtomicUsize::new(0));
        let gh_phase = phase.clone();
        let gh = move |_cwd: &str, args: &[&str]| {
            if args.starts_with(&["auth", "status"]) { return ok(String::new()); }
            if args.starts_with(&["api", "user"]) { return ok(r#"{"login":"me"}"#.into()); }
            if args.starts_with(&["pr", "view"]) {
                let current = gh_phase.load(Ordering::SeqCst);
                let comments = if current == 0 { serde_json::json!([{"id":"old","author":{"login":"alice"},"body":"old"}]) }
                    else { serde_json::json!([{"id":"old","author":{"login":"alice"},"body":"old"},{"id":"new","author":{"login":"alice"},"body":"new"},{"id":"mine","author":{"login":"me"},"body":"mine"}]) };
                let mut value: serde_json::Value = serde_json::from_str(&pr_json(comments, if current == 0 || current >= 3 { "pass" } else { "fail" }, if current < 2 { "a" } else { "b" })).unwrap();
                if current >= 3 { value["mergeable"] = serde_json::Value::String("CONFLICTING".into()); }
                return ok(serde_json::to_string(&value).unwrap());
            }
            if args.starts_with(&["api"]) { return ok("[]".into()); }
            None
        };
        let caches = Caches::default();
        let ctx = Ctx { forges: &crate::forge::platform::NO_ACCOUNTS, gh: &gh, caches: &caches, prefs: repo.join("forge.json"), env: &[] };
        let store = Store::open_in_memory().unwrap();
        store.upsert_pr_monitor(meowo_store::PrMonitorInput { key: "acme/repo#12", cwd: repo.to_str().unwrap(), session_id: 7, slug: "acme/repo", number: 12, enabled: true }, 1).unwrap();
        let monitor = store.pr_monitor("acme/repo#12").unwrap().unwrap();
        poll_with(&ctx, &store, &monitor, None).unwrap();
        assert!(store.pr_events(&monitor.key).unwrap().is_empty(), "首轮只建基线");
        phase.store(1, Ordering::SeqCst);
        poll_with(&ctx, &store, &store.pr_monitor(&monitor.key).unwrap().unwrap(), None).unwrap();
        let events = store.pr_events(&monitor.key).unwrap();
        assert_eq!(events.len(), 2, "只报别人新评论和检查失败，自己的评论不报");
        phase.store(2, Ordering::SeqCst);
        poll_with(&ctx, &store, &store.pr_monitor(&monitor.key).unwrap().unwrap(), None).unwrap();
        assert_eq!(store.pr_events(&monitor.key).unwrap().len(), 2, "重推与重复轮询不重复告警");
        phase.store(3, Ordering::SeqCst);
        poll_with(&ctx, &store, &store.pr_monitor(&monitor.key).unwrap().unwrap(), None).unwrap();
        let kinds = store.pr_events(&monitor.key).unwrap().into_iter().map(|event| event.kind).collect::<BTreeSet<_>>();
        assert!(kinds.contains("checks_passed") && kinds.contains("conflict"));
        phase.store(4, Ordering::SeqCst);
        poll_with(&ctx, &store, &store.pr_monitor(&monitor.key).unwrap().unwrap(), None).unwrap();
        assert_eq!(store.pr_events(&monitor.key).unwrap().len(), 4, "冲突与通过状态持续时不重复告警");
    }

    #[test]
    fn writes_use_injected_gh_and_line_comment_pins_current_head() {
        let repo = repo();
        let calls = Mutex::new(Vec::<Vec<String>>::new());
        let gh = |cwd: &str, args: &[&str]| {
            calls.lock().unwrap().push(std::iter::once(cwd.to_string()).chain(args.iter().map(|arg| arg.to_string())).collect());
            if args.starts_with(&["auth", "status"]) { return ok(String::new()); }
            if args.starts_with(&["pr", "view"]) { return ok(r#"{"headRefOid":"current-head"}"#.into()); }
            ok(String::new())
        };
        let caches = Caches::default();
        let ctx = Ctx { forges: &crate::forge::platform::NO_ACCOUNTS, gh: &gh, caches: &caches, prefs: repo.join("forge.json"), env: &[] };
        comment_with(&ctx, repo.to_str().unwrap(), 12, "note", true).unwrap();
        line_comment_with(&ctx, repo.to_str().unwrap(), 12, "src/a.ts", 3, "NEW", "line note").unwrap();
        let calls = calls.into_inner().unwrap();
        assert!(calls.iter().any(|args| args.contains(&"--comment".into()) && args.contains(&"note".into())));
        let line = calls.last().unwrap().join(" ");
        assert!(line.contains("commit_id=current-head") && line.contains("path=src/a.ts") && line.contains("line=3") && line.contains("side=NEW"));
    }

    #[test]
    fn disabling_monitor_requires_its_session_without_touching_the_worktree() {
        let store = Store::open_in_memory().unwrap();
        store.upsert_pr_monitor(meowo_store::PrMonitorInput { key: "acme/repo#12", cwd: "C:/removed-worktree", session_id: 8, slug: "acme/repo", number: 12, enabled: true }, 1).unwrap();
        assert_eq!(disable_pr_monitor(&store, "acme/repo#12", 7), Err("pr/monitor_not_found".into()));
        assert!(store.pr_monitor("acme/repo#12").unwrap().unwrap().enabled);
        disable_pr_monitor(&store, "acme/repo#12", 8).unwrap();
        assert!(!store.pr_monitor("acme/repo#12").unwrap().unwrap().enabled);
    }
}


#[cfg(test)]
mod wake_tests {
    use super::*;
    use meowo_store::SessionStatus;
    use std::sync::{Arc, Mutex};

    fn fixture(status: SessionStatus) -> (PathBuf, i64) {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("meowo-wake-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("board.db");
        let store = Store::open(&db).unwrap();
        let project = store.upsert_project_by_root("C:/repo", "repo", 1).unwrap();
        let (sid, _) = store.start_session(project, "wake", 1).unwrap();
        store.set_session_status(sid, status, 2).unwrap();
        store.upsert_pr_monitor(meowo_store::PrMonitorInput { key: "acme/repo#12", cwd: "C:/repo", session_id: sid, slug: "acme/repo", number: 12, enabled: true }, 3).unwrap();
        store.add_pr_event("acme/repo#12", "comment:new", "comment", "new comment", 4).unwrap();
        (db, sid)
    }

    #[test]
    fn running_turn_queues_instead_of_writing_and_duplicate_click_is_noop() {
        let (db, sid) = fixture(SessionStatus::Running);
        let queued = Arc::new(Mutex::new(Vec::new()));
        let written = Arc::new(Mutex::new(Vec::new()));
        let q = queued.clone();
        let w = written.clone();
        let state = tauri::async_runtime::block_on(wake_with(&db, sid, "acme/repo#12", "comment:new",
            move |prompt| { q.lock().unwrap().push(prompt); std::future::ready(Ok::<(), String>(())) },
            move |prompt| { w.lock().unwrap().push(prompt); std::future::ready(Ok::<(), String>(())) })).unwrap();
        assert_eq!(state, "queued");
        assert_eq!(queued.lock().unwrap().len(), 1);
        assert!(written.lock().unwrap().is_empty());
        tauri::async_runtime::block_on(wake_with(&db, sid, "acme/repo#12", "comment:new",
            |_| std::future::ready(Err("must not queue twice".into())), |_| std::future::ready(Err("must not write twice".into())))).unwrap();
        assert_eq!(queued.lock().unwrap().len(), 1);
    }

    #[test]
    fn idle_turn_writes_prompt_and_enter_once() {
        let (db, sid) = fixture(SessionStatus::Waiting);
        let written = Arc::new(Mutex::new(Vec::new()));
        let w = written.clone();
        let state = tauri::async_runtime::block_on(wake_with(&db, sid, "acme/repo#12", "comment:new",
            |_| std::future::ready(Err("idle must not queue".into())),
            move |prompt| { w.lock().unwrap().push(prompt); std::future::ready(Ok::<(), String>(())) })).unwrap();
        assert_eq!(state, "delivered");
        let text = written.lock().unwrap();
        assert!(text[0].contains("new comment") && text[0].ends_with("\r"));
        assert!(!text[0].contains('\x1b'));
    }
}
