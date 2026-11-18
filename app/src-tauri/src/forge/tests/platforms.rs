//! GitLab / Gitea（Forgejo、Codeberg）/ Bitbucket Cloud / GitHub 企业版的平台实现：HTTP 是假的（按方法与地址
//! 回包），不打真网络。
//!
//! 样例来源（forge/fixtures/platforms，2026-11-18 不带令牌的只读 GET 原样存下，体积大的裁掉了与映射无关的字段）：
//! - gitlab_*：gitlab.com `gitlab-org/cli`（MR !4365 来自 fork、带 head_pipeline；!4468 草稿；!4453 合并结果流水线），
//!   jobs 分别录自 fork 项目 88359063 与目标项目 34675721 的流水线，`gitlab_user_401.json` 是 `/api/v4/user` 的 401。
//!   讨论（discussions）匿名拿不到（401），用 docs.gitlab.com/api/discussions 的示例形状。
//! - gitea_*：codeberg.org `forgejo/forgejo`（PR #15303 来自 fork mfenniak/forgejo；#15307 标题 WIP: 是草稿；#15271 的
//!   评审与行内评论；`commits/6b42eac1…/status` 含 skipped 与相对 target_url）。409 / 405 的原话取自 Gitea 源码
//!   routers/api/v1/repo/pull.go 与 models/issues/pull.go。
//! - bitbucket_*：bitbucket.org `atlassian/atlassian-connect-express`（PR #604 打开、一人批准；#552 草稿、FAILED 构建、
//!   带行内评论；按分支查的列表；diffstat 跟随 302 后的结果）。错误体形状取自官方 OpenAPI（swagger.v3.json）的 error。

use super::super::api::{ForgeApi, NewPr, Target};
use super::super::bitbucket::{self, Bitbucket};
use super::super::gitea::{self, Gitea};
use super::super::gitlab::{self, Gitlab};
use super::super::http::{Auth, Client, ForgeHttp, Req};
use super::super::GhState;
use crate::github::{Body, Response, Token};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/forge/fixtures/platforms").join(name);
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
}

type Handler = dyn Fn(&str, &str, &Value) -> Response + Sync;
/// (方法, 地址, JSON 体, 认证头)。
type Call = (String, String, Value, Option<String>);

/// 假平台：记下每次请求（方法、地址、JSON 体、认证头），回包由测试给的函数决定。
struct Fake {
    handler: Box<Handler>,
    calls: Mutex<Vec<Call>>,
}

impl Fake {
    fn new(handler: impl Fn(&str, &str, &Value) -> Response + Sync + 'static) -> Self {
        Fake {
            handler: Box::new(handler),
            calls: Mutex::new(Vec::new()),
        }
    }
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
    fn urls(&self) -> Vec<String> {
        self.calls().into_iter().map(|(m, u, _, _)| format!("{m} {u}")).collect()
    }
}

impl ForgeHttp for Fake {
    fn send(&self, req: &Req) -> Result<Response, String> {
        let body = match &req.body {
            Some(Body::Json(v)) => v.clone(),
            _ => Value::Null,
        };
        self.calls
            .lock()
            .unwrap()
            .push((req.method.to_string(), req.url.to_string(), body.clone(), req.auth.map(Auth::header)));
        Ok((self.handler)(req.method, req.url, &body))
    }
}

fn resp(status: u16, body: Value) -> Response {
    Response {
        status,
        body: body.to_string(),
        ..Default::default()
    }
}

fn token() -> Token {
    Token::secret("glpat-TEST").unwrap()
}

fn client<'a>(fake: &'a Fake, auth: Auth, flags: &'a Mutex<Vec<bool>>) -> Client<'a> {
    Client {
        http: Box::new(fake),
        auth: Some(auth),
        on_auth: Box::new(move |ok| flags.lock().unwrap().push(ok)),
    }
}

fn gitlab<'a>(fake: &'a Fake, flags: &'a Mutex<Vec<bool>>) -> Gitlab<'a> {
    Gitlab {
        client: client(fake, Auth::Bearer(token()), flags),
        host: "gitlab.com".into(),
        state: GhState::Ok,
        login: Some("me".into()),
    }
}

fn gitea<'a>(fake: &'a Fake, flags: &'a Mutex<Vec<bool>>) -> Gitea<'a> {
    Gitea {
        client: client(fake, Auth::Token(token()), flags),
        host: "codeberg.org".into(),
        state: GhState::Ok,
        login: Some("me".into()),
    }
}

fn bitbucket<'a>(fake: &'a Fake, flags: &'a Mutex<Vec<bool>>) -> Bitbucket<'a> {
    Bitbucket {
        client: client(
            fake,
            Auth::Basic {
                user: "me@example.com".into(),
                token: token(),
            },
            flags,
        ),
        state: GhState::Ok,
        login: Some("me".into()),
    }
}

fn branch(name: &str) -> Target<'_> {
    Target::Branch {
        qualified: name,
        branch: name,
        head_slug: None,
    }
}

// ═══ GitLab ═══

#[test]
fn gitlab_mr_映射_fork_的流水线_job_手动与允许失败不挡() {
    let mr = fixture("gitlab_mr_4365.json");
    let jobs = fixture("gitlab_jobs_3082056756.json");
    let pr = gitlab::map_mr(&mr, jobs.as_array().map(Vec::as_slice)).unwrap();
    assert_eq!((pr.number, pr.state.as_str(), pr.forge.as_deref()), (4365, "open", Some("gitlab")));
    assert_eq!(pr.url, "https://gitlab.com/gitlab-org/cli/-/merge_requests/4365");
    assert_eq!(
        (pr.base_ref.as_deref(), pr.head_ref.as_deref()),
        (Some("main"), Some("feat/login-detect-ssh-host"))
    );
    assert_eq!(pr.head_oid.as_deref(), Some("afce14513e44f97e36e28a68e843111e3ce8bd73"));
    // requested_changes：可以合并（没冲突），但被挡住。
    assert_eq!((pr.mergeable.as_deref(), pr.merge_state.as_deref()), (Some("MERGEABLE"), Some("BLOCKED")));
    assert_eq!(pr.checks.len(), 13);
    assert!(
        pr.checks.iter().all(|c| c.state == "pass" || c.state == "skipped"),
        "手动 job（allow_failure）算跳过"
    );
    assert!(pr
        .checks
        .iter()
        .all(|c| c.workflow.is_some() && c.url.as_deref().is_some_and(|u| u.starts_with("https://gitlab.com/"))));
    assert_eq!(pr.checks_summary.as_deref(), Some("pass"));
    // 拿不到 job 明细：用流水线整体状态当一项。
    let pr = gitlab::map_mr(&mr, None).unwrap();
    assert_eq!(
        (pr.checks.len(), pr.checks[0].state.as_str(), pr.checks_summary.as_deref()),
        (1, "pass", Some("pass"))
    );
    // 草稿。
    let draft = gitlab::map_mr(&fixture("gitlab_mr_4468_draft.json"), None).unwrap();
    assert_eq!((draft.state.as_str(), draft.merge_state.as_deref()), ("draft", Some("DRAFT")));
}

#[test]
fn gitlab_合并状态与_job_状态_全部取值() {
    // docs.gitlab.com/api/merge_requests 的 detailed_merge_status 全集。
    let cases = [
        ("mergeable", "MERGEABLE", "CLEAN"),
        ("conflict", "CONFLICTING", "DIRTY"),
        ("broken_status", "CONFLICTING", "DIRTY"),
        ("checking", "UNKNOWN", "UNKNOWN"),
        ("unchecked", "UNKNOWN", "UNKNOWN"),
        ("preparing", "UNKNOWN", "UNKNOWN"),
        ("approvals_syncing", "UNKNOWN", "UNKNOWN"),
        ("draft_status", "MERGEABLE", "DRAFT"),
        ("need_rebase", "MERGEABLE", "BEHIND"),
        ("not_approved", "MERGEABLE", "BLOCKED"),
        ("requested_changes", "MERGEABLE", "BLOCKED"),
        ("ci_must_pass", "MERGEABLE", "BLOCKED"),
        ("ci_still_running", "MERGEABLE", "BLOCKED"),
        ("discussions_not_resolved", "MERGEABLE", "BLOCKED"),
        ("not_open", "MERGEABLE", "BLOCKED"),
        ("merge_request_blocked", "MERGEABLE", "BLOCKED"),
        ("commits_status", "MERGEABLE", "BLOCKED"),
        ("jira_association_missing", "MERGEABLE", "BLOCKED"),
        ("merge_time", "MERGEABLE", "BLOCKED"),
        ("security_policy_violations", "MERGEABLE", "BLOCKED"),
        ("security_policy_pipeline_check", "MERGEABLE", "BLOCKED"),
        ("status_checks_must_pass", "MERGEABLE", "BLOCKED"),
        ("locked_paths", "MERGEABLE", "BLOCKED"),
        ("locked_lfs_files", "MERGEABLE", "BLOCKED"),
        ("title_regex", "MERGEABLE", "BLOCKED"),
    ];
    for (status, mergeable, state) in cases {
        let got = gitlab::merge_status(&json!({ "detailed_merge_status": status, "has_conflicts": false, "draft": false }));
        assert_eq!(got, (mergeable.to_string(), state.to_string()), "{status}");
    }
    assert_eq!(
        gitlab::merge_status(&json!({ "detailed_merge_status": "need_rebase", "has_conflicts": true })).0,
        "CONFLICTING"
    );
    // docs.gitlab.com/api/jobs 的状态全集。
    for (status, want) in [
        ("success", "pass"),
        ("failed", "fail"),
        ("canceled", "fail"),
        ("canceling", "fail"),
        ("skipped", "skipped"),
        ("created", "pending"),
        ("pending", "pending"),
        ("running", "pending"),
        ("preparing", "pending"),
        ("scheduled", "pending"),
        ("waiting_for_resource", "pending"),
        ("waiting_for_callback", "pending"),
        ("manual", "pending"),
    ] {
        assert_eq!(gitlab::job_state(status, false), want, "{status}");
    }
    assert_eq!(gitlab::job_state("failed", true), "skipped", "允许失败的 job 失败不挡合并");
    assert_eq!(gitlab::job_state("manual", true), "skipped");
}

/// 按分支找 fork 来的 MR：列表 → 源项目 id → 单个 MR → 源项目流水线的 job，共 4 个请求。
#[test]
fn gitlab_按分支找_fork_mr_流水线按源项目查() {
    let fake = Fake::new(|_, url, _| match url {
        u if u.starts_with("https://gitlab.com/api/v4/projects/gitlab-org%2Fcli/merge_requests?source_branch=feat%2Flogin-detect-ssh-host") => {
            resp(200, json!([fixture("gitlab_mr_4365.json")]))
        }
        "https://gitlab.com/api/v4/projects/johan.walles%2Fcli" => resp(200, json!({ "id": 88359063 })),
        "https://gitlab.com/api/v4/projects/gitlab-org%2Fcli/merge_requests/4365" => resp(200, fixture("gitlab_mr_4365.json")),
        "https://gitlab.com/api/v4/projects/88359063/pipelines/3082056756/jobs?per_page=100" => resp(200, fixture("gitlab_jobs_3082056756.json")),
        _ => resp(404, json!({ "message": "404 Not Found" })),
    });
    let flags = Mutex::new(Vec::new());
    let gl = gitlab(&fake, &flags);
    let target = Target::Branch {
        qualified: "johan.walles:feat/login-detect-ssh-host",
        branch: "feat/login-detect-ssh-host",
        head_slug: Some("johan.walles/cli"),
    };
    let pr = gl.find(".", Some("gitlab-org/cli"), &target).unwrap().unwrap();
    assert_eq!((pr.number, pr.checks.len()), (4365, 13));
    assert_eq!(fake.calls().len(), 4, "{:?}", fake.urls());
    assert!(fake.calls().iter().all(|(_, _, _, auth)| auth.as_deref() == Some("Bearer glpat-TEST")));
    // 同仓库按这个分支找：列表里只有 fork 来的，不算。
    let none = gl.find(".", Some("gitlab-org/cli"), &branch("feat/login-detect-ssh-host")).unwrap();
    assert_eq!(none, None);
}

#[test]
fn gitlab_默认分支上只剩已合并的_不算() {
    let mut merged = fixture("gitlab_mr_4453.json");
    merged["state"] = "merged".into();
    merged["source_project_id"] = merged["target_project_id"].clone();
    merged["source_branch"] = "main".into();
    let list = json!([merged]);
    let fake = Fake::new(move |_, url, _| match url {
        u if u.contains("/merge_requests?source_branch=main") => resp(200, list.clone()),
        "https://gitlab.com/api/v4/projects/gitlab-org%2Fcli" => resp(200, json!({ "id": 34675721, "default_branch": "main" })),
        _ => resp(500, json!({})),
    });
    let flags = Mutex::new(Vec::new());
    assert_eq!(gitlab(&fake, &flags).find(".", Some("gitlab-org/cli"), &branch("main")).unwrap(), None);
}

#[test]
fn gitlab_合并_钉住头提交_各种拒绝说清楚() {
    let pr = gitlab::map_mr(&fixture("gitlab_mr_4365.json"), None).unwrap();
    let status = std::sync::Arc::new(Mutex::new((200u16, json!({}))));
    let detailed = std::sync::Arc::new(Mutex::new("mergeable".to_string()));
    let user_ok = std::sync::Arc::new(AtomicUsize::new(1));
    let (s2, d2, u2) = (status.clone(), detailed.clone(), user_ok.clone());
    let fake = Fake::new(move |method, url, _| match (method, url) {
        ("PUT", u) if u.ends_with("/merge_requests/4365/merge") => {
            let (code, body) = s2.lock().unwrap().clone();
            resp(code, body)
        }
        ("GET", u) if u.ends_with("/merge_requests/4365") => {
            let mut mr = fixture("gitlab_mr_4365.json");
            mr["detailed_merge_status"] = d2.lock().unwrap().clone().into();
            resp(200, mr)
        }
        ("GET", "https://gitlab.com/api/v4/user") => {
            if u2.load(Ordering::SeqCst) == 1 {
                resp(200, json!({ "username": "me" }))
            } else {
                resp(401, fixture("gitlab_user_401.json"))
            }
        }
        _ => resp(404, json!({})),
    });
    let flags = Mutex::new(Vec::new());
    let gl = gitlab(&fake, &flags);
    let head = pr.head_oid.clone().unwrap();
    assert_eq!(gl.merge(".", "gitlab-org/cli", &pr, "squash", &head), Ok(false));
    let (_, _, body, _) = fake.calls().into_iter().find(|(m, _, _, _)| m == "PUT").unwrap();
    assert_eq!(body, json!({ "sha": head, "squash": true, "should_remove_source_branch": false }));
    // 源码 lib/api/helpers.rb：sha 不符回 409。
    *status.lock().unwrap() = (409, json!({ "message": "SHA does not match HEAD of source branch: abc" }));
    assert_eq!(gl.merge(".", "gitlab-org/cli", &pr, "merge", &head), Err("gh/head_changed".into()));
    // 405 / 422：按 MR 当前的 detailed_merge_status 说原因。
    for (detail, want) in [
        ("not_approved", "gh/review_required".to_string()),
        ("conflict", format!("gh/merge_conflict: {}", pr.url)),
        ("ci_still_running", "gh/checks_pending".to_string()),
        ("discussions_not_resolved", "gh/merge_blocked: discussions_not_resolved".to_string()),
    ] {
        *status.lock().unwrap() = (405, json!({ "message": "405 Method Not Allowed" }));
        *detailed.lock().unwrap() = detail.into();
        assert_eq!(gl.merge(".", "gitlab-org/cli", &pr, "merge", &head), Err(want), "{detail}");
    }
    *status.lock().unwrap() = (422, json!({ "message": "Branch cannot be merged" }));
    *detailed.lock().unwrap() = "need_rebase".into();
    assert_eq!(
        gl.merge(".", "gitlab-org/cli", &pr, "merge", &head),
        Err("gh/merge_blocked: need_rebase".into())
    );
    // 没有合并权限也回 401：令牌本身好用（/user 成功）就是权限问题。
    *status.lock().unwrap() = (401, json!({ "message": "401 Unauthorized" }));
    assert_eq!(
        gl.merge(".", "gitlab-org/cli", &pr, "merge", &head),
        Err("forge/forbidden: 401 Unauthorized".into())
    );
    // 回查 /user 不再回报令牌好坏（以前回查成功会紧跟着报一次「好用」，多读写一次清单、多发一次事件）。
    assert_eq!(flags.lock().unwrap().last(), Some(&false));
    // 令牌真的失效：/user 也 401 → 要重新登录。
    user_ok.store(0, Ordering::SeqCst);
    assert_eq!(gl.merge(".", "gitlab-org/cli", &pr, "merge", &head), Err("gh/not_authed".into()));
    assert_eq!(flags.lock().unwrap().last(), Some(&false), "401 回报给账号（标要重新登录）");
}

#[test]
fn gitlab_开_mr_草稿前缀_fork_带目标项目_已有时给链接() {
    let reply = std::sync::Arc::new(Mutex::new(resp(201, json!({ "web_url": "https://gitlab.com/o/r/-/merge_requests/8" }))));
    let r2 = reply.clone();
    let fake = Fake::new(move |method, url, _| match (method, url) {
        ("GET", "https://gitlab.com/api/v4/projects/o%2Fr") => resp(200, json!({ "id": 11 })),
        ("POST", _) => r2.lock().unwrap().clone(),
        _ => resp(404, json!({})),
    });
    let flags = Mutex::new(Vec::new());
    let gl = gitlab(&fake, &flags);
    let new = NewPr {
        title: "Fix",
        body: "- Fix",
        base: "main",
        head: "me:feat",
        branch: "feat",
        head_slug: Some("me/r"),
        draft: true,
    };
    assert_eq!(gl.create(".", Some("o/r"), &new), Ok("https://gitlab.com/o/r/-/merge_requests/8".into()));
    let (_, url, body, _) = fake.calls().into_iter().find(|(m, _, _, _)| m == "POST").unwrap();
    assert_eq!(url, "https://gitlab.com/api/v4/projects/me%2Fr/merge_requests", "fork 在源项目上建");
    assert_eq!(
        body,
        json!({ "source_branch": "feat", "target_branch": "main", "title": "Draft: Fix", "description": "- Fix", "target_project_id": 11 })
    );
    // 源码 app/models/merge_request.rb：已有打开的 MR 回 409，message 是数组。
    *reply.lock().unwrap() = resp(
        409,
        json!({ "message": ["Another open merge request already exists for this source branch: !7"] }),
    );
    let same = NewPr {
        head_slug: None,
        head: "feat",
        draft: false,
        ..new
    };
    assert_eq!(
        gl.create(".", Some("o/r"), &same),
        Err("gh/pr_exists: https://gitlab.com/o/r/-/merge_requests/7".into())
    );
}

#[test]
fn gitlab_评审_讨论批准改动文件_行内评论钉在_diff_refs() {
    // 同仓库的分支：把样例（来自 fork）的源项目改成目标项目。
    let mut mr = fixture("gitlab_mr_4365.json");
    mr["source_project_id"] = mr["target_project_id"].clone();
    // docs.gitlab.com/api/discussions 的示例形状：一条系统备注、一条普通评论、一条 DiffNote。
    let discussions = json!([
        { "id": "a", "notes": [{ "id": 1, "type": null, "body": "added 1 commit", "author": { "username": "bot" }, "created_at": "2026-11-01T00:00:00Z", "system": true }] },
        { "id": "b", "notes": [{ "id": 2, "type": "DiscussionNote", "body": "looks good", "author": { "username": "alice" }, "created_at": "2026-11-02T00:00:00Z", "system": false }] },
        { "id": "c", "notes": [{ "id": 3, "type": "DiffNote", "body": "nit", "author": { "username": "bob" }, "created_at": "2026-11-03T00:00:00Z", "system": false,
            "position": { "base_sha": "x", "start_sha": "y", "head_sha": "z", "old_path": "a.go", "new_path": "a.go", "position_type": "text", "old_line": null, "new_line": 12 } }] }
    ]);
    let mr2 = mr.clone();
    let fake = Fake::new(move |method, url, _| match (method, url) {
        ("GET", u) if u.contains("/merge_requests?source_branch=") => resp(200, json!([mr2.clone()])),
        ("GET", u) if u.ends_with("/merge_requests/4365") => resp(200, mr2.clone()),
        ("GET", u) if u.contains("/jobs") => resp(200, json!([])),
        ("GET", u) if u.contains("/discussions") => resp(200, discussions.clone()),
        ("GET", u) if u.ends_with("/approvals") => resp(200, fixture("gitlab_approvals_4365.json")),
        ("GET", u) if u.contains("/diffs") => resp(200, fixture("gitlab_diffs_4365.json")),
        ("POST", _) => resp(201, json!({})),
        _ => resp(404, json!({})),
    });
    let flags = Mutex::new(Vec::new());
    let gl = gitlab(&fake, &flags);
    let review = gl.review(".", "gitlab-org/cli", "feat/login-detect-ssh-host").unwrap().unwrap();
    assert_eq!(review.pr.number, 4365);
    assert_eq!(
        review.comments.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ["comment:2", "comment:3"],
        "系统备注不算评论"
    );
    let inline = &review.comments[1];
    assert_eq!(
        (inline.path.as_deref(), inline.line, inline.side.as_deref()),
        (Some("a.go"), Some(12), Some("RIGHT"))
    );
    assert!(inline.url.as_deref().unwrap().ends_with("/merge_requests/4365#note_3"));
    let approvals = fixture("gitlab_approvals_4365.json");
    assert_eq!(review.reviews.len(), approvals["approved_by"].as_array().unwrap().len());
    let files = fixture("gitlab_diffs_4365.json");
    assert_eq!(review.files.len(), files.as_array().unwrap().len());
    let (add, del) = gitlab::count_lines(files[0]["diff"].as_str().unwrap());
    assert_eq!((review.files[0].additions, review.files[0].deletions), (add, del));
    assert!(review.files.iter().all(|f| f.patch.is_some()));
    // 行内评论：position 的三个 sha 取自 MR 的 diff_refs，删除行只写 old_line。
    gl.line_comment(".", "gitlab-org/cli", 4365, "a.go", 7, "OLD", "why?").unwrap();
    let (_, url, body, _) = fake.calls().into_iter().rfind(|(m, _, _, _)| m == "POST").unwrap();
    assert_eq!(url, "https://gitlab.com/api/v4/projects/gitlab-org%2Fcli/merge_requests/4365/discussions");
    assert_eq!(body["position"]["base_sha"], mr["diff_refs"]["base_sha"]);
    assert_eq!(body["position"]["start_sha"], mr["diff_refs"]["start_sha"]);
    assert_eq!(body["position"]["head_sha"], mr["diff_refs"]["head_sha"]);
    assert_eq!(body["position"]["old_line"], 7);
    assert!(body["position"].get("new_line").is_none());
    // 审查指出：没改动的上下文行要同时带 old_line 与 new_line（docs.gitlab.com/api/discussions）。样例 diff 的第一个文件
    // `@@ -48,6 +48,13 @@`：新 48 是上下文（旧 48）、新 51 是新增行、块后的新 70 对旧 63。
    let post = |path: &str, line: i64, side: &str| {
        gl.line_comment(".", "gitlab-org/cli", 4365, path, line, side, "x").unwrap();
        let (_, _, body, _) = fake.calls().into_iter().rfind(|(m, _, _, _)| m == "POST").unwrap();
        (
            body["position"]["old_line"].as_i64(),
            body["position"]["new_line"].as_i64(),
            body["position"]["old_path"].clone(),
        )
    };
    let doc = "docs/source/auth/login.md";
    assert_eq!(post(doc, 48, "NEW"), (Some(48), Some(48), json!(doc)));
    assert_eq!(post(doc, 51, "NEW"), (None, Some(51), json!(doc)));
    assert_eq!(post(doc, 70, "NEW"), (Some(63), Some(70), json!(doc)));
    assert_eq!(post(doc, 10, "OLD"), (Some(10), Some(10), json!(doc)));
}

#[test]
fn gitlab_行内评论_由_diff_推出两侧行号() {
    let diff = "@@ -1,4 +1,5 @@
 a
-b
+B
+B2
 c
 d
@@ -20,3 +21,2 @@
 x
-y
 z
";
    assert_eq!(gitlab::diff_position(diff, "NEW", 1), (Some(1), Some(1)), "上下文行两侧都有");
    assert_eq!(gitlab::diff_position(diff, "NEW", 2), (None, Some(2)), "新增行只有新");
    assert_eq!(gitlab::diff_position(diff, "OLD", 2), (Some(2), None), "删除行只有旧");
    assert_eq!(gitlab::diff_position(diff, "NEW", 4), (Some(3), Some(4)));
    assert_eq!(gitlab::diff_position(diff, "NEW", 10), (Some(9), Some(10)), "块之间没改动的行按累计偏移");
    assert_eq!(gitlab::diff_position(diff, "OLD", 21), (Some(21), None));
    assert_eq!(gitlab::diff_position(diff, "NEW", 30), (Some(30), Some(30)), "第二块删一加零后偏移回到 0");
}

/// 重命名的文件：old_path 用旧路径。
#[test]
fn gitlab_行内评论_重命名文件用旧路径() {
    let mr = fixture("gitlab_mr_4365.json");
    let fake = Fake::new(move |method, url, _| match (method, url) {
        ("GET", u) if u.ends_with("/merge_requests/4365") => resp(200, mr.clone()),
        ("GET", u) if u.contains("/diffs") => resp(
            200,
            json!([{ "old_path": "src/old.go", "new_path": "src/new.go", "renamed_file": true, "diff": "@@ -1,2 +1,2 @@
 a
-b
+c
" }]),
        ),
        ("POST", _) => resp(201, json!({})),
        _ => resp(404, json!({})),
    });
    let flags = Mutex::new(Vec::new());
    gitlab(&fake, &flags)
        .line_comment(".", "gitlab-org/cli", 4365, "src/new.go", 1, "NEW", "x")
        .unwrap();
    let (_, _, body, _) = fake.calls().into_iter().rfind(|(m, _, _, _)| m == "POST").unwrap();
    assert_eq!(
        (body["position"]["old_path"].clone(), body["position"]["new_path"].clone()),
        (json!("src/old.go"), json!("src/new.go"))
    );
    assert_eq!(
        (body["position"]["old_line"].as_i64(), body["position"]["new_line"].as_i64()),
        (Some(1), Some(1))
    );
}

#[test]
fn gitlab_401_标要重新登录() {
    let fake = Fake::new(|_, _, _| resp(401, fixture("gitlab_user_401.json")));
    let flags = Mutex::new(Vec::new());
    let err = gitlab(&fake, &flags).find(".", Some("o/r"), &branch("x")).unwrap_err();
    assert_eq!(err, "gh/not_authed");
    assert_eq!(*flags.lock().unwrap(), vec![false]);
}

// ═══ Gitea / Forgejo ═══

#[test]
fn gitea_pr_映射_相对链接拼主机_草稿不当冲突() {
    let checks = gitea::map_statuses("codeberg.org", &fixture("gitea_status_6b42eac1.json"));
    assert!(!checks.is_empty());
    assert!(
        checks.iter().all(|c| c
            .url
            .as_deref()
            .is_none_or(|u| u.starts_with("https://codeberg.org/forgejo/forgejo/actions/runs/"))),
        "{checks:?}"
    );
    assert!(checks.iter().any(|c| c.state == "skipped"));
    let pr = gitea::map_pull(&fixture("gitea_pull_15303.json"), checks).unwrap();
    assert_eq!((pr.number, pr.state.as_str(), pr.forge.as_deref()), (15303, "open", Some("gitea")));
    assert_eq!(pr.url, "https://codeberg.org/forgejo/forgejo/pulls/15303");
    assert_eq!(
        (pr.base_ref.as_deref(), pr.head_ref.as_deref(), pr.head_oid.as_deref()),
        (
            Some("v15.0/forgejo"),
            Some("v15-backport-15270"),
            Some("b9160301251f5115810a5083975dd91077fbd3d5")
        )
    );
    assert_eq!((pr.mergeable.as_deref(), pr.merge_state.as_deref()), (Some("MERGEABLE"), Some("CLEAN")));
    assert_eq!(pr.checks_summary.as_deref(), Some("pass"));
    // 标题 WIP: → 草稿；Codeberg 这时 mergeable 也是 false，不当冲突。
    let wip = gitea::map_pull(&fixture("gitea_pull_15307_wip.json"), Vec::new()).unwrap();
    assert_eq!(
        (wip.state.as_str(), wip.mergeable.as_deref(), wip.merge_state.as_deref()),
        ("draft", Some("MERGEABLE"), Some("DRAFT"))
    );
    // mergeable=false 也可能是服务端还在检查（Gitea models/issues/pull.go Mergeable()：Checking、Conflict、Error、WIP 都回
    // false，API 不给 status）。区分不了就当「还不知道」，不当冲突（审查指出）；真有冲突时合并会回 409 再归类。
    let mut checking = fixture("gitea_pull_15303.json");
    checking["mergeable"] = false.into();
    let pr = gitea::map_pull(&checking, Vec::new()).unwrap();
    assert_eq!((pr.mergeable.as_deref(), pr.merge_state.as_deref()), (Some("UNKNOWN"), Some("UNKNOWN")));
    // 状态取值（Forgejo swagger 的 CommitStatusState）。
    for (s, want) in [
        ("success", "pass"),
        ("warning", "pass"),
        ("failure", "fail"),
        ("error", "fail"),
        ("pending", "pending"),
        ("skipped", "skipped"),
    ] {
        assert_eq!(gitea::status_state(s), want, "{s}");
    }
}

#[test]
fn gitea_按分支找_fork_的_pr_先看打开的() {
    let fake = Fake::new(|_, url, _| match url {
        u if u.starts_with("https://codeberg.org/api/v1/repos/forgejo/forgejo/pulls?state=open&head=v15-backport-15270") => {
            resp(200, fixture("gitea_pulls_open_head.json"))
        }
        u if u.starts_with("https://codeberg.org/api/v1/repos/forgejo/forgejo/pulls?state=closed") => resp(200, json!([])),
        u if u.contains("/commits/b9160301251f5115810a5083975dd91077fbd3d5/status") => resp(200, fixture("gitea_status_6b42eac1.json")),
        _ => resp(404, json!({ "message": "not found" })),
    });
    let flags = Mutex::new(Vec::new());
    let ge = gitea(&fake, &flags);
    let target = Target::Branch {
        qualified: "mfenniak:v15-backport-15270",
        branch: "v15-backport-15270",
        head_slug: Some("mfenniak/forgejo"),
    };
    let pr = ge.find(".", Some("forgejo/forgejo"), &target).unwrap().unwrap();
    assert_eq!(pr.number, 15303);
    assert_eq!(fake.calls().len(), 2, "打开的列表 + 头提交的状态：{:?}", fake.urls());
    assert!(fake.calls().iter().all(|(_, _, _, auth)| auth.as_deref() == Some("token glpat-TEST")));
    // 同仓库同名分支没有：打开的、最近关闭的都看过，没有。
    assert_eq!(ge.find(".", Some("forgejo/forgejo"), &branch("v15-backport-15270")).unwrap(), None);
}

#[test]
fn gitea_合并_钉住头提交_拒绝原话归类() {
    let pr = gitea::map_pull(&fixture("gitea_pull_15303.json"), Vec::new()).unwrap();
    let reply = std::sync::Arc::new(Mutex::new(resp(200, json!({}))));
    let r2 = reply.clone();
    let fake = Fake::new(move |_, _, _| r2.lock().unwrap().clone());
    let flags = Mutex::new(Vec::new());
    let ge = gitea(&fake, &flags);
    assert_eq!(ge.merge(".", "forgejo/forgejo", &pr, "squash", "b916"), Ok(false));
    let (_, url, body, _) = fake.calls().pop().unwrap();
    assert_eq!(url, "https://codeberg.org/api/v1/repos/forgejo/forgejo/pulls/15303/merge");
    assert_eq!(
        body,
        json!({ "Do": "squash", "head_commit_id": "b916", "delete_branch_after_merge": false })
    );
    let cases = [
        (409, "head out of date", "gh/head_changed".to_string()),
        (
            405,
            "squash is not allowed an allowed merge style for this repository",
            "gh/method_not_allowed".to_string(),
        ),
        (405, "Work in progress PRs cannot be merged", "gh/merge_blocked: draft".to_string()),
        // Gitea main services/pull/check.go：冲突、检查中、出错都回 ErrNotMergeableState，API 统一回 405「Please try again
        // later」——分不清，不能说成「检查在跑」（复核指出），给专门的码并带上 PR 链接。
        (405, "Please try again later", format!("forge/not_mergeable: {}", pr.url)),
        (
            405,
            "User not allowed to merge PR",
            "forge/forbidden: User not allowed to merge PR".to_string(),
        ),
        (409, "Merge Conflict", format!("gh/merge_conflict: {}", pr.url)),
        (423, "repo is archived", "gh/merge_blocked: repo is archived".to_string()),
    ];
    for (code, message, want) in cases {
        *reply.lock().unwrap() = resp(code, json!({ "message": message, "url": "https://codeberg.org/api/swagger" }));
        assert_eq!(ge.merge(".", "forgejo/forgejo", &pr, "squash", "b916"), Err(want), "{code} {message}");
    }
    *reply.lock().unwrap() = resp(405, json!({ "message": "The PR is already merged" }));
    assert_eq!(ge.merge(".", "forgejo/forgejo", &pr, "squash", "b916"), Ok(false), "已经合了按成功");
}

#[test]
fn gitea_合并方式_开_pr_已存在给链接() {
    let fake = Fake::new(|method, url, _| match (method, url) {
        ("GET", "https://codeberg.org/api/v1/repos/forgejo/forgejo") => resp(200, fixture("gitea_repo.json")),
        ("POST", _) => resp(
            409,
            json!({ "message": "pull request already exists for these targets [id: 1, issue_id: 42, head_repo_id: 2, base_repo_id: 2, head_branch: feat, base_branch: forgejo]" }),
        ),
        _ => resp(404, json!({})),
    });
    let flags = Mutex::new(Vec::new());
    let ge = gitea(&fake, &flags);
    assert_eq!(ge.merge_methods(".", "forgejo/forgejo").unwrap(), vec!["merge", "squash", "rebase"]);
    let new = NewPr {
        title: "Fix",
        body: "",
        base: "forgejo",
        head: "feat",
        branch: "feat",
        head_slug: None,
        draft: true,
    };
    assert_eq!(
        ge.create(".", Some("forgejo/forgejo"), &new),
        Err("gh/pr_exists: https://codeberg.org/forgejo/forgejo/pulls/42".into())
    );
    let (_, _, body, _) = fake.calls().into_iter().find(|(m, _, _, _)| m == "POST").unwrap();
    assert_eq!(body["title"], "WIP: Fix", "草稿用 WIP: 前缀");
}

#[test]
fn gitea_评审_请求评审不算结论_行内评论带文件行() {
    let reviews = gitea::map_reviews(fixture("gitea_reviews_15271.json").as_array().unwrap());
    assert_eq!(
        reviews.iter().map(|r| r.state.as_deref().unwrap()).collect::<Vec<_>>(),
        ["COMMENT", "APPROVED"]
    );
    assert!(reviews.iter().all(|r| r.id.starts_with("review:")));
    let inline = gitea::map_comments(fixture("gitea_review_comments_2303344.json").as_array().unwrap());
    assert_eq!(inline.len(), 2);
    assert_eq!(
        (inline[0].path.as_deref(), inline[0].line, inline[0].side.as_deref()),
        (Some("templates/repo/diff/range_diff.tmpl"), Some(65), Some("RIGHT"))
    );
    let issue = gitea::map_comments(fixture("gitea_issue_comments_15271.json").as_array().unwrap());
    assert!(issue
        .iter()
        .all(|c| c.path.is_none() && c.line.is_none() && c.url.as_deref().is_some_and(|u| u.contains("#issuecomment-"))));
    let files = gitea::map_files(fixture("gitea_files_15303.json").as_array().unwrap());
    assert_eq!(
        files
            .iter()
            .map(|f| (f.path.as_str(), f.status.as_str(), f.additions, f.deletions))
            .collect::<Vec<_>>(),
        [
            ("services/actions/task.go", "modified", 1, 1),
            ("services/actions/task_test.go", "modified", 36, 0)
        ]
    );
}

// ═══ Bitbucket Cloud ═══

#[test]
fn bitbucket_pr_映射_短_hash_草稿_构建失败() {
    let statuses = fixture("bitbucket_statuses_604.json");
    let pr = bitbucket::map_pr(
        &fixture("bitbucket_pr_604.json"),
        bitbucket::map_statuses(statuses["values"].as_array().unwrap()),
    )
    .unwrap();
    assert_eq!((pr.number, pr.state.as_str(), pr.forge.as_deref()), (604, "open", Some("bitbucket")));
    assert_eq!(pr.url, "https://bitbucket.org/atlassian/atlassian-connect-express/pull-requests/604");
    assert_eq!(
        (pr.head_ref.as_deref(), pr.head_oid.as_deref(), pr.base_ref.as_deref()),
        (Some("renovate/prettier-3.x"), Some("e0949d3373fd"), Some("master"))
    );
    assert_eq!((pr.checks.len(), pr.checks_summary.as_deref()), (3, Some("pass")));
    let statuses = fixture("bitbucket_statuses_552.json");
    let draft = bitbucket::map_pr(
        &fixture("bitbucket_pr_552_draft.json"),
        bitbucket::map_statuses(statuses["values"].as_array().unwrap()),
    )
    .unwrap();
    assert_eq!(
        (draft.state.as_str(), draft.merge_state.as_deref(), draft.checks_summary.as_deref()),
        ("draft", Some("DRAFT"), Some("fail"))
    );
    for (s, want) in [("SUCCESSFUL", "pass"), ("FAILED", "fail"), ("STOPPED", "fail"), ("INPROGRESS", "pending")] {
        assert_eq!(bitbucket::status_state(s), want);
    }
    // 评审结论来自参与者：#604 有一人批准。
    let reviews = bitbucket::map_reviews(&fixture("bitbucket_pr_604.json"));
    assert_eq!(reviews.iter().map(|r| r.state.as_deref().unwrap()).collect::<Vec<_>>(), ["APPROVED"]);
}

#[test]
fn bitbucket_按分支找_打开的优先_只取要的字段() {
    let fake = Fake::new(|_, url, _| match url {
        u if u.starts_with("https://api.bitbucket.org/2.0/repositories/atlassian/atlassian-connect-express/pullrequests?q=") => {
            resp(200, fixture("bitbucket_prs_branch.json"))
        }
        u if u.contains("/pullrequests/604/statuses") => resp(200, fixture("bitbucket_statuses_604.json")),
        _ => resp(404, json!({ "type": "error", "error": { "message": "not found" } })),
    });
    let flags = Mutex::new(Vec::new());
    let bb = bitbucket(&fake, &flags);
    let pr = bb
        .find(".", Some("atlassian/atlassian-connect-express"), &branch("renovate/prettier-3.x"))
        .unwrap()
        .unwrap();
    assert_eq!((pr.number, pr.state.as_str()), (604, "open"));
    let calls = fake.calls();
    assert_eq!(calls.len(), 2, "{:?}", fake.urls());
    let list = &calls[0].1;
    assert!(list.contains("q=source.branch.name%3D%22renovate%2Fprettier-3.x%22"), "{list}");
    assert!(list.contains("&fields=") && list.contains("&state=DECLINED"), "{list}");
    assert_eq!(
        calls[0].3.as_deref(),
        Some("Basic bWVAZXhhbXBsZS5jb206Z2xwYXQtVEVTVA=="),
        "API token：邮箱 + 令牌的 Basic 认证"
    );
}

#[test]
fn bitbucket_合并_没有_sha_参数_先核对头提交与允许的方式() {
    let pr = bitbucket::map_pr(&fixture("bitbucket_pr_604.json"), Vec::new()).unwrap();
    let reply = std::sync::Arc::new(Mutex::new(resp(200, json!({}))));
    let r2 = reply.clone();
    let fake = Fake::new(move |method, url, _| match (method, url) {
        ("GET", u) if u.ends_with("/pullrequests/604") => {
            let mut fresh = fixture("bitbucket_pr_604.json");
            fresh["destination"]["branch"]["merge_strategies"] = json!(["merge_commit", "squash"]);
            resp(200, fresh)
        }
        ("POST", _) => r2.lock().unwrap().clone(),
        _ => resp(404, json!({})),
    });
    let flags = Mutex::new(Vec::new());
    let bb = bitbucket(&fake, &flags);
    assert_eq!(
        bb.merge(".", "atlassian/atlassian-connect-express", &pr, "rebase", "e0949d3373fd"),
        Err("gh/method_not_allowed".into())
    );
    assert_eq!(
        bb.merge(".", "atlassian/atlassian-connect-express", &pr, "squash", "0000000"),
        Err("gh/head_changed".into())
    );
    assert!(!fake.calls().iter().any(|(m, _, _, _)| m == "POST"), "核对没过就不发合并");
    assert_eq!(
        bb.merge(".", "atlassian/atlassian-connect-express", &pr, "squash", "e0949d3373fd"),
        Ok(false)
    );
    let (_, url, body, _) = fake.calls().into_iter().find(|(m, _, _, _)| m == "POST").unwrap();
    assert_eq!(
        url,
        "https://api.bitbucket.org/2.0/repositories/atlassian/atlassian-connect-express/pullrequests/604/merge"
    );
    assert_eq!(body, json!({ "merge_strategy": "squash", "close_source_branch": false }));
    *reply.lock().unwrap() = resp(202, json!({}));
    assert_eq!(
        bb.merge(".", "atlassian/atlassian-connect-express", &pr, "merge", "e0949d3373fd"),
        Ok(true),
        "202 = 转成异步或进了队列"
    );
    *reply.lock().unwrap() = resp(
        400,
        json!({ "type": "error", "error": { "message": "You can't merge until you resolve all merge conflicts." } }),
    );
    assert_eq!(
        bb.merge(".", "atlassian/atlassian-connect-express", &pr, "merge", "e0949d3373fd"),
        Err(format!("gh/merge_conflict: {}", pr.url))
    );
    *reply.lock().unwrap() = resp(409, json!({}));
    assert_eq!(
        bb.merge(".", "atlassian/atlassian-connect-express", &pr, "merge", "e0949d3373fd"),
        Err("gh/head_changed".into())
    );
}

#[test]
fn bitbucket_开_pr_已有打开的直接给链接_评论与改动文件() {
    let fake = Fake::new(|method, url, _| match (method, url) {
        ("GET", u) if u.contains("/pullrequests?q=") => resp(200, fixture("bitbucket_prs_branch.json")),
        _ => resp(500, json!({})),
    });
    let flags = Mutex::new(Vec::new());
    let bb = bitbucket(&fake, &flags);
    let new = NewPr {
        title: "x",
        body: "",
        base: "master",
        head: "renovate/prettier-3.x",
        branch: "renovate/prettier-3.x",
        head_slug: None,
        draft: false,
    };
    assert_eq!(
        bb.create(".", Some("atlassian/atlassian-connect-express"), &new),
        Err("gh/pr_exists: https://bitbucket.org/atlassian/atlassian-connect-express/pull-requests/604".into())
    );
    assert!(fake.calls().iter().all(|(m, _, _, _)| m == "GET"), "已有就不再建");
    let comments = bitbucket::map_comments(fixture("bitbucket_comments_552.json")["values"].as_array().unwrap());
    assert_eq!(
        (comments[0].path.as_deref(), comments[0].line, comments[0].side.as_deref()),
        (Some("lib/store/redis.js"), Some(99), Some("RIGHT"))
    );
    assert!(comments.iter().any(|c| c.path.is_none()), "普通评论没有文件行");
    let files = bitbucket::map_files(fixture("bitbucket_diffstat_604.json")["values"].as_array().unwrap());
    assert_eq!(
        files
            .iter()
            .map(|f| (f.path.as_str(), f.status.as_str(), f.additions, f.deletions))
            .collect::<Vec<_>>(),
        [("package.json", "modified", 1, 1)]
    );
}

// ═══ GitHub 企业版 ═══

#[test]
fn 企业版_内置实现走_api_v3_与_api_graphql_只发往这台主机() {
    let fake = Fake::new(|method, url, body| match (method, url) {
        ("GET", "https://ghe.example.com/api/v3/user") => resp(200, json!({ "login": "octo", "name": "Octo Cat" })),
        ("POST", "https://ghe.example.com/api/graphql") => {
            assert!(body["query"].as_str().unwrap().contains("mergeCommitAllowed"));
            resp(
                200,
                json!({ "data": { "repository": { "mergeCommitAllowed": true, "squashMergeAllowed": false, "rebaseMergeAllowed": true } } }),
            )
        }
        _ => resp(404, json!({ "message": "Not Found" })),
    });
    assert_eq!(
        super::super::ghe::current_user(&fake, "ghe.example.com", &token()),
        Ok(("octo".into(), Some("Octo Cat".into())))
    );
    let flags = Mutex::new(Vec::new());
    let forge = super::super::ghe::forge(
        Box::new(&fake),
        "ghe.example.com",
        Some(token()),
        GhState::Ok,
        std::env::temp_dir().join("meowo-ghe-forge.json"),
        Box::new(|ok| flags.lock().unwrap().push(ok)),
    );
    assert_eq!(forge.state(false), GhState::Ok);
    assert!(!forge.too_old(), "企业版走内置实现，总能钉住头提交");
    assert_eq!(forge.merge_methods(".", "team/app").unwrap(), vec!["merge", "rebase"]);
    let calls = fake.calls();
    assert!(
        calls
            .iter()
            .all(|(_, url, _, auth)| url.starts_with("https://ghe.example.com/api/") && auth.as_deref() == Some("Bearer glpat-TEST")),
        "{:?}",
        fake.urls()
    );
    assert_eq!(*flags.lock().unwrap(), vec![true], "成功的回包回报令牌好用");
}

// ═══ 只读真机核对（M7）：默认跳过，手动跑 ═══
//
// `cargo test -p meowo-app --lib live_只读_各平台 -- --ignored --nocapture`
// 不带任何令牌，只发 GET：用本实现对 gitlab.com、codeberg.org、bitbucket.org、gitee.com 的公开仓库按链接与按分支查 PR/MR
// 与检查，再直接取官方 API 的原始响应逐项对照（号、状态、标题、头提交、检查条数与各项状态）。

fn live_client(host: &str) -> Client<'static> {
    Client {
        http: Box::new(super::super::http::RealForgeHttp { host: host.into() }),
        auth: None,
        on_auth: Box::new(|_| {}),
    }
}

fn live_raw(host: &str, url: &str) -> Value {
    for attempt in 0..6 {
        match live_client(host).get(url) {
            Ok(v) => return v,
            Err(e) if attempt < 5 => eprintln!("  重试 {url}: {e}"),
            Err(e) => panic!("{url}: {e}"),
        }
    }
    unreachable!()
}

fn retry<T>(what: &str, f: impl Fn() -> Result<T, String>) -> T {
    for attempt in 0..6 {
        match f() {
            Ok(v) => return v,
            Err(e) if attempt < 5 && (e.starts_with("forge/network") || e.starts_with("forge/tls")) => eprintln!("  重试 {what}: {e}"),
            Err(e) => panic!("{what}: {e}"),
        }
    }
    unreachable!()
}

fn show(label: &str, pr: &meowo_protocol::ipc::PullRequestDto) {
    let states: Vec<String> = pr.checks.iter().map(|c| format!("{}={}", c.name, c.state)).collect();
    eprintln!(
        "{label}: #{} {} head={} base={:?} head_ref={:?} mergeable={:?}/{:?} checks={} [{}] {}",
        pr.number,
        pr.state,
        pr.head_oid.as_deref().unwrap_or("-"),
        pr.base_ref,
        pr.head_ref,
        pr.mergeable,
        pr.merge_state,
        pr.checks_summary.as_deref().unwrap_or("-"),
        states.join(", "),
        pr.url
    );
}

// 真机核对的期望值：独立写死的对照表（审查指出不能拿实现自己的映射函数自证）。左边是平台原始状态，右边是界面
// 应显示的结论，依据各平台文档对状态的定义：GitLab job（允许失败的失败、手动的不挡合并）、Gitea commit status
//（warning 视为通过）、Bitbucket build status（STOPPED 不算通过）。表里没有的原始值直接让核对失败，不去猜。
const GITLAB_EXPECT: &[(&str, bool, &str)] = &[
    ("success", false, "pass"),
    ("success", true, "pass"),
    ("failed", false, "fail"),
    ("failed", true, "skipped"),
    ("canceled", false, "fail"),
    ("canceled", true, "fail"),
    ("skipped", false, "skipped"),
    ("skipped", true, "skipped"),
    ("manual", true, "skipped"),
    ("manual", false, "pending"),
    ("created", false, "pending"),
    ("pending", false, "pending"),
    ("running", false, "pending"),
    ("running", true, "pending"),
];
const GITEA_EXPECT: &[(&str, &str)] = &[
    ("success", "pass"),
    ("warning", "pass"),
    ("failure", "fail"),
    ("error", "fail"),
    ("pending", "pending"),
    ("skipped", "skipped"),
];
const BITBUCKET_EXPECT: &[(&str, &str)] = &[("SUCCESSFUL", "pass"), ("FAILED", "fail"), ("STOPPED", "fail"), ("INPROGRESS", "pending")];

fn expect_gitlab(status: &str, allow_failure: bool) -> &'static str {
    GITLAB_EXPECT
        .iter()
        .find(|(s, a, _)| *s == status && *a == allow_failure)
        .map(|(_, _, w)| *w)
        .unwrap_or_else(|| panic!("对照表里没有 {status}/{allow_failure}"))
}
fn expect_gitea(status: &str) -> &'static str {
    GITEA_EXPECT
        .iter()
        .find(|(s, _)| *s == status)
        .map(|(_, w)| *w)
        .unwrap_or_else(|| panic!("对照表里没有 {status}"))
}
/// Gitee check-run：与 GitHub 的 CheckRun 同义（官方 Swagger 的 status / conclusion 枚举）。
const GITEE_RUN_EXPECT: &[(&str, &str, &str)] = &[
    ("queued", "", "pending"),
    ("in_progress", "", "pending"),
    ("completed", "success", "pass"),
    ("completed", "neutral", "pass"),
    ("completed", "skipped", "skipped"),
    ("completed", "failure", "fail"),
    ("completed", "cancelled", "fail"),
    ("completed", "timed_out", "fail"),
    ("completed", "action_required", "fail"),
];
fn expect_gitee_run(status: &str, conclusion: &str) -> &'static str {
    let conclusion = if status == "completed" { conclusion } else { "" };
    GITEE_RUN_EXPECT
        .iter()
        .find(|(s, c, _)| *s == status && *c == conclusion)
        .map(|(_, _, w)| *w)
        .unwrap_or_else(|| panic!("对照表里没有 {status}/{conclusion}"))
}
fn expect_bitbucket(state: &str) -> &'static str {
    BITBUCKET_EXPECT
        .iter()
        .find(|(s, _)| *s == state)
        .map(|(_, w)| *w)
        .unwrap_or_else(|| panic!("对照表里没有 {state}"))
}

#[test]
#[ignore]
fn live_只读_各平台() {
    // GitLab：gitlab-org/cli 最近更新的、同仓库分支的打开 MR。
    let opened = live_raw(
        "gitlab.com",
        "https://gitlab.com/api/v4/projects/gitlab-org%2Fcli/merge_requests?state=opened&per_page=20&order_by=updated_at",
    );
    let mr = opened
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["source_project_id"] == m["target_project_id"])
        .expect("有同仓库分支的打开 MR")
        .clone();
    let iid = mr["iid"].as_u64().unwrap() as u32;
    let gl = Gitlab {
        client: live_client("gitlab.com"),
        host: "gitlab.com".into(),
        state: GhState::Ok,
        login: None,
    };
    let by_branch = retry("gitlab 按分支", || {
        gl.find(".", Some("gitlab-org/cli"), &branch(mr["source_branch"].as_str().unwrap()))
    })
    .expect("按分支找到");
    let by_url = retry("gitlab 按链接", || gl.find_url(".", mr["web_url"].as_str().unwrap())).expect("按链接找到");
    show("gitlab 按分支", &by_branch);
    show("gitlab 按链接", &by_url);
    let raw = live_raw(
        "gitlab.com",
        &format!("https://gitlab.com/api/v4/projects/gitlab-org%2Fcli/merge_requests/{iid}"),
    );
    eprintln!(
        "  原始：!{} state={} draft={} sha={} detailed_merge_status={}",
        raw["iid"], raw["state"], raw["draft"], raw["sha"], raw["detailed_merge_status"]
    );
    assert_eq!((by_branch.number, by_url.number), (iid, iid));
    assert_eq!(by_branch.head_oid.as_deref(), raw["sha"].as_str());
    assert_eq!(by_branch.title, raw["title"].as_str().unwrap());
    assert_eq!(by_branch.state, if raw["draft"].as_bool().unwrap() { "draft" } else { "open" });
    if let Some(p) = raw["head_pipeline"].as_object() {
        let jobs = live_raw(
            "gitlab.com",
            &format!(
                "https://gitlab.com/api/v4/projects/{}/pipelines/{}/jobs?per_page=100",
                p["project_id"], p["id"]
            ),
        );
        let jobs = jobs.as_array().unwrap();
        let listing: Vec<String> = jobs
            .iter()
            .map(|j| format!("{}={}", j["name"].as_str().unwrap(), j["status"].as_str().unwrap()))
            .collect();
        eprintln!(
            "  原始：pipeline {} status={} jobs={} [{}]",
            p["id"],
            p["status"],
            jobs.len(),
            listing.join(", ")
        );
        assert_eq!(by_branch.checks.len(), jobs.len().max(1));
        for (check, job) in by_branch.checks.iter().zip(jobs) {
            assert_eq!(check.name, job["name"].as_str().unwrap());
            assert_eq!(
                check.state,
                expect_gitlab(job["status"].as_str().unwrap(), job["allow_failure"].as_bool().unwrap_or(false)),
                "{job}"
            );
        }
    } else {
        eprintln!("  原始：没有 head_pipeline");
        assert!(by_branch.checks.is_empty());
    }

    // Codeberg：forgejo/forgejo 最近更新的打开 PR（fork 来的按 owner:分支）。
    let pulls = live_raw(
        "codeberg.org",
        "https://codeberg.org/api/v1/repos/forgejo/forgejo/pulls?state=open&limit=10&sort=recentupdate",
    );
    // 挑一个头提交上有检查的（没有就退回第一个）。
    let candidates: Vec<Value> = pulls
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["head"]["repo"].is_object())
        .cloned()
        .collect();
    let has_checks = |p: &Value| {
        let url = format!(
            "https://codeberg.org/api/v1/repos/forgejo/forgejo/commits/{}/status?limit=100",
            p["head"]["sha"].as_str().unwrap_or_default()
        );
        live_raw("codeberg.org", &url)["statuses"].as_array().is_some_and(|s| !s.is_empty())
    };
    let pull = candidates
        .iter()
        .find(|p| has_checks(p))
        .or(candidates.first())
        .expect("有打开的 PR")
        .clone();
    let number = pull["number"].as_u64().unwrap() as u32;
    let head_repo = pull["head"]["repo"]["full_name"].as_str().unwrap().to_string();
    let head_branch = pull["head"]["ref"].as_str().unwrap().to_string();
    let ge = Gitea {
        client: live_client("codeberg.org"),
        host: "codeberg.org".into(),
        state: GhState::Ok,
        login: None,
    };
    let head_slug = (head_repo != "forgejo/forgejo").then_some(head_repo.as_str());
    let target = Target::Branch {
        qualified: &head_branch,
        branch: &head_branch,
        head_slug,
    };
    let by_branch = retry("codeberg 按分支", || ge.find(".", Some("forgejo/forgejo"), &target)).expect("按分支找到");
    let by_url = retry("codeberg 按链接", || ge.find_url(".", pull["html_url"].as_str().unwrap())).expect("按链接找到");
    show("codeberg 按分支", &by_branch);
    show("codeberg 按链接", &by_url);
    let raw = live_raw(
        "codeberg.org",
        &format!("https://codeberg.org/api/v1/repos/forgejo/forgejo/pulls/{number}"),
    );
    eprintln!(
        "  原始：#{} state={} draft={} mergeable={} head={} ({})",
        raw["number"], raw["state"], raw["draft"], raw["mergeable"], raw["head"]["sha"], head_repo
    );
    assert_eq!((by_branch.number, by_url.number), (number, number));
    assert_eq!(by_branch.head_oid.as_deref(), raw["head"]["sha"].as_str());
    let status = live_raw(
        "codeberg.org",
        &format!(
            "https://codeberg.org/api/v1/repos/forgejo/forgejo/commits/{}/status?limit=100",
            raw["head"]["sha"].as_str().unwrap()
        ),
    );
    let statuses = status["statuses"].as_array().cloned().unwrap_or_default();
    let statuses = &statuses;
    eprintln!("  原始：combined state={} statuses={}", status["state"], statuses.len());
    assert_eq!(by_branch.checks.len(), statuses.len());
    for (check, st) in by_branch.checks.iter().zip(statuses) {
        assert_eq!(check.name, st["context"].as_str().unwrap());
        assert_eq!(check.state, expect_gitea(st["status"].as_str().unwrap()), "{st}");
    }

    // Bitbucket：atlassian/atlassian-connect-express 最近的打开 PR（同仓库分支）。
    let list = live_raw(
        "api.bitbucket.org",
        "https://api.bitbucket.org/2.0/repositories/atlassian/atlassian-connect-express/pullrequests?state=OPEN&pagelen=5&fields=values.id,values.source.branch.name,values.source.repository.full_name,values.links.html.href",
    );
    let item = list["values"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["source"]["repository"]["full_name"] == "atlassian/atlassian-connect-express")
        .expect("有打开的 PR")
        .clone();
    let id = item["id"].as_u64().unwrap() as u32;
    let bb = Bitbucket {
        client: live_client(bitbucket::API_HOST),
        state: GhState::Ok,
        login: None,
    };
    let by_branch = retry("bitbucket 按分支", || {
        bb.find(
            ".",
            Some("atlassian/atlassian-connect-express"),
            &branch(item["source"]["branch"]["name"].as_str().unwrap()),
        )
    })
    .expect("按分支找到");
    let by_url = retry("bitbucket 按链接", || {
        bb.find_url(".", item["links"]["html"]["href"].as_str().unwrap())
    })
    .expect("按链接找到");
    show("bitbucket 按分支", &by_branch);
    show("bitbucket 按链接", &by_url);
    let raw = live_raw("api.bitbucket.org", &format!("https://api.bitbucket.org/2.0/repositories/atlassian/atlassian-connect-express/pullrequests/{id}?fields=id,state,draft,title,source.commit.hash"));
    eprintln!(
        "  原始：#{} state={} draft={} hash={}",
        raw["id"], raw["state"], raw["draft"], raw["source"]["commit"]["hash"]
    );
    assert_eq!((by_branch.number, by_url.number), (id, id));
    assert_eq!(by_branch.head_oid.as_deref(), raw["source"]["commit"]["hash"].as_str());
    assert_eq!(by_branch.title, raw["title"].as_str().unwrap());
    let statuses = live_raw(
        "api.bitbucket.org",
        &format!("https://api.bitbucket.org/2.0/repositories/atlassian/atlassian-connect-express/pullrequests/{id}/statuses?pagelen=100"),
    );
    let statuses = statuses["values"].as_array().unwrap();
    let listing: Vec<String> = statuses
        .iter()
        .map(|s| format!("{}={}", s["name"].as_str().unwrap_or(""), s["state"].as_str().unwrap_or("")))
        .collect();
    eprintln!("  原始：statuses={} [{}]", statuses.len(), listing.join(", "));
    assert_eq!(by_branch.checks.len(), statuses.len());
    for (check, st) in by_branch.checks.iter().zip(statuses) {
        assert_eq!(check.state, expect_bitbucket(st["state"].as_str().unwrap()), "{st}");
    }

    // Gitee：openharmony/docs 最近更新的打开 PR（fork 来的按 head 仓库区分）。
    let pulls = live_raw(
        "gitee.com",
        "https://gitee.com/api/v5/repos/openharmony/docs/pulls?state=open&sort=updated&direction=desc&per_page=5",
    );
    let pull = pulls.as_array().unwrap().first().expect("有打开的 PR").clone();
    let number = pull["number"].as_u64().unwrap() as u32;
    let head_repo = pull["head"]["repo"]["full_name"].as_str().unwrap().to_string();
    let head_branch = pull["head"]["ref"].as_str().unwrap().to_string();
    let ge = super::super::gitee::Gitee {
        client: live_client("gitee.com"),
        state: GhState::Ok,
        login: None,
    };
    let head_slug = (head_repo != "openharmony/docs").then_some(head_repo.as_str());
    let target = Target::Branch {
        qualified: &head_branch,
        branch: &head_branch,
        head_slug,
    };
    let by_branch = retry("gitee 按分支", || ge.find(".", Some("openharmony/docs"), &target)).expect("按分支找到");
    let by_url = retry("gitee 按链接", || ge.find_url(".", pull["html_url"].as_str().unwrap())).expect("按链接找到");
    show("gitee 按分支", &by_branch);
    show("gitee 按链接", &by_url);
    let raw = live_raw("gitee.com", &format!("https://gitee.com/api/v5/repos/openharmony/docs/pulls/{number}"));
    eprintln!(
        "  原始：#{} state={} draft={} mergeable={} head={} ({head_repo})",
        raw["number"], raw["state"], raw["draft"], raw["mergeable"], raw["head"]["sha"]
    );
    assert_eq!((by_branch.number, by_url.number), (number, number));
    assert_eq!(by_branch.head_oid.as_deref(), raw["head"]["sha"].as_str());
    assert_eq!(by_branch.title, raw["title"].as_str().unwrap());
    let expect_state = if raw["draft"].as_bool() == Some(true) { "draft" } else { "open" };
    assert_eq!(by_branch.state, expect_state);
    let runs = live_raw(
        "gitee.com",
        &format!(
            "https://gitee.com/api/v5/repos/openharmony/docs/commits/{}/check-runs?per_page=100",
            raw["head"]["sha"].as_str().unwrap()
        ),
    );
    let runs = runs["check_runs"].as_array().cloned().unwrap_or_default();
    eprintln!("  原始：check_runs={}", runs.len());
    assert_eq!(by_branch.checks.len(), runs.len());
    for (check, run) in by_branch.checks.iter().zip(&runs) {
        assert_eq!(
            check.state,
            expect_gitee_run(run["status"].as_str().unwrap_or(""), run["conclusion"].as_str().unwrap_or("")),
            "{run}"
        );
    }
}

// ═══ Gitee（gitee.com）═══
//
// 样例：gitee_* 是 2026-11-18 对 gitee.com 公开仓库 openharmony/docs 不带令牌只读 GET 原样存下的（PR !95569 来自 fork
// liuyongkai2/docs_lisiteng，正文截到 200 字；`pulls?state=open&head=0704`；该 PR 的评论、改动文件、头提交的 check-runs
// （空）；`/api/v5/user` 不带令牌的 401）。check-run 的 status / conclusion 取值取自官方 Swagger（/api/v5/doc_json 的
// 「创建检查项」参数枚举）。

fn gitee<'a>(fake: &'a Fake, flags: &'a Mutex<Vec<bool>>) -> super::super::gitee::Gitee<'a> {
    super::super::gitee::Gitee {
        client: client(fake, Auth::Token(token()), flags),
        state: GhState::Ok,
        login: Some("me".into()),
    }
}

#[test]
fn gitee_pr_映射_fork_未知能否合并_没有检查不编造() {
    use super::super::gitee;
    let checks = gitee::map_checks(&fixture("gitee_checkruns_empty.json"));
    assert!(checks.is_empty());
    let pr = gitee::map_pull(&fixture("gitee_pull_95569.json"), checks).unwrap();
    assert_eq!((pr.number, pr.state.as_str(), pr.forge.as_deref()), (95569, "open", Some("gitee")));
    assert_eq!(pr.url, "https://gitee.com/openharmony/docs/pulls/95569");
    assert_eq!((pr.base_ref.as_deref(), pr.head_ref.as_deref()), (Some("master"), Some("0704")));
    assert_eq!(pr.head_oid.as_deref(), Some("5f61c05b90b4e321e83b77bb15a104ad74dad63b"));
    // 实测 mergeable=false（Gitee 不给原因）：按「还不知道」，不当冲突。
    assert_eq!((pr.mergeable.as_deref(), pr.merge_state.as_deref()), (Some("UNKNOWN"), Some("UNKNOWN")));
    assert_eq!(pr.checks_summary.as_deref(), Some("none"), "仓库没接检查就不显示检查");
    for (status, conclusion, want) in [
        ("queued", "", "pending"),
        ("in_progress", "", "pending"),
        ("completed", "success", "pass"),
        ("completed", "neutral", "pass"),
        ("completed", "skipped", "skipped"),
        ("completed", "failure", "fail"),
        ("completed", "cancelled", "fail"),
        ("completed", "timed_out", "fail"),
        ("completed", "action_required", "fail"),
    ] {
        assert_eq!(
            gitee::check_state(&json!({ "status": status, "conclusion": conclusion })),
            want,
            "{status}/{conclusion}"
        );
    }
    let mut merged = fixture("gitee_pull_95569.json");
    merged["state"] = "merged".into();
    assert_eq!(gitee::map_pull(&merged, Vec::new()).unwrap().state, "merged");
}

#[test]
fn 布尔型_mergeable_的平台不因还不知道就挡住合并() {
    let unknown = |forge: Option<&str>| meowo_protocol::ipc::PullRequestDto { mergeable: Some("UNKNOWN".into()), forge: forge.map(str::to_string), ..Default::default() };
    assert!(super::super::unknown_blocks_merge(&unknown(None)), "GitHub 正在算，等一轮");
    assert!(super::super::unknown_blocks_merge(&unknown(Some("gitlab"))));
    assert!(!super::super::unknown_blocks_merge(&unknown(Some("gitee"))), "交给服务器判");
    assert!(!super::super::unknown_blocks_merge(&unknown(Some("gitea"))));
}

#[test]
fn gitee_按分支找_head_过滤_fork_与同仓库_只有已知组合的_500_当没找到() {
    let fake = Fake::new(|_, url, _| match url {
        u if u.starts_with("https://gitee.com/api/v5/repos/openharmony/docs/pulls?state=open&head=0704") => {
            resp(200, fixture("gitee_pulls_open_head.json"))
        }
        // 实测 state=all 配 head 回 500；不带 head 的最近列表这里给空。
        u if u.contains("state=all") => resp(200, json!([])),
        u if u.contains("/check-runs") => resp(200, fixture("gitee_checkruns_empty.json")),
        _ => resp(500, json!({ "message": "500 Internal Server Error" })),
    });
    let flags = Mutex::new(Vec::new());
    let ge = gitee(&fake, &flags);
    let target = Target::Branch {
        qualified: "liuyongkai2:0704",
        branch: "0704",
        head_slug: Some("liuyongkai2/docs_lisiteng"),
    };
    let pr = ge.find(".", Some("openharmony/docs"), &target).unwrap().unwrap();
    assert_eq!(pr.number, 95569);
    assert!(fake.calls().iter().all(|(_, url, _, auth)| url.starts_with("https://gitee.com/api/v5/")
        && !url.contains("access_token")
        && auth.as_deref() == Some("token glpat-TEST")));
    // 同仓库的 0704 分支没有：不把 fork 的认成自己的。
    assert_eq!(ge.find(".", Some("openharmony/docs"), &branch("0704")).unwrap(), None);
    // 复核指出：以前任何 5xx 都当「没有 PR」，PR 区块会消失、建 PR 查重被跳过、行内评论报 invalid_line。现在只有实测会 500
    // 的组合（带 head 且 state 不是 open）才当没找到，别的 5xx 照常报错。
    let broken = Fake::new(|_, _, _| resp(500, json!({ "message": "500 Internal Server Error" })));
    let ge = gitee(&broken, &flags);
    assert_eq!(ge.find(".", Some("o/r"), &branch("x")), Err("gh/failed: HTTP 500: 500 Internal Server Error".into()));
    let new = NewPr { title: "x", body: "", base: "master", head: "x", branch: "x", head_slug: None, draft: false };
    assert!(ge.create(".", Some("o/r"), &new).is_err());
    assert!(!broken.calls().iter().any(|(m, _, _, _)| m == "POST"), "查重失败就不建，免得重复建 PR");
    let err = ge.line_comment(".", "o/r", 1, "a.md", 1, "NEW", "x").unwrap_err();
    assert_ne!(err, "pr/invalid_line");
    assert!(super::super::gitee::tolerates_500("https://gitee.com/api/v5/repos/o/r/pulls?state=all&head=x"));
    assert!(!super::super::gitee::tolerates_500("https://gitee.com/api/v5/repos/o/r/pulls?state=open&head=x"));
    assert!(!super::super::gitee::tolerates_500("https://gitee.com/api/v5/repos/o/r/pulls?state=all&per_page=50"));
}

#[test]
fn gitee_合并_先核对头提交_方式按参数_拒绝归类() {
    let pr = super::super::gitee::map_pull(&fixture("gitee_pull_95569.json"), Vec::new()).unwrap();
    let reply = std::sync::Arc::new(Mutex::new(resp(200, json!({ "sha": "x", "merged": true }))));
    let r2 = reply.clone();
    let fake = Fake::new(move |method, url, _| match (method, url) {
        ("GET", u) if u.ends_with("/pulls/95569") => resp(200, fixture("gitee_pull_95569.json")),
        ("PUT", _) => r2.lock().unwrap().clone(),
        _ => resp(404, json!({})),
    });
    let flags = Mutex::new(Vec::new());
    let ge = gitee(&fake, &flags);
    assert_eq!(ge.merge(".", "openharmony/docs", &pr, "squash", "0000"), Err("gh/head_changed".into()));
    assert!(!fake.calls().iter().any(|(m, _, _, _)| m == "PUT"), "头提交对不上不发合并");
    let head = pr.head_oid.clone().unwrap();
    assert_eq!(ge.merge(".", "openharmony/docs", &pr, "squash", &head), Ok(false));
    let (_, url, body, _) = fake.calls().into_iter().find(|(m, _, _, _)| m == "PUT").unwrap();
    assert_eq!(url, "https://gitee.com/api/v5/repos/openharmony/docs/pulls/95569/merge");
    assert_eq!(body, json!({ "merge_method": "squash" }));
    for (message, want) in [
        ("此 Pull Request 存在冲突，无法自动合并", format!("gh/merge_conflict: {}", pr.url)),
        ("该 Pull Request 未通过审查", "gh/review_required".to_string()),
        ("仓库不允许此合并方式", "gh/method_not_allowed".to_string()),
        ("Pull Request 未达到合并条件", "gh/merge_blocked: Pull Request 未达到合并条件".to_string()),
    ] {
        *reply.lock().unwrap() = resp(400, json!({ "message": message }));
        assert_eq!(ge.merge(".", "openharmony/docs", &pr, "merge", &head), Err(want), "{message}");
    }
    *reply.lock().unwrap() = resp(403, json!({ "message": "403 Forbidden" }));
    assert_eq!(
        ge.merge(".", "openharmony/docs", &pr, "merge", &head),
        Err("forge/forbidden: 403 Forbidden".into())
    );
}

#[test]
fn gitee_评论_改动文件_行内评论按_diff_序号() {
    use super::super::gitee;
    let comments = gitee::map_comments(fixture("gitee_comments_95569.json").as_array().unwrap());
    assert_eq!(comments.len(), 5);
    let inline: Vec<_> = comments.iter().filter(|c| c.path.is_some()).collect();
    assert_eq!(inline.len(), 1);
    assert_eq!((inline[0].line, inline[0].side.as_deref()), (Some(17999), Some("RIGHT")));
    assert!(inline[0].url.as_deref().unwrap().ends_with("/pulls/95569#note_43636791"));
    let files = gitee::map_files(fixture("gitee_files_95569.json").as_array().unwrap());
    assert_eq!((files[0].additions, files[0].deletions), (2, 2), "Gitee 的行数是字符串");
    let patch = files[0].patch.clone().unwrap();
    assert!(patch.starts_with("@@ -17996,7 +17996,7 @@"));
    // 样例 diff：三行上下文（17996–17998）之后删一行、加一行。position 从第一个 @@ 之后数。
    assert_eq!(gitee::diff_line_position(&patch, "NEW", 17996), Some(1));
    assert_eq!(gitee::diff_line_position(&patch, "OLD", 17999), Some(4));
    assert_eq!(gitee::diff_line_position(&patch, "NEW", 17999), Some(5));
    assert_eq!(gitee::diff_line_position(&patch, "NEW", 1), None, "不在 diff 里的行发不了行内评论");
    let fake = Fake::new(|method, url, _| match (method, url) {
        ("GET", u) if u.ends_with("/pulls/95569") => resp(200, fixture("gitee_pull_95569.json")),
        ("GET", u) if u.ends_with("/files") => resp(200, fixture("gitee_files_95569.json")),
        ("POST", _) => resp(201, json!({})),
        _ => resp(404, json!({})),
    });
    let flags = Mutex::new(Vec::new());
    let path = "zh-cn/application-dev/reference/apis-arkui/_ark_u_i___native_module.md";
    gitee(&fake, &flags)
        .line_comment(".", "openharmony/docs", 95569, path, 17999, "NEW", "x")
        .unwrap();
    let (_, url, body, _) = fake.calls().into_iter().find(|(m, _, _, _)| m == "POST").unwrap();
    assert_eq!(url, "https://gitee.com/api/v5/repos/openharmony/docs/pulls/95569/comments");
    assert_eq!(
        body,
        json!({ "body": "x", "commit_id": "5f61c05b90b4e321e83b77bb15a104ad74dad63b", "path": path, "position": 5 })
    );
}

#[test]
fn gitee_开_pr_已有开着的直接给链接_401_要重新登录() {
    let fake = Fake::new(|method, url, _| match (method, url) {
        ("GET", u) if u.contains("/pulls?state=open&head=0704") => resp(200, fixture("gitee_pulls_open_head.json")),
        ("GET", "https://gitee.com/api/v5/user") => resp(401, fixture("gitee_user_noauth.json")),
        _ => resp(500, json!({})),
    });
    let flags = Mutex::new(Vec::new());
    let ge = gitee(&fake, &flags);
    let new = NewPr {
        title: "x",
        body: "",
        base: "master",
        head: "liuyongkai2:0704",
        branch: "0704",
        head_slug: Some("liuyongkai2/docs_lisiteng"),
        draft: false,
    };
    assert_eq!(
        ge.create(".", Some("openharmony/docs"), &new),
        Err("gh/pr_exists: https://gitee.com/openharmony/docs/pulls/95569".into())
    );
    assert!(fake.calls().iter().all(|(m, _, _, _)| m == "GET"));
    let client = client(&fake, Auth::Token(token()), &flags);
    assert_eq!(super::super::gitee::current_user(&client), Err("gh/not_authed".into()));
}
