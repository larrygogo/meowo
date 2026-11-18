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
    Builtin { http: api, token: Some(&token), on_auth: &|_| {}, prefs, ends: crate::forge::builtin::Endpoints::github_com() }.run(dir, args)
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
    obj.insert("meowoHead".into(), gh["headRefName"].clone());
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
    // 与 gh 一样每页 100 条（G7 真机对照：不补的话 --slurp 的分页边界与 gh 不同）。
    assert!(api.calls()[0].1.ends_with("/repos/acme/repo/pulls/3/files?per_page=100"), "{}", api.calls()[0].1);
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
    let flag = |ok: bool| {
        if !ok {
            flagged.fetch_add(1, Ordering::SeqCst);
        }
    };
    let token = token();
    let builtin = Builtin { http: &api, token: Some(&token), on_auth: &flag, prefs: &prefs, ends: crate::forge::builtin::Endpoints::github_com() };
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
    let none = Builtin { http: &api, token: None, on_auth: &flag, prefs: &prefs, ends: crate::forge::builtin::Endpoints::github_com() };
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
    let builtin = Builtin { http: &Down, token: Some(&token), on_auth: &|_| {}, prefs: &prefs, ends: crate::forge::builtin::Endpoints::github_com() };
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
    let ctx = Ctx { forges: &crate::forge::platform::NO_ACCOUNTS, gh: &gh, caches: &caches, prefs: prefs.clone(), env: &[] };
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
    let ctx = Ctx { forges: &crate::forge::platform::NO_ACCOUNTS, gh: &gh, caches: &Caches::default(), prefs: sb.root.join("forge.json"), env: &[] };
    let mut dto = forge_status_with(&ctx, &s(&repo), false).unwrap();
    label_connection(&mut dto, "off");
    assert_eq!((dto.gh.as_str(), dto.connection.as_deref()), ("off", Some("off")));
    let mut dto = forge_status_with(&ctx, &s(&repo), false).unwrap();
    label_connection(&mut dto, "gh");
    assert_eq!(dto.gh, "missing", "本机 gh 方式没装 gh：照旧是 missing");
}

/// 回归（独立审查 #2）：与 gh 的 findForRefs 对齐——查 30 条、开着的优先、head 写法完全相等、
/// 分支就是默认分支时不认已合并 / 已关闭的。修复前只查 10 条、默认分支上会挑到已合并的 PR。
#[test]
fn pr_view_按分支_对齐_gh_查30条_默认分支不认已了结的() {
    let prefs = std::env::temp_dir().join("meowo-builtin-none.json");
    let base: Value = serde_json::from_str(&fixture("derived_pr_no_checks.json")).unwrap();
    let with = |number: u64, state: &str, head: &str, owner: &str, cross: bool| {
        let mut pr = base.clone();
        pr["number"] = json!(number);
        pr["state"] = json!(state);
        pr["headRefName"] = json!(head);
        gql_node(&pr, owner, cross)
    };
    // 12 个 fork 用了同名分支、都比自己的新：自己的那条排在第 13 位。
    let mut crowded: Vec<Value> = (100..112).map(|n| with(n, "OPEN", "feature", &format!("fork{n}"), true)).collect();
    crowded.push(with(7, "OPEN", "feature", "acme", false));
    let main_nodes = json!([with(20, "MERGED", "main", "acme", false), with(21, "CLOSED", "main", "acme", false)]);
    let main_open = json!([with(22, "MERGED", "main", "acme", false), with(23, "OPEN", "main", "acme", false)]);
    let crowded = json!(crowded);
    let api = Api::new(move |_, _, body| {
        let q = query_of(body);
        assert!(q.contains("first:30") && q.contains("defaultBranchRef{name}"), "{q}");
        let nodes = match body["variables"]["head"].as_str().unwrap() {
            "feature" => crowded.clone(),
            "main" if body["variables"]["name"] == "repo" => main_nodes.clone(),
            "main" => main_open.clone(),
            _ => json!([]),
        };
        data(json!({ "repository": { "pullRequests": { "nodes": nodes }, "defaultBranchRef": { "name": "main" } } }))
    });
    let number = |args: &[&str]| {
        let out = run(&api, &prefs, ".", args);
        if out.code == Some(0) { json_out(&out)["number"].as_u64() } else { assert!(out.stderr.contains("no pull requests found")); None }
    };
    assert_eq!(number(&["pr", "view", "feature", "--repo", "acme/repo", "--json", "number"]), Some(7));
    assert_eq!(number(&["pr", "view", "fork105:feature", "--repo", "acme/repo", "--json", "number"]), Some(105));
    // gh 的 HeadLabel：同仓库 PR 的写法没有 owner，`acme:feature` 对不上同仓库那条。
    assert_eq!(number(&["pr", "view", "acme:feature", "--repo", "acme/repo", "--json", "number"]), None);
    assert_eq!(number(&["pr", "view", "main", "--repo", "acme/repo", "--json", "number"]), None, "默认分支上只有已了结的：没有 PR");
    assert_eq!(number(&["pr", "view", "main", "--repo", "acme/other", "--json", "number"]), Some(23), "默认分支上开着的照认");
}
/// connected_gh 的分派本身：关闭不发请求、本机 gh、Meowo（含没登录）、自动按主机分流（审查 #6、#9）。
#[test]
fn 分派_四种方式_自动按主机() {
    use crate::github::Executor;
    let gh_calls = AtomicUsize::new(0);
    let builtin_calls = Mutex::new(Vec::<Option<String>>::new());
    let gh = |_: &str, _: &[&str]| {
        gh_calls.fetch_add(1, Ordering::SeqCst);
        Some(CmdOut { code: Some(0), stdout: "gh".into(), ..Default::default() })
    };
    let builtin = |_: &str, _: &[&str], token: Option<&Token>| {
        builtin_calls.lock().unwrap().push(token.map(|t| t.expose().to_string()));
        CmdOut { code: Some(0), stdout: "builtin".into(), ..Default::default() }
    };
    let github_com = |dir: &str, _: &[&str]| dir == "github-repo";
    let args = ["pr", "view", "--json", "number"];
    let token = Token::new("gho_t").unwrap();
    let run = |exec: Executor, dir: &str| dispatch_gh(exec, dir, &args, &gh, &builtin, &github_com).map(|o| o.stdout);
    assert_eq!(run(Executor::Off, "github-repo"), None);
    assert_eq!(gh_calls.load(Ordering::SeqCst), 0, "关闭：不发任何请求");
    assert_eq!(run(Executor::Gh, "github-repo").as_deref(), Some("gh"));
    assert_eq!(run(Executor::Meowo(None), "ghe-repo").as_deref(), Some("builtin"));
    assert_eq!(run(Executor::Meowo(Some(token.clone())), "ghe-repo").as_deref(), Some("builtin"));
    assert_eq!(run(Executor::Auto(token.clone()), "github-repo").as_deref(), Some("builtin"));
    assert_eq!(run(Executor::Auto(token), "ghe-repo").as_deref(), Some("gh"), "自动：企业版仍走本机 gh");
    assert_eq!(*builtin_calls.lock().unwrap(), [None, Some("gho_t".to_string()), Some("gho_t".to_string())]);
    assert_eq!(gh_calls.load(Ordering::SeqCst), 2);
}

#[test]
fn 认主机_链接_repo_参数与目录远端() {
    let sb = Sandbox::new("builtin-host");
    let github = github_repo(&sb);
    let ghe = sb.repo("ghe");
    sb.git(&ghe, &["remote", "add", "origin", "https://github.example.com/acme/app.git"]);
    let none = sb.root.join("none");
    std::fs::create_dir_all(&none).unwrap();
    assert!(targets_github_com(".", &["pr", "view", "https://github.com/o/r/pull/1", "--json", "number"]));
    assert!(!targets_github_com(".", &["pr", "view", "https://github.example.com/o/r/pull/1", "--json", "number"]));
    assert!(targets_github_com(".", &["pr", "view", "feat", "--repo", "o/r", "--json", "number"]));
    assert!(targets_github_com(".", &["auth", "status", "--hostname", "github.com"]));
    assert!(targets_github_com(&s(&github), &["pr", "view", "--json", "number"]));
    assert!(!targets_github_com(&s(&ghe), &["pr", "view", "--json", "number"]));
    assert!(!targets_github_com(&s(&none), &["pr", "view", "--json", "number"]));
}

/// 切方式、登录、退出共用的缓存作废：登录状态、gh 新旧、合并方式、PR、入队标记都清掉（审查 #9）。
#[test]
fn 连接变了_forge_缓存全部作废() {
    let key = format!("meowo-test-invalidate-{}", std::process::id());
    *lock(&CACHES.auth) = Some((Instant::now(), GhState::Ok));
    *lock(&CACHES.too_old) = Some(false);
    lock(&CACHES.methods).insert(key.clone(), (Instant::now(), vec!["merge".into()]));
    lock(&CACHES.prs).insert(key.clone(), (Instant::now(), None));
    lock(&CACHES.queued).insert((key.clone(), 1, "sha".into()));
    invalidate_connection();
    assert!(lock(&CACHES.auth).is_none());
    assert!(lock(&CACHES.too_old).is_none());
    assert!(!lock(&CACHES.methods).contains_key(&key));
    assert!(!lock(&CACHES.prs).contains_key(&key));
    assert!(!lock(&CACHES.queued).iter().any(|(k, _, _)| *k == key));
}

// ═══ 真机核对（G7，只读）═══
//
// 默认 #[ignore]。运行（PowerShell，令牌只进环境变量，不落文件、不打印）：
//   $env:GH_TOKEN = (gh auth token); cargo test -p meowo-app --lib live_对照_gh -- --ignored --nocapture; Remove-Item Env:GH_TOKEN
// 用内置执行器（真 HTTP）与本机 gh 对同一条只读命令各跑一次，逐字段对比。不做任何写操作。

/// 两份 JSON 的差异路径（gh 有而我们没有的字段、值不同的字段；我们多出的字段也列出）。
fn json_diff(path: &str, gh: &Value, ours: &Value, out: &mut Vec<String>) {
    match (gh, ours) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in a {
                match b.get(k) {
                    Some(w) => json_diff(&format!("{path}.{k}"), v, w, out),
                    None => out.push(format!("{path}.{k}: 只有 gh 有")),
                }
            }
            for k in b.keys().filter(|k| !a.contains_key(*k)) {
                out.push(format!("{path}.{k}: 只有内置有"));
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                out.push(format!("{path}: 条数 gh {} / 内置 {}", a.len(), b.len()));
            }
            for (i, (v, w)) in a.iter().zip(b).enumerate() {
                json_diff(&format!("{path}[{i}]"), v, w, out);
            }
        }
        (Value::String(a), Value::String(b)) if a != b => {
            let at = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
            let around = |s: &str| s.chars().skip(at.saturating_sub(20)).take(50).collect::<String>().escape_debug().to_string();
            out.push(format!("{path}: 长度 gh {} / 内置 {}，第 {at} 字起不同：gh「{}」/ 内置「{}」", a.chars().count(), b.chars().count(), around(a), around(b)));
        }
        _ if gh != ours => out.push(format!("{path}: gh {} / 内置 {}", short(gh), short(ours))),
        _ => {}
    }
}

fn short(v: &Value) -> String {
    let text = v.to_string();
    if text.chars().count() > 60 { format!("{}…", text.chars().take(60).collect::<String>()) } else { text }
}

/// 断网偶发（本机到 GitHub 的 TLS 握手常超时）：两边各重试几次。
fn with_retry(run: impl Fn() -> Option<CmdOut>) -> CmdOut {
    let mut last = CmdOut::default();
    for _ in 0..6 {
        let Some(out) = run() else { panic!("gh 不在") };
        let flaky = out.both().contains("error connecting") || out.both().contains("TLS handshake") || out.both().contains("EOF") || out.timed_out;
        if !flaky {
            return out;
        }
        last = out;
    }
    panic!("连不上 GitHub：{}", tail(&last.both(), 3))
}

#[test]
#[ignore]
fn live_对照_gh() {
    let token = Token::new(&std::env::var("GH_TOKEN").expect("设 GH_TOKEN")).expect("令牌格式");
    let prefs = std::env::temp_dir().join("meowo-live-none.json");
    let builtin = Builtin { http: &crate::github::RealHttp, token: Some(&token), on_auth: &|_| {}, prefs: &prefs, ends: crate::forge::builtin::Endpoints::github_com() };
    let dir = std::env::temp_dir();
    let dir = dir.to_string_lossy();
    let cli_pr = with_retry(|| real_gh(&dir, &["pr", "list", "--repo", "cli/cli", "--state", "merged", "--limit", "1", "--json", "number"]));
    let cli_pr = serde_json::from_str::<Value>(&cli_pr.stdout).unwrap()[0]["number"].to_string();
    let fields_review = format!("{PR_FIELDS},comments,reviews");
    let cases: Vec<Vec<&str>> = vec![
        vec!["pr", "view", "192", "--repo", "larrygogo/meowo", "--json", PR_FIELDS],
        vec!["pr", "view", "192", "--repo", "larrygogo/meowo", "--json", &fields_review],
        vec!["pr", "view", "fix/scm-switch-flash", "--repo", "larrygogo/meowo", "--json", "number,state"],
        vec!["pr", "view", "meowo/no-such-branch-for-g7", "--repo", "larrygogo/meowo", "--json", "number,state"],
        vec!["repo", "view", "larrygogo/meowo", "--json", "mergeCommitAllowed,squashMergeAllowed,rebaseMergeAllowed"],
        vec!["api", "repos/larrygogo/meowo/pulls/192/files", "--paginate", "--slurp"],
        vec!["api", "user"],
        vec!["pr", "view", &cli_pr, "--repo", "cli/cli", "--json", PR_FIELDS],
        vec!["auth", "status", "--hostname", "github.com"],
    ];
    let mut report = Vec::new();
    for args in &cases {
        let gh = with_retry(|| real_gh(&dir, args));
        let ours = with_retry(|| Some(builtin.run(&dir, args)));
        let line = args.join(" ");
        if gh.code != ours.code {
            report.push(format!("{line}\n  退出码 gh {:?} / 内置 {:?}\n  gh: {}\n  内置: {}", gh.code, ours.code, tail(&gh.both(), 2), tail(&ours.both(), 2)));
            continue;
        }
        if gh.code != Some(0) {
            report.push(format!("{line}\n  都失败（退出码 {:?}）\n  gh: {}\n  内置: {}", gh.code, tail(&gh.stderr, 1), tail(&ours.stderr, 1)));
            continue;
        }
        if args[0] == "auth" {
            report.push(format!("{line}\n  都成功"));
            continue;
        }
        let a: Value = serde_json::from_str(&gh.stdout).unwrap_or(Value::Null);
        let b: Value = serde_json::from_str(&ours.stdout).unwrap_or(Value::Null);
        let mut diffs = Vec::new();
        json_diff("", &a, &b, &mut diffs);
        let checks = a.get("statusCheckRollup").and_then(|c| c.as_array()).map(|c| c.len());
        let parsed_same = args[0] == "pr" && args.iter().any(|x| x.contains("statusCheckRollup")) && parse_pr_full(&gh.stdout) == parse_pr_full(&ours.stdout);
        report.push(format!(
            "{line}\n  逐字段差异 {} 处{}{}{}",
            diffs.len(),
            checks.map(|n| format!("；检查 {n} 项")).unwrap_or_default(),
            if parsed_same { "；forge 解析结果相同" } else { "" },
            diffs.iter().take(25).map(|d| format!("\n    {d}")).collect::<String>()
        ));
    }
    println!("\n===== G7 真机对照 =====\n{}", report.join("\n"));
    // 令牌不能出现在报告里。
    assert!(!report.join("").contains(token.expose()));
}

#[test]
fn pr_ready_草稿转可评审_已就绪不再发变更() {
    let prefs = std::env::temp_dir().join("meowo-builtin-none.json");
    let mutations = AtomicUsize::new(0);
    let api = Api::new(move |_, _, body| {
        let q = query_of(body);
        if q.contains("markPullRequestReadyForReview") {
            mutations.fetch_add(1, Ordering::SeqCst);
            assert_eq!(body["variables"]["input"], json!({ "pullRequestId": "PR_5" }));
            return data(json!({ "markPullRequestReadyForReview": { "pullRequest": { "isDraft": false } } }));
        }
        let draft = body["variables"]["number"] == 5;
        data(json!({ "repository": { "pullRequest": { "id": "PR_5", "isDraft": draft } } }))
    });
    let out = run(&api, &prefs, ".", &["pr", "ready", "5", "--repo", "acme/repo"]);
    assert_eq!(out.code, Some(0));
    assert!(out.stdout.contains("ready for review"));
    let out = run(&api, &prefs, ".", &["pr", "ready", "6", "--repo", "acme/repo"]);
    assert!(out.code == Some(0) && out.stdout.contains("already"));
    assert_eq!(api.calls().iter().filter(|c| query_of(&c.2).contains("markPullRequestReadyForReview")).count(), 1);
}

// ═══ 真机写路径（G7，leader 授权）═══
//
// 默认 #[ignore]。只对临时分支操作：G7_BASE / G7_HEAD 是事先从 main 推上去的临时分支，G7_HEAD_SHA 是 head
// 的提交号。依次：建草稿 PR → 读回（与 gh 对照）→ 再建一次（已存在）→ 评论 → 转可评审 → 头提交不符的合并被拒
// → 正确合并（squash 进临时 base）→ 读回已合并（与 gh 对照）。令牌只从 GH_TOKEN 读，不打印。
#[test]
#[ignore]
fn live_写路径_g7() {
    let token = Token::new(&std::env::var("GH_TOKEN").expect("设 GH_TOKEN")).expect("令牌格式");
    let base = std::env::var("G7_BASE").expect("G7_BASE");
    let head = std::env::var("G7_HEAD").expect("G7_HEAD");
    let sha = std::env::var("G7_HEAD_SHA").expect("G7_HEAD_SHA");
    assert!(base.starts_with("meowo-g7-base-") && head.starts_with("meowo-g7-head-"), "只许对临时分支操作");
    let slug = "larrygogo/meowo";
    let prefs = std::env::temp_dir().join("meowo-live-none.json");
    let builtin = Builtin { http: &crate::github::RealHttp, token: Some(&token), on_auth: &|_| {}, prefs: &prefs, ends: crate::forge::builtin::Endpoints::github_com() };
    let dir = std::env::temp_dir();
    let dir = dir.to_string_lossy();
    let ours = |args: &[&str]| with_retry(|| Some(builtin.run(&dir, args)));
    let gh = |args: &[&str]| with_retry(|| real_gh(&dir, args));
    let show = |step: &str, out: &CmdOut| println!("[{step}] 退出码 {:?}\n  stdout: {}\n  stderr: {}", out.code, tail(&out.stdout, 3), tail(&out.stderr, 3));
    let compare = |step: &str, number: &str| {
        let a = gh(&["pr", "view", number, "--repo", slug, "--json", PR_FIELDS]);
        let b = ours(&["pr", "view", number, "--repo", slug, "--json", PR_FIELDS]);
        let mut diffs = Vec::new();
        json_diff("", &serde_json::from_str(&a.stdout).unwrap_or(Value::Null), &serde_json::from_str(&b.stdout).unwrap_or(Value::Null), &mut diffs);
        println!("[{step}] pr view 与 gh 逐字段差异 {} 处{}", diffs.len(), diffs.iter().take(10).map(|d| format!("\n    {d}")).collect::<String>());
        parse_pr_full(&b.stdout).expect("forge 能解析")
    };

    let flaky = |out: &CmdOut| out.both().contains("error connecting") || out.both().contains("EOF") || out.both().contains("TLS") || out.timed_out;
    // 写操作只发一次；传输失败（回包可能丢了）先读回确认有没有生效，确认没生效才重发。
    let write = |step: &str, args: &[&str], took_effect: &dyn Fn() -> bool| -> CmdOut {
        for attempt in 0..6 {
            let out = builtin.run(&dir, args);
            if !flaky(&out) {
                show(step, &out);
                return out;
            }
            println!("[{step}] 第 {} 次传输失败：{}", attempt + 1, tail(&out.both(), 1));
            if took_effect() {
                println!("[{step}] 读回确认已生效（回包丢失）");
                return CmdOut { code: Some(0), stdout: "（回包丢失，读回确认已生效）".into(), ..Default::default() };
            }
        }
        panic!("[{step}] 一直连不上");
    };
    let find_pr = || {
        let out = ours(&["pr", "view", &head, "--repo", slug, "--json", "number,url"]);
        (out.code == Some(0)).then(|| serde_json::from_str::<Value>(&out.stdout).unwrap())
    };

    let title = format!("Meowo G7 自测，可忽略（{head}）");
    let create = ["pr", "create", "--title", &title, "--body", "Meowo 内置 gh 写路径验证（G7），由临时分支发起，验证后合进临时分支并删除。", "--base", &base, "--head", &head, "--draft", "--repo", slug];
    let out = write("pr create --draft", &create, &|| find_pr().is_some());
    assert_eq!(out.code, Some(0));
    let found = find_pr().expect("建好的 PR 读得回");
    let url = found["url"].as_str().unwrap().to_string();
    let number = found["number"].to_string();
    println!("PR {url}");
    if out.stdout.starts_with("https://") {
        assert_eq!(out.stdout.trim(), url, "pr create 输出的就是 PR 链接（同 gh）");
    }

    let pr = compare("读回草稿", &number);
    assert_eq!((pr.state.as_str(), pr.head_oid.as_deref()), ("draft", Some(sha.as_str())));

    let again = write("再建一次（已存在）", &create, &|| false);
    assert_eq!(again.code, Some(1));
    assert!(again.both().contains("already exists") && again.both().contains(&url), "forge 要从这里抠出链接");
    println!("  forge 归类：gh/pr_exists 抠出的链接 = {}", again.both().lines().map(str::trim).find(|l| l.starts_with("https://")).unwrap_or_default());

    let marker = "Meowo G7 自测评论（内置执行器）";
    let has_comment = || ours(&["pr", "view", &number, "--repo", slug, "--json", "comments"]).stdout.contains(marker);
    let out = write("pr comment", &["pr", "comment", &number, "--repo", slug, "--body", marker], &has_comment);
    assert_eq!(out.code, Some(0));
    assert!(has_comment(), "评论读得回");
    let gh_comments = gh(&["pr", "view", &number, "--repo", slug, "--json", "comments"]);
    assert!(gh_comments.stdout.contains(marker), "gh 也看得到这条评论");

    let is_ready = || ours(&["pr", "view", &number, "--repo", slug, "--json", "isDraft"]).stdout.contains("\"isDraft\":false");
    let out = write("pr ready", &["pr", "ready", &number, "--repo", slug], &is_ready);
    assert_eq!(out.code, Some(0));
    assert!(is_ready());

    let wrong = "0000000000000000000000000000000000000000";
    let out = write("头提交不符的合并", &["pr", "merge", &number, "--squash", "--match-head-commit", wrong, "--repo", slug], &|| false);
    assert_eq!(out.code, Some(1));
    println!("  forge 归类：{}", classify_gh_failure(&out, None));

    let is_merged = || ours(&["pr", "view", &number, "--repo", slug, "--json", "state"]).stdout.contains("MERGED");
    // GitHub 刚转可评审时可能还在算能不能合：「不可合并」时等一会儿再试。
    let mut merged = false;
    for _ in 0..10 {
        let out = write("pr merge --squash", &["pr", "merge", &number, "--squash", "--match-head-commit", &sha, "--repo", slug], &is_merged);
        if out.code == Some(0) {
            merged = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    assert!(merged, "合并没成功");
    std::thread::sleep(std::time::Duration::from_secs(2));
    let pr = compare("读回已合并", &number);
    assert_eq!(pr.state, "merged");    println!("G7_PR_NUMBER={number}");
}
