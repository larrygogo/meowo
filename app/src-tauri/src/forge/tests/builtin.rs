//! GitHub 的内置实现（forge/builtin.rs）：对 forge 用到的每个操作（内部照 gh 子命令组织），输出与 gh 同形。GitHub 是假的（按地址与
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
    // forge 的平台接口照常认出「没有 PR」。
    let forge = forge_on(&api, Some(token()), Box::new(|_| {}));
    assert_eq!(forge.find(".", Some("larrygogo/meowo-pr-drill"), &Target::Branch { qualified: "nope", branch: "nope", head_slug: None }), Ok(None));
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
    // worktree 清理的查法：经平台接口按当前分支查（github.com 账号 = 这个假 HTTP 上的内置实现）。
    let forges = ApiForges(&api);
    let caches = Caches::default();
    let ctx = Ctx { forges: &forges, caches: &caches, prefs: prefs.clone(), env: &[] };
    match platform_pr_lookup_with(&ctx, &s(&repo)) {
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
            assert_eq!(body, &json!({ "body": "hi there", "commit_id": "abc", "path": "src/a.ts", "line": 3, "side": "RIGHT" }));
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
    // 这里只测内置 `gh api` 把 -f / -F 字段原样转成请求体（参数是测试自己给的），抓不到 side 写错这类回退；
    // side 的 NEW/OLD → RIGHT/LEFT 映射由 forge/review.rs 的 writes_use_injected_gh_and_line_comment_pins_current_head 钉住。
    let out = run(&api, &prefs, ".", &["api", "repos/acme/repo/pulls/3/comments", "-f", "body=hi there", "-f", "commit_id=abc", "-f", "path=src/a.ts", "-F", "line=3", "-f", "side=RIGHT"]);
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

/// 按分支找 PR（feat，仓库 acme/repo）。
fn find_feat(forge: &dyn ForgeApi) -> Result<Option<PullRequestDto>, String> {
    forge.find(".", Some("acme/repo"), &Target::Branch { qualified: "feat", branch: "feat", head_slug: None })
}

fn forge_on<'a>(api: &'a Api, token: Option<Token>, on_auth: Box<dyn Fn(bool) + Sync + 'a>) -> super::super::api::GhForge<'a> {
    let state = if token.is_some() { GhState::Ok } else { GhState::Unauthed };
    crate::forge::builtin::forge(Box::new(api), crate::forge::builtin::Endpoints::github_com(), token, state, std::env::temp_dir().join("meowo-builtin-none.json"), on_auth)
}

#[test]
fn 令牌被拒_401_forge_认成没登录_通知重新登录_没令牌不发请求() {
    let api = Api::new(|_, _, _| resp(401, json!({ "message": "Bad credentials" })));
    let flagged = AtomicUsize::new(0);
    let forge = forge_on(&api, Some(token()), Box::new(|ok| {
        if !ok {
            flagged.fetch_add(1, Ordering::SeqCst);
        }
    }));
    let prefs = std::env::temp_dir().join("meowo-builtin-none.json");
    let out = run(&api, &prefs, ".", &["pr", "view", "feat", "--repo", "acme/repo", "--json", "number"]);
    assert_eq!(out.code, Some(1));
    assert!(says_unauthed(&out));
    assert!(!out.both().contains("gho_TESTTOKEN"), "错误输出里不能有令牌");
    assert_eq!(find_feat(&forge), Err("gh/not_authed".into()));
    assert_eq!(flagged.load(Ordering::SeqCst), 1, "401 报给账号，标「要重新登录」");
    // 没有令牌：不发请求，同样是没登录。
    let calls = api.calls().len();
    let none = forge_on(&api, None, Box::new(|_| {}));
    assert_eq!(find_feat(&none), Err("gh/not_authed".into()));
    assert_eq!(api.calls().len(), calls);
}

#[test]
fn 断网_按查询失败处理() {
    struct Down;
    impl Http for Down {
        fn send(&self, _: &Request) -> Result<Response, String> {
            Err("Connection refused".into())
        }
    }
    let forge = crate::forge::builtin::forge(
        Box::new(Down),
        crate::forge::builtin::Endpoints::github_com(),
        Some(token()),
        GhState::Ok,
        std::env::temp_dir().join("meowo-builtin-none.json"),
        Box::new(|_| {}),
    );
    let err = find_feat(&forge).unwrap_err();
    assert!(err.starts_with("gh/failed") && err.contains("error connecting"), "{err}");
}

/// github.com 账号 = 假 HTTP 上的内置实现（forge 全流程测试用）。
struct ApiForges<'a>(&'a Api);

impl Forges for ApiForges<'_> {
    fn platform(&self, host: &str) -> Option<Platform> {
        platform::public_platform(host)
    }
    fn open(&self, host: &str) -> Option<Box<dyn ForgeApi + '_>> {
        (platform::host_name(host) == platform::GITHUB_COM).then(|| Box::new(forge_on(self.0, Some(token()), Box::new(|_| {}))) as Box<dyn ForgeApi>)
    }
}

/// forge 的整条流程跑在内置实现上：「更改」面板状态 → 开 PR → 合并。
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
    let forges = ApiForges(&api);
    let caches = Caches::default();
    let ctx = Ctx { forges: &forges, caches: &caches, prefs: prefs.clone(), env: &[] };
    let dto = forge_status_with(&ctx, &s(&repo), false).unwrap();
    assert_eq!(dto.gh, "ok");
    assert_eq!(dto.merge_methods, ["merge", "squash"]);
    assert!(dto.pr.is_none() && dto.pr_defaults.is_some());
    let pushed = |_: &str| Ok(PushResultDto { remote: "origin".into(), branch: "meowo/drill".into(), set_upstream: true, pushed: 1 });
    let pr = pr_create_with(&ctx, &s(&repo), Some("T"), None, None, false, &pushed).unwrap();
    assert_eq!((pr.number, pr.state.as_str()), (12, "open"));
    let merged = pr_merge_with(&ctx, &s(&repo), 12, "squash", "1111111111111111111111111111111111111111").unwrap();
    assert!(merged.merged && !merged.queued);
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
/// 登录、添加、退出账号共用的缓存作废：合并方式、PR、入队标记都清掉（审查 #9）。
#[test]
fn 账号变了_forge_缓存全部作废() {
    let key = format!("meowo-test-invalidate-{}", std::process::id());
    lock(&CACHES.methods).insert(key.clone(), (Instant::now(), vec!["merge".into()]));
    lock(&CACHES.prs).insert(key.clone(), (Instant::now(), None));
    lock(&CACHES.queued).insert((key.clone(), 1, "sha".into()));
    invalidate_connection();
    assert!(!lock(&CACHES.methods).contains_key(&key));
    assert!(!lock(&CACHES.prs).contains_key(&key));
    assert!(!lock(&CACHES.queued).iter().any(|(k, _, _)| *k == key));
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
