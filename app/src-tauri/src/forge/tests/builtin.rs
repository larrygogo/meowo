//! 内置 gh（forge/builtin.rs）：对 forge 用到的每条 gh 子命令，输出与 gh 同形。GitHub 是假的（按地址与
//! GraphQL 查询回包），不打真网络。PR 字段的对照基准是 forge/fixtures 里 gh 2.96 实录的 `pr view --json`：
//! 把实录反推成 GitHub GraphQL 的节点喂进去，内置实现吐出来的 JSON 必须与实录逐字段相等。

use super::super::builtin::Builtin;
use super::*;
use crate::github::{Body, Http, Request, Response, Token};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};

type Handler = dyn Fn(&str, &str, &Value) -> Response + Sync;

/// 假 GitHub：记下每次请求（方法、地址、JSON 体），回包由测试给的函数决定。
struct Api {
    handler: Box<Handler>,
    calls: Mutex<Vec<(String, String, Value)>>,
}

impl Api {
    fn new(handler: impl Fn(&str, &str, &Value) -> Response + Sync + 'static) -> Self {
        Api { handler: Box::new(handler), calls: Mutex::new(Vec::new()) }
    }
    fn calls(&self) -> Vec<(String, String, Value)> {
        self.calls.lock().unwrap().clone()
    }
}

impl Http for Api {
    fn send(&self, req: &Request) -> Result<Response, String> {
        assert!(crate::github::allowed_url(req.url), "只许发往 GitHub：{}", req.url);
        assert!(req.token.is_some(), "内置实现的请求都要带令牌");
        let body = match &req.body {
            Some(Body::Json(v)) => v.clone(),
            _ => Value::Null,
        };
        self.calls.lock().unwrap().push((req.method.to_string(), req.url.to_string(), body.clone()));
        Ok((self.handler)(req.method, req.url, &body))
    }
}

fn resp(status: u16, body: Value) -> Response {
    Response { status, body: body.to_string(), ..Default::default() }
}

fn data(v: Value) -> Response {
    resp(200, json!({ "data": v }))
}

fn query_of(body: &Value) -> &str {
    body.get("query").and_then(|q| q.as_str()).unwrap_or_default()
}

fn token() -> Token {
    Token::new("gho_TESTTOKEN").unwrap()
}

fn run(api: &Api, prefs: &Path, dir: &str, args: &[&str]) -> CmdOut {
    let token = token();
    Builtin { http: api, token: Some(&token), on_unauthorized: &|| {}, prefs }.run(dir, args)
}

fn json_out(out: &CmdOut) -> Value {
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    serde_json::from_str(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", out.stdout))
}

/// gh `pr view --json` 的实录 → GitHub GraphQL 的 PR 节点（内置实现查到的原样）。
fn gql_node(gh: &Value, owner: &str, cross: bool) -> Value {
    let mut node = gh.clone();
    let contexts: Vec<Value> = gh["statusCheckRollup"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|c| {
            let blank = |v: &Value| if v.as_str().is_some_and(|s| s.is_empty() || s == "0001-01-01T00:00:00Z") { Value::Null } else { v.clone() };
            if c["__typename"] == "StatusContext" {
                json!({ "__typename": "StatusContext", "context": c["context"], "state": c["state"], "targetUrl": c["targetUrl"], "createdAt": c["startedAt"] })
            } else {
                json!({
                    "__typename": "CheckRun", "name": c["name"], "status": c["status"], "conclusion": blank(&c["conclusion"]),
                    "detailsUrl": c["detailsUrl"], "startedAt": blank(&c["startedAt"]), "completedAt": blank(&c["completedAt"]),
                    "checkSuite": { "workflowRun": { "workflow": { "name": c["workflowName"] } } },
                })
            }
        })
        .collect();
    let obj = node.as_object_mut().unwrap();
    obj.remove("statusCheckRollup");
    obj.insert("commits".into(), json!({ "nodes": [{ "commit": { "statusCheckRollup": { "contexts": { "nodes": contexts, "pageInfo": { "hasNextPage": false, "endCursor": null } } } } }] }));
    obj.insert("meowoId".into(), json!("PR_kwDO"));
    obj.insert("meowoState".into(), gh["state"].clone());
    obj.insert("meowoCross".into(), json!(cross));
    obj.insert("meowoOwner".into(), json!({ "login": owner }));
    node
}

#[test]
fn pr_view_按号_各实录样例逐字段同形() {
    let prefs = std::env::temp_dir().join("meowo-builtin-none.json");
    for name in [
        "pr_checkrun_fail.json",
        "pr_checkrun_merged.json",
        "pr_conflict.json",
        "pr_status_context_pending.json",
        "derived_pr_in_progress.json",
        "derived_pr_no_checks.json",
        "derived_pr_unknown_mergeable.json",
    ] {
        let recorded: Value = serde_json::from_str(&fixture(name)).unwrap();
        let node = gql_node(&recorded, "someone", false);
        let api = Api::new(move |_, url, body| {
            assert!(url.ends_with("/graphql"));
            assert!(query_of(body).contains("pullRequest(number:$number)"));
            data(json!({ "repository": { "pullRequest": node.clone() } }))
        });
        let number = recorded["number"].to_string();
        let out = run(&api, &prefs, ".", &["pr", "view", &number, "--repo", "acme/repo", "--json", PR_FIELDS]);
        assert_eq!(json_out(&out), recorded, "{name}");
        // 与 gh 同一个解析：forge 拿到的 DTO 也一样。
        assert_eq!(parse_pr_full(&out.stdout), parse_pr_full(&fixture(name)), "{name}");
        let vars = &api.calls()[0].2["variables"];
        assert_eq!((vars["owner"].as_str(), vars["name"].as_str()), (Some("acme"), Some("repo")));
    }
}

#[test]
fn pr_view_按分支_挑开着的_不认同名_fork_没有就是_no_pull_requests_found() {
    let prefs = std::env::temp_dir().join("meowo-builtin-none.json");
    let base: Value = serde_json::from_str(&fixture("derived_pr_no_checks.json")).unwrap();
    let mut closed = base.clone();
    closed["number"] = json!(10);
    closed["state"] = json!("CLOSED");
    let mut fork = base.clone();
    fork["number"] = json!(11);
    let nodes = json!([gql_node(&fork, "stranger", true), gql_node(&closed, "larrygogo", false), gql_node(&base, "larrygogo", false)]);
    let api = Api::new(move |_, _, body| {
        let q = query_of(body);
        assert!(q.contains("pullRequests(headRefName:$head"));
        let head = body["variables"]["head"].as_str().unwrap();
        data(json!({ "repository": { "pullRequests": { "nodes": if head == "meowo/drill" { nodes.clone() } else { json!([]) } } } }))
    });
    let out = run(&api, &prefs, ".", &["pr", "view", "meowo/drill", "--repo", "larrygogo/meowo-pr-drill", "--json", "number,state"]);
    assert_eq!(json_out(&out), json!({ "number": 12, "state": "OPEN" }));
    // owner:分支 只认那个 owner 的（fork 的 PR）。
    let out = run(&api, &prefs, ".", &["pr", "view", "stranger:meowo/drill", "--repo", "larrygogo/meowo-pr-drill", "--json", "number"]);
    assert_eq!(json_out(&out), json!({ "number": 11 }));
    let out = run(&api, &prefs, ".", &["pr", "view", "nope", "--repo", "larrygogo/meowo-pr-drill", "--json", "number"]);
    assert_eq!(out.code, Some(1));
    assert!(out.stderr.contains("no pull requests found for branch \"nope\""));
    // forge 的 view_pr 照常认出「没有 PR」。
    let gh = |dir: &str, args: &[&str]| Some(run(&api, &prefs, dir, args));
    assert_eq!(view_pr(&gh, &Caches::default(), ".", Some("larrygogo/meowo-pr-drill"), Some("nope")), Ok(None));
}

#[test]
fn pr_view_按链接与不带目标_按目录认仓库和分支() {
    let sb = Sandbox::new("builtin-view");
    let repo = github_repo(&sb);
    let prefs = sb.root.join("forge.json");
    let recorded: Value = serde_json::from_str(&fixture("derived_pr_no_checks.json")).unwrap();
    let node = gql_node(&recorded, "larrygogo", false);
    let api = Api::new(move |_, _, body| {
        let vars = &body["variables"];
        assert_eq!(vars["owner"], "larrygogo");
        assert_eq!(vars["name"], "meowo-pr-drill");
        if query_of(body).contains("pullRequests(") {
            assert_eq!(vars["head"], "meowo/drill", "不带目标 = 当前分支");
            data(json!({ "repository": { "pullRequests": { "nodes": [node.clone()] } } }))
        } else {
            assert_eq!(vars["number"], 12);
            data(json!({ "repository": { "pullRequest": node.clone() } }))
        }
    });
    let out = run(&api, &prefs, &s(&repo), &["pr", "view", "https://github.com/larrygogo/meowo-pr-drill/pull/12", "--json", "number,state,isDraft,url,title"]);
    assert_eq!(json_out(&out)["number"], 12);
    // worktree 清理的查法：不带目标、不带 --repo。
    let gh = |dir: &str, args: &[&str]| Some(run(&api, &prefs, dir, args));
    match crate::worktree::gh_pr_lookup_with(&gh, &repo) {
        crate::worktree::PrLookup::Found { number, state, head_oid } => {
            assert_eq!((number, state.as_str(), head_oid.as_str()), (12, "OPEN", "1111111111111111111111111111111111111111"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn pr_view_评论与评审_超过一页按游标续取() {
    let prefs = std::env::temp_dir().join("meowo-builtin-none.json");
    let pages = AtomicUsize::new(0);
    let api = Api::new(move |_, _, body| {
        let q = query_of(body);
        if q.contains("node(id:$id)") {
            pages.fetch_add(1, Ordering::SeqCst);
            assert_eq!(body["variables"]["after"], "c1");
            assert!(q.contains("comments(first:100,after:$after)"));
            return data(json!({ "node": { "comments": { "nodes": [{ "id": "IC_2", "author": { "login": "bob" }, "body": "second", "createdAt": "2026-01-02T00:00:00Z", "url": "u2" }], "pageInfo": { "hasNextPage": false, "endCursor": "c2" } } } }));
        }
        data(json!({ "repository": { "pullRequest": {
            "meowoId": "PR_1", "meowoState": "OPEN", "meowoCross": false, "meowoOwner": { "login": "acme" }, "number": 7,
            "comments": { "nodes": [{ "id": "IC_1", "author": { "login": "alice" }, "body": "first", "createdAt": "2026-01-01T00:00:00Z", "url": "u1" }], "pageInfo": { "hasNextPage": true, "endCursor": "c1" } },
            "reviews": { "nodes": [{ "id": "PRR_1", "author": { "login": "rev" }, "body": "ok", "state": "APPROVED", "submittedAt": "2026-01-03T00:00:00Z" }], "pageInfo": { "hasNextPage": false, "endCursor": null } },
        } } }))
    });
    let out = run(&api, &prefs, ".", &["pr", "view", "7", "--repo", "acme/repo", "--json", "number,comments,reviews"]);
    let v = json_out(&out);
    let ids: Vec<&str> = v["comments"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["IC_1", "IC_2"]);
    assert_eq!(v["reviews"][0]["state"], "APPROVED");
    assert_eq!(v["comments"][1]["author"]["login"], "bob");
}

#[test]
fn pr_create_成功回链接_已存在时回_gh_原话带链接() {
    let prefs = std::env::temp_dir().join("meowo-builtin-none.json");
    let exists = std::sync::atomic::AtomicBool::new(false);
    let api = Api::new(move |method, url, body| {
        if method == "POST" && url.ends_with("/repos/acme/repo/pulls") {
            assert_eq!(body["head"], "feat");
            assert_eq!(body["base"], "main");
            assert_eq!(body["draft"], true);
            if exists.swap(true, Ordering::SeqCst) {
                return resp(422, json!({ "message": "Validation Failed", "errors": [{ "resource": "PullRequest", "code": "custom", "message": "A pull request already exists for acme:feat." }] }));
            }
            return resp(201, json!({ "html_url": "https://github.com/acme/repo/pull/5", "number": 5 }));
        }
        assert!(url.contains("/repos/acme/repo/pulls?state=open&head=acme%3Afeat&base=main"), "{url}");
        resp(200, json!([{ "html_url": "https://github.com/acme/repo/pull/5" }]))
    });
    let args = ["pr", "create", "--title", "T", "--body", "B", "--base", "main", "--head", "feat", "--draft", "--repo", "acme/repo"];
    let out = run(&api, &prefs, ".", &args);
    assert_eq!((out.code, out.stdout.trim()), (Some(0), "https://github.com/acme/repo/pull/5"));
    let out = run(&api, &prefs, ".", &args);
    assert_eq!(out.code, Some(1));
    assert!(out.both().contains("already exists") && out.both().lines().any(|l| l.trim() == "https://github.com/acme/repo/pull/5"));
}

#[test]
fn pr_merge_三种方式钉住头提交_头变了与合并队列() {
    let prefs = std::env::temp_dir().join("meowo-builtin-none.json");
    let api = Api::new(|method, url, body| {
        if method == "PUT" {
            assert!(url.ends_with("/repos/acme/repo/pulls/9/merge"));
            return match body["sha"].as_str() {
                Some("stale") => resp(409, json!({ "message": "Head branch was modified. Review and try the merge again." })),
                Some("queued") => resp(405, json!({ "message": "Changes must be made through the merge queue" })),
                _ => resp(200, json!({ "merged": true })),
            };
        }
        let q = query_of(body);
        if q.contains("enqueuePullRequest") {
            assert_eq!(body["variables"]["input"], json!({ "pullRequestId": "PR_9", "expectedHeadOid": "queued" }));
            return data(json!({ "enqueuePullRequest": { "mergeQueueEntry": { "id": "MQE" } } }));
        }
        data(json!({ "repository": { "pullRequest": { "id": "PR_9", "baseRefName": "main" } } }))
    });
    for method in ["merge", "squash", "rebase"] {
        let flag = format!("--{method}");
        let out = run(&api, &prefs, ".", &["pr", "merge", "9", &flag, "--match-head-commit", "abc", "--repo", "acme/repo"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        let last = api.calls().last().unwrap().2.clone();
        assert_eq!(last, json!({ "merge_method": method, "sha": "abc" }));
    }
    let out = run(&api, &prefs, ".", &["pr", "merge", "9", "--squash", "--match-head-commit", "stale", "--repo", "acme/repo"]);
    assert_eq!(classify_gh_failure(&out, None), "gh/head_changed");
    let out = run(&api, &prefs, ".", &["pr", "merge", "9", "--merge", "--match-head-commit", "queued", "--repo", "acme/repo"]);
    assert_eq!(out.code, Some(0));
    assert!(out.both().to_lowercase().contains("merge queue"), "forge 靠这句判断进了队列");
    // gh 新旧探测。
    let help = run(&api, &prefs, ".", &["pr", "merge", "--help"]);
    assert!(help.both().contains("--match-head-commit"));
}

#[test]
fn repo_view_合并方式与实录同形() {
    let prefs = std::env::temp_dir().join("meowo-builtin-none.json");
    let api = Api::new(|_, _, body| {
        assert!(query_of(body).contains("mergeCommitAllowed squashMergeAllowed rebaseMergeAllowed"));
        data(json!({ "repository": { "mergeCommitAllowed": true, "squashMergeAllowed": true, "rebaseMergeAllowed": true } }))
    });
    let out = run(&api, &prefs, ".", &["repo", "view", "larrygogo/meowo-pr-drill", "--json", "mergeCommitAllowed,squashMergeAllowed,rebaseMergeAllowed"]);
    let recorded: Value = serde_json::from_str(&fixture("repo_view_drill.json")).unwrap();
    assert_eq!(json_out(&out), recorded);
    assert_eq!(parse_merge_methods(&out.stdout), ["merge", "squash", "rebase"]);
}

#[test]
fn api_分页_slurp_与字段() {
    let prefs = std::env::temp_dir().join("meowo-builtin-none.json");
    let api = Api::new(|method, url, body| {
        if url.ends_with("/user") {
            return resp(200, json!({ "login": "me" }));
        }
        if method == "POST" {
            assert!(url.ends_with("/repos/acme/repo/pulls/3/comments"));
            assert_eq!(body, &json!({ "body": "hi there", "commit_id": "abc", "path": "src/a.ts", "line": 3, "side": "NEW" }));
            return resp(201, json!({ "id": 1 }));
        }
        if url.contains("page=2") {
            return resp(200, json!([{ "filename": "b.ts" }]));
        }
        Response {
            status: 200,
            body: json!([{ "filename": "a.ts" }]).to_string(),
            link: Some("<https://api.github.com/repositories/1/pulls/3/files?page=2>; rel=\"next\", <https://api.github.com/repositories/1/pulls/3/files?page=2>; rel=\"last\"".into()),
            location: None,
        }
    });
    let out = run(&api, &prefs, ".", &["api", "repos/acme/repo/pulls/3/files", "--paginate", "--slurp"]);
    assert_eq!(json_out(&out), json!([[{ "filename": "a.ts" }], [{ "filename": "b.ts" }]]));
    let out = run(&api, &prefs, ".", &["api", "repos/acme/repo/pulls/3/files", "--paginate"]);
    assert_eq!(json_out(&out), json!([{ "filename": "a.ts" }, { "filename": "b.ts" }]));
    let out = run(&api, &prefs, ".", &["api", "repos/acme/repo/pulls/3/comments", "-f", "body=hi there", "-f", "commit_id=abc", "-f", "path=src/a.ts", "-F", "line=3", "-f", "side=NEW"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(json_out(&run(&api, &prefs, ".", &["api", "user"]))["login"], "me");
    // 别的主机、读宿主文件：拒。
    assert_eq!(run(&api, &prefs, ".", &["api", "https://evil.com/x"]).code, Some(1));
    assert_eq!(run(&api, &prefs, ".", &["api", "x", "-F", "body=@/etc/passwd"]).code, Some(1));
}

#[test]
fn api_非_2xx_回_gh_的写法() {
    let prefs = std::env::temp_dir().join("meowo-builtin-none.json");
    let api = Api::new(|_, _, _| resp(404, json!({ "message": "Not Found" })));
    let out = run(&api, &prefs, ".", &["api", "repos/acme/none"]);
    assert_eq!(out.code, Some(1));
    assert!(out.stderr.contains("gh: Not Found (HTTP 404)"));
    assert!(out.stdout.contains("Not Found"));
}

#[test]
fn 评论与评审写入() {
    let prefs = std::env::temp_dir().join("meowo-builtin-none.json");
    let api = Api::new(|_, url, body| {
        if url.ends_with("/repos/acme/repo/pulls/4/reviews") {
            assert_eq!(body, &json!({ "body": "lgtm", "event": "COMMENT" }));
        } else {
            assert!(url.ends_with("/repos/acme/repo/issues/4/comments"));
            assert_eq!(body, &json!({ "body": "note" }));
        }
        resp(201, json!({ "html_url": "https://github.com/acme/repo/pull/4#c" }))
    });
    assert_eq!(run(&api, &prefs, ".", &["pr", "comment", "4", "--repo", "acme/repo", "--body", "note"]).code, Some(0));
    assert_eq!(run(&api, &prefs, ".", &["pr", "review", "4", "--repo", "acme/repo", "--body", "lgtm", "--comment"]).code, Some(0));
}

#[test]
fn 登录状态_401_与没登录_forge_都认成没登录_401_通知重新登录() {
    let prefs = std::env::temp_dir().join("meowo-builtin-none.json");
    let api = Api::new(|_, _, _| resp(401, json!({ "message": "Bad credentials" })));
    let flagged = AtomicUsize::new(0);
    let flag = || {
        flagged.fetch_add(1, Ordering::SeqCst);
    };
    let token = token();
    let builtin = Builtin { http: &api, token: Some(&token), on_unauthorized: &flag, prefs: &prefs };
    let out = builtin.run(".", &["auth", "status", "--hostname", "github.com"]);
    assert_eq!(out.code, Some(1));
    assert!(says_unauthed(&out));
    assert!(!out.both().contains("gho_TESTTOKEN"), "错误输出里不能有令牌");
    assert_eq!(flagged.load(Ordering::SeqCst), 1);
    let gh = |dir: &str, args: &[&str]| Some(builtin.run(dir, args));
    let caches = Caches::default();
    assert_eq!(caches.gh_state(&gh, true), GhState::Unauthed);
    assert_eq!(view_pr(&gh, &caches, ".", Some("acme/repo"), Some("feat")), Err("gh/not_authed".into()));
    // Meowo 模式没登录：不发请求，同样是没登录。
    let none = Builtin { http: &api, token: None, on_unauthorized: &flag, prefs: &prefs };
    let calls = api.calls().len();
    let out = none.run(".", &["auth", "status", "--hostname", "github.com"]);
    assert!(out.code == Some(1) && says_unauthed(&out));
    assert_eq!(api.calls().len(), calls);
    // 登录有效。
    let good = Api::new(|_, _, _| resp(200, json!({ "login": "octocat" })));
    let gh = |dir: &str, args: &[&str]| Some(run(&good, &prefs, dir, args));
    assert_eq!(Caches::default().gh_state(&gh, true), GhState::Ok);
}

#[test]
fn 断网_按查询失败处理() {
    struct Down;
    impl Http for Down {
        fn send(&self, _: &Request) -> Result<Response, String> {
            Err("Connection refused".into())
        }
    }
    let prefs = std::env::temp_dir().join("meowo-builtin-none.json");
    let token = token();
    let builtin = Builtin { http: &Down, token: Some(&token), on_unauthorized: &|| {}, prefs: &prefs };
    let gh = |dir: &str, args: &[&str]| Some(builtin.run(dir, args));
    let err = view_pr(&gh, &Caches::default(), ".", Some("acme/repo"), Some("feat")).unwrap_err();
    assert!(err.starts_with("gh/failed") && err.contains("error connecting"), "{err}");
}

/// forge 的整条流程跑在内置实现上：「更改」面板状态 → 开 PR → 合并，与 gh 时同样的结果。
#[test]
fn forge_全流程_在内置实现上() {
    let sb = Sandbox::new("builtin-flow");
    let repo = github_repo(&sb);
    let prefs = sb.root.join("forge.json");
    let open: Value = serde_json::from_str(&fixture("derived_pr_no_checks.json")).unwrap();
    let created = std::sync::atomic::AtomicBool::new(false);
    let api = Api::new(move |method, url, body| {
        if url.ends_with("/user") {
            return resp(200, json!({ "login": "larrygogo" }));
        }
        if method == "POST" && url.ends_with("/repos/larrygogo/meowo-pr-drill/pulls") {
            created.store(true, Ordering::SeqCst);
            return resp(201, json!({ "html_url": "https://github.com/larrygogo/meowo-pr-drill/pull/12" }));
        }
        if method == "PUT" {
            assert_eq!(body, &json!({ "merge_method": "squash", "sha": "1111111111111111111111111111111111111111" }));
            return resp(200, json!({ "merged": true }));
        }
        let q = query_of(body);
        if q.contains("mergeCommitAllowed") {
            return data(json!({ "repository": { "mergeCommitAllowed": true, "squashMergeAllowed": true, "rebaseMergeAllowed": false } }));
        }
        let nodes = if created.load(Ordering::SeqCst) { json!([gql_node(&open, "larrygogo", false)]) } else { json!([]) };
        data(json!({ "repository": { "pullRequests": { "nodes": nodes } } }))
    });
    let gh = |dir: &str, args: &[&str]| Some(run(&api, &prefs, dir, args));
    let caches = Caches::default();
    let ctx = Ctx { gh: &gh, caches: &caches, prefs: prefs.clone(), env: &[] };
    let dto = forge_status_with(&ctx, &s(&repo), false).unwrap();
    assert_eq!(dto.gh, "ok");
    assert!(!dto.gh_too_old);
    assert_eq!(dto.merge_methods, ["merge", "squash"]);
    assert!(dto.pr.is_none() && dto.pr_defaults.is_some());
    let pushed = |_: &str| Ok(PushResultDto { remote: "origin".into(), branch: "meowo/drill".into(), set_upstream: true, pushed: 1 });
    let pr = pr_create_with(&ctx, &s(&repo), Some("T"), None, None, false, &pushed).unwrap();
    assert_eq!((pr.number, pr.state.as_str()), (12, "open"));
    let merged = pr_merge_with(&ctx, &s(&repo), 12, "squash", "1111111111111111111111111111111111111111").unwrap();
    assert!(merged.merged && !merged.queued);
}

#[test]
fn 关闭时状态标为_off() {
    let sb = Sandbox::new("builtin-off");
    let repo = github_repo(&sb);
    let gh = |_: &str, _: &[&str]| -> Option<CmdOut> { None };
    let ctx = Ctx { gh: &gh, caches: &Caches::default(), prefs: sb.root.join("forge.json"), env: &[] };
    let mut dto = forge_status_with(&ctx, &s(&repo), false).unwrap();
    label_connection(&mut dto, "off");
    assert_eq!((dto.gh.as_str(), dto.connection.as_deref()), ("off", Some("off")));
    let mut dto = forge_status_with(&ctx, &s(&repo), false).unwrap();
    label_connection(&mut dto, "gh");
    assert_eq!(dto.gh, "missing", "本机 gh 方式没装 gh：照旧是 missing");
}
