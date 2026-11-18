//! Meowo 登录模式下的 gh 替身：对 forge 用到的 gh 子命令，直接调 GitHub REST / GraphQL，给出与 gh 同形
//! 的输出（`--json` 字段名、退出码、`no pull requests found` 等错误文案），上层 forge 代码不用改。
//!
//! 覆盖的命令（与 forge.rs / review.rs / badge.rs / worktree.rs 的调用一一对应）：
//! - `auth status --hostname github.com` → `GET /user`；
//! - `pr view [号|URL|分支|owner:分支] [--repo slug] --json <字段>` → 一次 GraphQL 取齐字段（检查、评论、
//!   评审超过 100 条时按游标续页）；
//! - `pr create --title --body --base --head [--draft] [--repo]` → `POST /repos/{slug}/pulls`；
//! - `pr merge <号> --merge|--squash|--rebase [--match-head-commit sha] --repo` → `PUT .../merge`
//!   （仓库要求合并队列时改用 GraphQL `enqueuePullRequest`）；`pr merge --help` 回带 `--match-head-commit` 的帮助；
//! - `pr comment` / `pr review --comment` → issue 评论 / COMMENT 评审；
//! - `repo view <slug> --json mergeCommitAllowed,...` → GraphQL；
//! - `api <endpoint> [--paginate] [--slurp] [-f k=v] [-F k=v] [-X 方法]` → REST（只认 api.github.com）。
//!
//! 401：回 `HTTP 401: Bad credentials`（forge 的 says_unauthed 照常认出「没登录」），并通知 github 模块标
//! 「需要重新登录」。断网：`error connecting to api.github.com`，forge 按查询失败处理、保留上一次的结果。

use super::{repo_head, CmdOut};
use crate::github::{Http, Request, Response, Token, API};
use serde_json::{json, Map, Value};
use std::path::Path;

/// 没登录时每条命令的回话。带 `HTTP 401`：forge 的 says_unauthed 据此归为「没登录」。
const NOT_SIGNED_IN: &str = "HTTP 401: not signed in to GitHub in Meowo\nSign in from Meowo Settings > GitHub.";
/// `gh pr merge --help` 的替身：forge 只看里面有没有 `--match-head-commit`（判断 gh 新旧）。
const MERGE_HELP: &str = "Merge a pull request on GitHub.\n\nFLAGS\n      --match-head-commit SHA   Commit SHA that the pull request head must match to allow merge\n  -m, --merge                Merge the commits with the base branch\n  -r, --rebase               Rebase the commits onto the base branch\n  -s, --squash               Squash the commits into one commit and merge it into the base branch\n";
/// gh 对没完成的检查给的零时间（见 forge/fixtures/derived_pr_in_progress.json）。
const ZERO_TIME: &str = "0001-01-01T00:00:00Z";
/// 一条连接最多续几页（每页 100）：防异常数据无限翻页。
const MAX_PAGES: usize = 20;

pub(crate) struct Builtin<'a> {
    pub(crate) http: &'a dyn Http,
    pub(crate) token: Option<&'a Token>,
    /// 每个回包报一次令牌好不好用：false = 401（被拒），true = 2xx。github 模块据此标 / 清「要重新登录」。
    pub(crate) on_auth: &'a (dyn Fn(bool) + Sync),
    /// forge.json（PR 所在远端的选择），没带 --repo 时按目录推仓库要用。
    pub(crate) prefs: &'a Path,
}

fn ok(stdout: impl Into<String>) -> CmdOut {
    CmdOut { code: Some(0), stdout: stdout.into(), stderr: String::new(), timed_out: false }
}

fn fail(stderr: impl Into<String>) -> CmdOut {
    CmdOut { code: Some(1), stdout: String::new(), stderr: stderr.into(), timed_out: false }
}

fn parse_json(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or(Value::Null)
}

/// 回包里的 `message`（REST 错误）；422 时把 `errors[].message` 也接上。
fn message_of(resp: &Response) -> String {
    let v = parse_json(&resp.body);
    let mut parts: Vec<String> = v.get("message").and_then(|m| m.as_str()).map(str::to_string).into_iter().collect();
    if let Some(errors) = v.get("errors").and_then(|e| e.as_array()) {
        parts.extend(errors.iter().filter_map(|e| e.get("message").and_then(|m| m.as_str()).or_else(|| e.as_str())).map(str::to_string));
    }
    if parts.is_empty() {
        resp.body.lines().next().unwrap_or_default().chars().take(200).collect()
    } else {
        parts.join("; ")
    }
}

/// 非 2xx 的统一失败：`gh: <原因> (HTTP n)`，与 gh api 的写法一致。
fn http_fail(resp: &Response) -> CmdOut {
    fail(format!("gh: {} (HTTP {})", message_of(resp), resp.status))
}

fn success(resp: &Response) -> bool {
    (200..300).contains(&resp.status)
}

// ═══ 参数 ═══

const VALUED: &[&str] = &[
    "--repo", "-R", "--json", "--title", "-t", "--body", "-b", "--base", "-B", "--head", "-H", "--match-head-commit", "-f", "--raw-field",
    "-F", "--field", "-X", "--method", "--hostname", "--header",
];

#[derive(Default)]
struct Args<'a> {
    positional: Vec<&'a str>,
    flags: Vec<(&'a str, Option<&'a str>)>,
}

impl<'a> Args<'a> {
    fn parse(args: &[&'a str]) -> Self {
        let mut out = Args::default();
        let mut i = 0;
        while i < args.len() {
            let arg = args[i];
            if let Some((name, value)) = arg.split_once('=').filter(|(name, _)| name.starts_with("--")) {
                out.flags.push((name, Some(value)));
            } else if VALUED.contains(&arg) {
                out.flags.push((arg, args.get(i + 1).copied()));
                i += 1;
            } else if arg.starts_with('-') && arg.len() > 1 {
                out.flags.push((arg, None));
            } else {
                out.positional.push(arg);
            }
            i += 1;
        }
        out
    }
    fn get(&self, names: &[&str]) -> Option<&'a str> {
        self.flags.iter().rev().find(|(n, _)| names.contains(n)).and_then(|(_, v)| *v)
    }
    fn all(&self, names: &[&str]) -> Vec<&'a str> {
        self.flags.iter().filter(|(n, _)| names.contains(n)).filter_map(|(_, v)| *v).collect()
    }
    fn has(&self, names: &[&str]) -> bool {
        self.flags.iter().any(|(n, _)| names.contains(n))
    }
    fn fields(&self) -> Vec<&'a str> {
        self.get(&["--json"]).map(|f| f.split(',').map(str::trim).filter(|f| !f.is_empty()).collect()).unwrap_or_default()
    }
}

/// owner/repo（只收 GitHub 允许的字符，拼进 URL 与 GraphQL 变量都安全）。
fn split_slug(slug: &str) -> Option<(&str, &str)> {
    let (owner, name) = slug.split_once('/')?;
    let valid = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    (valid(owner) && valid(name)).then_some((owner, name))
}

/// `https://github.com/<owner>/<repo>/pull/<号>` → (slug, 号)。别的主机不认（Meowo 登录只连 github.com）。
fn parse_pr_url(url: &str) -> Option<(String, u64)> {
    let rest = url.strip_prefix("https://github.com/").or_else(|| url.strip_prefix("https://www.github.com/"))?;
    let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    if parts.len() < 4 || parts[2] != "pull" {
        return None;
    }
    let slug = format!("{}/{}", parts[0], parts[1]);
    split_slug(&slug)?;
    Some((slug, parts[3].parse().ok()?))
}

fn encode(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

/// `Link: <https://…?page=2>; rel="next", <…>; rel="last"` → next 的地址。
fn next_link(link: Option<&str>) -> Option<String> {
    link?.split(',').find_map(|part| {
        let (url, rel) = part.split_once(';')?;
        rel.contains("rel=\"next\"").then(|| url.trim().trim_start_matches('<').trim_end_matches('>').to_string())
    })
}

// ═══ pr view 的字段 ═══

/// 一条可续页的连接：字段名、在 PR 节点里的 JSON 指针、选择集（`ARGS` 处填分页参数）。
struct Connection {
    field: &'static str,
    pointer: &'static str,
    template: &'static str,
}

const CONNECTIONS: &[Connection] = &[
    Connection {
        field: "statusCheckRollup",
        pointer: "/commits/nodes/0/commit/statusCheckRollup/contexts",
        template: "commits(last:1){nodes{commit{statusCheckRollup{contexts(ARGS){nodes{__typename ...on CheckRun{name status conclusion detailsUrl startedAt completedAt checkSuite{workflowRun{workflow{name}}}} ...on StatusContext{context state targetUrl createdAt}} pageInfo{hasNextPage endCursor}}}}}}",
    },
    Connection {
        field: "comments",
        pointer: "/comments",
        template: "comments(ARGS){nodes{id author{login} authorAssociation body createdAt url} pageInfo{hasNextPage endCursor}}",
    },
    Connection {
        field: "reviews",
        pointer: "/reviews",
        template: "reviews(ARGS){nodes{id author{login} authorAssociation body state submittedAt} pageInfo{hasNextPage endCursor}}",
    },
];

/// 与 GraphQL 同名的标量字段（gh 的 `--json` 字段名就是 GraphQL 字段名）。
const SCALARS: &[&str] = &[
    "number", "state", "isDraft", "url", "title", "body", "baseRefName", "headRefName", "headRefOid", "mergeable", "mergeStateStatus",
    "createdAt", "updatedAt", "closedAt", "mergedAt", "closed", "additions", "deletions", "changedFiles", "id", "isCrossRepository",
];

/// `--json` 字段 → GraphQL 选择集。另外总带上分页与挑 PR 要用的内部字段（meowo 前缀的别名，不进输出）。
fn pr_selection(fields: &[&str]) -> Result<String, String> {
    let mut parts = vec!["meowoId: id".to_string(), "meowoState: state".into(), "meowoCross: isCrossRepository".into(), "meowoOwner: headRepositoryOwner{login}".into(), "meowoHead: headRefName".into()];
    for field in fields {
        if SCALARS.contains(field) {
            parts.push(field.to_string());
        } else if *field == "author" {
            parts.push("author{login}".into());
        } else if let Some(conn) = CONNECTIONS.iter().find(|c| c.field == *field) {
            parts.push(conn.template.replace("ARGS", "first:100"));
        } else {
            return Err(format!("Unknown JSON field: \"{field}\""));
        }
    }
    Ok(parts.join(" "))
}

fn nodes_at<'v>(node: &'v Value, pointer: &str) -> Vec<&'v Value> {
    node.pointer(&format!("{pointer}/nodes")).and_then(|v| v.as_array()).map(|a| a.iter().collect()).unwrap_or_default()
}

fn str_or<'v>(v: &'v Value, pointer: &str, fallback: &'v str) -> &'v str {
    v.pointer(pointer).and_then(|x| x.as_str()).unwrap_or(fallback)
}

/// GraphQL 的检查节点 → gh `statusCheckRollup` 的一项（字段与 forge/fixtures 的实录一致）。
fn rollup_item(item: &Value) -> Value {
    if item.get("__typename").and_then(|t| t.as_str()) == Some("StatusContext") {
        return json!({
            "__typename": "StatusContext",
            "context": str_or(item, "/context", ""),
            "startedAt": str_or(item, "/createdAt", ZERO_TIME),
            "state": str_or(item, "/state", ""),
            "targetUrl": str_or(item, "/targetUrl", ""),
        });
    }
    json!({
        "__typename": "CheckRun",
        "completedAt": str_or(item, "/completedAt", ZERO_TIME),
        "conclusion": str_or(item, "/conclusion", ""),
        "detailsUrl": str_or(item, "/detailsUrl", ""),
        "name": str_or(item, "/name", ""),
        "startedAt": str_or(item, "/startedAt", ZERO_TIME),
        "status": str_or(item, "/status", ""),
        "workflowName": str_or(item, "/checkSuite/workflowRun/workflow/name", ""),
    })
}

/// PR 节点 → gh `pr view --json <字段>` 的输出对象（只含要的字段）。
fn gh_pr_json(node: &Value, fields: &[&str]) -> Value {
    let mut out = Map::new();
    for field in fields {
        let value = match *field {
            "statusCheckRollup" => Value::Array(nodes_at(node, CONNECTIONS[0].pointer).into_iter().map(rollup_item).collect()),
            "comments" | "reviews" => Value::Array(nodes_at(node, &format!("/{field}")).into_iter().cloned().collect()),
            other => node.get(other).cloned().unwrap_or(Value::Null),
        };
        out.insert(field.to_string(), value);
    }
    Value::Object(out)
}

/// gh 按分支找 PR 的挑法，逐条照 gh 2.96 的 pkg/cmd/pr/shared/finder.go `findForRefs`（403–441 行）：
/// - 查同名 head 分支最近的 30 条（按创建时间倒序，不限状态）；
/// - 开着的稳定排到前面；
/// - PR 的 head 写法（fork 是 `owner:分支`，同仓库只是分支名，见 api/queries_pr.go `HeadLabel`）要与要找的
///   写法完全相等（find_refs_resolution.go `Matches`）；
/// - 要找的分支就是仓库默认分支时，只认开着的（gh issue #4263：默认分支上的已合并 / 已关闭 PR 没意义）。
fn pick_branch_pr<'v>(nodes: &'v [Value], qualified: &str, default_branch: Option<&str>) -> Option<&'v Value> {
    let label = |n: &Value| {
        let head = str_or(n, "/meowoHead", "");
        if n.get("meowoCross").and_then(|c| c.as_bool()).unwrap_or(false) {
            format!("{}:{head}", str_or(n, "/meowoOwner/login", ""))
        } else {
            head.to_string()
        }
    };
    let open = |n: &Value| str_or(n, "/meowoState", "") == "OPEN";
    let mut sorted: Vec<&Value> = nodes.iter().collect();
    sorted.sort_by_key(|n| !open(n));
    sorted.into_iter().find(|n| label(n) == qualified && (open(n) || default_branch != Some(qualified)))
}

// ═══ 执行 ═══

enum Target {
    Number(u64),
    Branch(String),
}

impl Builtin<'_> {
    pub(crate) fn run(&self, dir: &str, args: &[&str]) -> CmdOut {
        if self.token.is_none() {
            return fail(NOT_SIGNED_IN);
        }
        let result = match args {
            ["auth", "status", ..] => self.auth_status(),
            ["pr", "merge", "--help"] => Ok(ok(MERGE_HELP)),
            ["pr", "view", rest @ ..] => self.pr_view(dir, rest),
            ["pr", "create", rest @ ..] => self.pr_create(dir, rest),
            ["pr", "merge", rest @ ..] => self.pr_merge(dir, rest),
            ["pr", "comment", rest @ ..] => self.pr_comment(dir, rest, false),
            ["pr", "review", rest @ ..] => self.pr_comment(dir, rest, true),
            ["repo", "view", rest @ ..] => self.repo_view(dir, rest),
            ["api", rest @ ..] => self.api(rest),
            _ => Err(fail(format!("meowo: unsupported gh command: {}", args.iter().take(2).copied().collect::<Vec<_>>().join(" ")))),
        };
        result.unwrap_or_else(|out| out)
    }

    fn send(&self, method: &str, url: &str, body: Option<Value>) -> Result<Response, CmdOut> {
        let token = self.token.ok_or_else(|| fail(NOT_SIGNED_IN))?;
        let req = Request { method, url, token: Some(token), body: body.map(crate::github::Body::Json) };
        let resp = self.http.send(&req).map_err(|e| fail(format!("error connecting to api.github.com\n{e}")))?;
        if resp.status == 401 {
            (self.on_auth)(false);
            return Err(fail("HTTP 401: Bad credentials (https://api.github.com)\nSign in to GitHub again from Meowo Settings > GitHub."));
        }
        if success(&resp) {
            (self.on_auth)(true);
        }
        Ok(resp)
    }

    fn rest(&self, method: &str, path: &str, body: Option<Value>) -> Result<Response, CmdOut> {
        self.send(method, &format!("{API}/{}", path.trim_start_matches('/')), body)
    }

    /// GraphQL：有 errors 就按 gh 的写法失败（`GraphQL: <原因>`），否则回 data。
    fn graphql(&self, query: &str, variables: Value) -> Result<Value, CmdOut> {
        let resp = self.rest("POST", "graphql", Some(json!({ "query": query, "variables": variables })))?;
        if !success(&resp) {
            return Err(http_fail(&resp));
        }
        let v = parse_json(&resp.body);
        if let Some(errors) = v.get("errors").and_then(|e| e.as_array()).filter(|e| !e.is_empty()) {
            let text: Vec<&str> = errors.iter().filter_map(|e| e.get("message").and_then(|m| m.as_str())).collect();
            return Err(fail(format!("GraphQL: {}", text.join(", "))));
        }
        Ok(v.get("data").cloned().unwrap_or(Value::Null))
    }

    /// 没带 --repo 时按目录认仓库（与 forge 同一个判定：gh-resolved、用户选的远端、唯一的 GitHub 远端）。
    /// 回 (slug, 当前分支的 head 写法)。
    fn local_repo(&self, dir: &str) -> Option<(String, Option<String>)> {
        let (info, _, _) = repo_head(dir, &[], self.prefs, false)?;
        Some((info.gh.slug()?.to_string(), info.head_ref()))
    }

    fn repo_of(&self, dir: &str, args: &Args) -> Result<String, CmdOut> {
        if let Some(slug) = args.get(&["--repo", "-R"]) {
            return Ok(slug.to_string());
        }
        self.local_repo(dir)
            .map(|(slug, _)| slug)
            .ok_or_else(|| fail("none of the git remotes configured for this repository point to a known GitHub host"))
    }

    fn auth_status(&self) -> Result<CmdOut, CmdOut> {
        let resp = self.rest("GET", "user", None)?;
        if !success(&resp) {
            return Err(http_fail(&resp));
        }
        let login = parse_json(&resp.body).get("login").and_then(|l| l.as_str()).unwrap_or_default().to_string();
        Ok(ok(format!("github.com\n  ✓ Logged in to github.com account {login} (Meowo)\n")))
    }

    fn pr_view(&self, dir: &str, rest: &[&str]) -> Result<CmdOut, CmdOut> {
        let args = Args::parse(rest);
        let fields = args.fields();
        if fields.is_empty() {
            return Err(fail("Specify one or more comma-separated fields for `--json`"));
        }
        let selection = pr_selection(&fields).map_err(fail)?;
        let (slug, target) = match args.positional.first().copied() {
            Some(url) if url.starts_with("https://") => {
                let (slug, number) = parse_pr_url(url).ok_or_else(|| fail(format!("invalid pull request URL: {url}")))?;
                (slug, Target::Number(number))
            }
            Some(number) if !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) => {
                (self.repo_of(dir, &args)?, Target::Number(number.parse().map_err(|_| fail("invalid number"))?))
            }
            Some(branch) => (self.repo_of(dir, &args)?, Target::Branch(branch.to_string())),
            None => {
                let (slug, head) = match args.get(&["--repo", "-R"]) {
                    Some(slug) => (slug.to_string(), self.local_repo(dir).and_then(|(_, head)| head)),
                    None => self.local_repo(dir).ok_or_else(|| fail("none of the git remotes configured for this repository point to a known GitHub host"))?,
                };
                (slug, Target::Branch(head.ok_or_else(|| fail("could not determine current branch: not on any branch"))?))
            }
        };
        let (owner, name) = split_slug(&slug).ok_or_else(|| fail(format!("invalid repository: {slug}")))?;
        let node = match &target {
            Target::Number(number) => {
                let query = format!("query($owner:String!,$name:String!,$number:Int!){{repository(owner:$owner,name:$name){{pullRequest(number:$number){{{selection}}}}}}}");
                let data = self.graphql(&query, json!({ "owner": owner, "name": name, "number": number }))?;
                data.pointer("/repository/pullRequest")
                    .filter(|v| !v.is_null())
                    .cloned()
                    .ok_or_else(|| fail(format!("GraphQL: Could not resolve to a PullRequest with the number of {number}. (repository.pullRequest)")))?
            }
            Target::Branch(branch) => {
                // gh 的 ParseQualifiedHeadRef：`owner:分支` 拆开，查询只用分支名。
                let head = branch.split_once(':').map_or(branch.as_str(), |(_, b)| b);
                let query = format!("query($owner:String!,$name:String!,$head:String!){{repository(owner:$owner,name:$name){{pullRequests(headRefName:$head,first:30,orderBy:{{field:CREATED_AT,direction:DESC}}){{nodes{{{selection}}}}} defaultBranchRef{{name}}}}}}");
                let data = self.graphql(&query, json!({ "owner": owner, "name": name, "head": head }))?;
                let nodes = data.pointer("/repository/pullRequests/nodes").and_then(|n| n.as_array()).cloned().unwrap_or_default();
                let default_branch = data.pointer("/repository/defaultBranchRef/name").and_then(|n| n.as_str());
                pick_branch_pr(&nodes, branch, default_branch).cloned().ok_or_else(|| fail(format!("no pull requests found for branch \"{branch}\"")))?
            }
        };
        let node = self.more_pages(node, &fields)?;
        Ok(ok(format!("{}\n", gh_pr_json(&node, &fields))))
    }

    /// 检查、评论、评审超过一页时按游标续页，拼回同一个节点里（gh 也是取全的）。
    fn more_pages(&self, mut node: Value, fields: &[&str]) -> Result<Value, CmdOut> {
        let id = node.get("meowoId").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        for conn in CONNECTIONS.iter().filter(|c| fields.contains(&c.field)) {
            for _ in 0..MAX_PAGES {
                let Some(info) = node.pointer(&format!("{}/pageInfo", conn.pointer)) else { break };
                let cursor = match (info.get("hasNextPage").and_then(|v| v.as_bool()), info.get("endCursor").and_then(|v| v.as_str())) {
                    (Some(true), Some(cursor)) if !id.is_empty() => cursor.to_string(),
                    _ => break,
                };
                let query = format!(
                    "query($id:ID!,$after:String!){{node(id:$id){{...on PullRequest{{{}}}}}}}",
                    conn.template.replace("ARGS", "first:100,after:$after")
                );
                let data = self.graphql(&query, json!({ "id": id, "after": cursor }))?;
                let Some(page) = data.pointer(&format!("/node{}", conn.pointer)).cloned() else { break };
                let Some(target) = node.pointer_mut(conn.pointer) else { break };
                if let (Some(list), Some(more)) = (target.get_mut("nodes").and_then(|n| n.as_array_mut()), page.get("nodes").and_then(|n| n.as_array())) {
                    list.extend(more.iter().cloned());
                }
                target["pageInfo"] = page.get("pageInfo").cloned().unwrap_or(Value::Null);
            }
        }
        Ok(node)
    }

    fn pr_create(&self, dir: &str, rest: &[&str]) -> Result<CmdOut, CmdOut> {
        let args = Args::parse(rest);
        let slug = self.repo_of(dir, &args)?;
        split_slug(&slug).ok_or_else(|| fail(format!("invalid repository: {slug}")))?;
        let head = match args.get(&["--head", "-H"]) {
            Some(head) => head.to_string(),
            None => self.local_repo(dir).and_then(|(_, head)| head).ok_or_else(|| fail("could not determine the current branch"))?,
        };
        let base = args.get(&["--base", "-B"]).ok_or_else(|| fail("--base is required"))?.to_string();
        let body = json!({
            "title": args.get(&["--title", "-t"]).unwrap_or_default(),
            "body": args.get(&["--body", "-b"]).unwrap_or_default(),
            "base": base,
            "head": head,
            "draft": args.has(&["--draft", "-d"]),
        });
        let resp = self.rest("POST", &format!("repos/{slug}/pulls"), Some(body))?;
        if success(&resp) {
            let url = parse_json(&resp.body).get("html_url").and_then(|u| u.as_str()).unwrap_or_default().to_string();
            return Ok(ok(format!("{url}\n")));
        }
        let message = message_of(&resp);
        if resp.status == 422 && message.contains("already exists") {
            // REST 不给已有 PR 的链接：按 head + base 查一下，按 gh 的原话回（forge 从里面抠 https:// 那行）。
            let owner = slug.split('/').next().unwrap_or_default();
            let qualified = if head.contains(':') { head.clone() } else { format!("{owner}:{head}") };
            let url = self
                .rest("GET", &format!("repos/{slug}/pulls?state=open&head={}&base={}", encode(&qualified), encode(&base)), None)
                .ok()
                .filter(success)
                .and_then(|r| parse_json(&r.body).get(0).and_then(|p| p.get("html_url")).and_then(|u| u.as_str()).map(str::to_string))
                .unwrap_or_default();
            return Err(fail(format!("a pull request for branch \"{head}\" into branch \"{base}\" already exists:\n{url}")));
        }
        Err(fail(format!("pull request create failed: {message} (HTTP {})", resp.status)))
    }

    fn pr_merge(&self, dir: &str, rest: &[&str]) -> Result<CmdOut, CmdOut> {
        let args = Args::parse(rest);
        let number: u64 = args.positional.first().and_then(|n| n.parse().ok()).ok_or_else(|| fail("pull request number required"))?;
        let slug = self.repo_of(dir, &args)?;
        let (owner, name) = split_slug(&slug).ok_or_else(|| fail(format!("invalid repository: {slug}")))?;
        let method = if args.has(&["--squash", "-s"]) {
            "squash"
        } else if args.has(&["--rebase", "-r"]) {
            "rebase"
        } else {
            "merge"
        };
        let sha = args.get(&["--match-head-commit"]);
        let mut body = json!({ "merge_method": method });
        if let Some(sha) = sha {
            body["sha"] = json!(sha);
        }
        let resp = self.rest("PUT", &format!("repos/{slug}/pulls/{number}/merge"), Some(body))?;
        if success(&resp) {
            return Ok(ok(format!("✓ Merged pull request #{number}\n")));
        }
        let message = message_of(&resp);
        if !message.to_lowercase().contains("merge queue") {
            return Err(fail(format!("{message} (HTTP {})", resp.status)));
        }
        // 仓库要求合并队列：与 gh 一样改为入队（头提交照样钉住）。
        let data = self.graphql(
            "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){pullRequest(number:$number){id baseRefName}}}",
            json!({ "owner": owner, "name": name, "number": number }),
        )?;
        let id = str_or(&data, "/repository/pullRequest/id", "").to_string();
        let base = str_or(&data, "/repository/pullRequest/baseRefName", "").to_string();
        let mut input = json!({ "pullRequestId": id });
        if let Some(sha) = sha {
            input["expectedHeadOid"] = json!(sha);
        }
        self.graphql("mutation($input:EnqueuePullRequestInput!){enqueuePullRequest(input:$input){mergeQueueEntry{id}}}", json!({ "input": input }))?;
        Ok(ok(format!("! Pull request #{number} will be added to the merge queue for {base} when ready\n")))
    }

    fn pr_comment(&self, dir: &str, rest: &[&str], review: bool) -> Result<CmdOut, CmdOut> {
        let args = Args::parse(rest);
        let number: u64 = args.positional.first().and_then(|n| n.parse().ok()).ok_or_else(|| fail("pull request number required"))?;
        let slug = self.repo_of(dir, &args)?;
        split_slug(&slug).ok_or_else(|| fail(format!("invalid repository: {slug}")))?;
        let text = args.get(&["--body", "-b"]).ok_or_else(|| fail("--body is required"))?;
        let resp = if review {
            self.rest("POST", &format!("repos/{slug}/pulls/{number}/reviews"), Some(json!({ "body": text, "event": "COMMENT" })))?
        } else {
            self.rest("POST", &format!("repos/{slug}/issues/{number}/comments"), Some(json!({ "body": text })))?
        };
        if !success(&resp) {
            return Err(http_fail(&resp));
        }
        let url = parse_json(&resp.body).get("html_url").and_then(|u| u.as_str()).unwrap_or_default().to_string();
        Ok(ok(format!("{url}\n")))
    }

    fn repo_view(&self, dir: &str, rest: &[&str]) -> Result<CmdOut, CmdOut> {
        const REPO_FIELDS: &[&str] = &[
            "mergeCommitAllowed", "squashMergeAllowed", "rebaseMergeAllowed", "nameWithOwner", "url", "isPrivate", "deleteBranchOnMerge", "viewerPermission",
        ];
        let args = Args::parse(rest);
        let slug = match args.positional.first() {
            Some(slug) => slug.to_string(),
            None => self.repo_of(dir, &args)?,
        };
        let (owner, name) = split_slug(&slug).ok_or_else(|| fail(format!("invalid repository: {slug}")))?;
        let fields = args.fields();
        if let Some(bad) = fields.iter().find(|f| !REPO_FIELDS.contains(f)) {
            return Err(fail(format!("Unknown JSON field: \"{bad}\"")));
        }
        if fields.is_empty() {
            return Err(fail("Specify one or more comma-separated fields for `--json`"));
        }
        let query = format!("query($owner:String!,$name:String!){{repository(owner:$owner,name:$name){{{}}}}}", fields.join(" "));
        let data = self.graphql(&query, json!({ "owner": owner, "name": name }))?;
        let repo = data.get("repository").cloned().unwrap_or(Value::Null);
        let out: Map<String, Value> = fields.iter().map(|f| (f.to_string(), repo.get(*f).cloned().unwrap_or(Value::Null))).collect();
        Ok(ok(format!("{}\n", Value::Object(out))))
    }

    fn api(&self, rest: &[&str]) -> Result<CmdOut, CmdOut> {
        let args = Args::parse(rest);
        let endpoint = args.positional.first().copied().ok_or_else(|| fail("endpoint required"))?;
        let mut fields = Map::new();
        for pair in args.all(&["-f", "--raw-field"]) {
            let (k, v) = pair.split_once('=').ok_or_else(|| fail(format!("invalid field: {pair}")))?;
            fields.insert(k.to_string(), json!(v));
        }
        for pair in args.all(&["-F", "--field"]) {
            let (k, v) = pair.split_once('=').ok_or_else(|| fail(format!("invalid field: {pair}")))?;
            // gh 的 -F：true/false/null/整数按类型传；@文件 不支持（forge 不用，也不许读宿主文件）。
            let value = match v {
                "true" => json!(true),
                "false" => json!(false),
                "null" => Value::Null,
                _ if v.starts_with('@') => return Err(fail("meowo: reading field values from files is not supported")),
                _ => v.parse::<i64>().map(|n| json!(n)).unwrap_or_else(|_| json!(v)),
            };
            fields.insert(k.to_string(), value);
        }
        let method = args.get(&["-X", "--method"]).map(str::to_uppercase).unwrap_or_else(|| if fields.is_empty() { "GET".into() } else { "POST".into() });
        let mut url = if endpoint.starts_with("https://") {
            if !endpoint.starts_with(&format!("{API}/")) {
                return Err(fail("meowo: only api.github.com is supported"));
            }
            endpoint.to_string()
        } else {
            format!("{API}/{}", endpoint.trim_start_matches('/'))
        };
        let body = if method == "GET" {
            if !fields.is_empty() {
                let query: Vec<String> = fields
                    .iter()
                    .map(|(k, v)| format!("{}={}", encode(k), encode(&v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))))
                    .collect();
                url = format!("{url}{}{}", if url.contains('?') { "&" } else { "?" }, query.join("&"));
            }
            None
        } else {
            Some(Value::Object(fields))
        };
        let paginate = args.has(&["--paginate"]) && method == "GET";
        let mut pages: Vec<Value> = Vec::new();
        let mut next = Some(url);
        let mut first_body = String::new();
        while let Some(current) = next.take() {
            let resp = self.send(&method, &current, body.clone())?;
            if !success(&resp) {
                return Err(CmdOut { stdout: resp.body.clone(), ..http_fail(&resp) });
            }
            if !paginate {
                first_body = resp.body;
                break;
            }
            pages.push(parse_json(&resp.body));
            next = next_link(resp.link.as_deref()).filter(|n| n.starts_with(&format!("{API}/")) && pages.len() < 100);
        }
        if !paginate {
            return Ok(ok(first_body));
        }
        let out = if args.has(&["--slurp"]) {
            Value::Array(pages)
        } else if pages.iter().all(|p| p.is_array()) {
            Value::Array(pages.into_iter().flat_map(|p| p.as_array().cloned().unwrap_or_default()).collect())
        } else {
            return Ok(ok(pages.iter().map(|p| p.to_string()).collect::<Vec<_>>().join("\n")));
        };
        Ok(ok(out.to_string()))
    }
}

/// 生产入口：真 HTTP、真 forge.json，401 时通知 github 模块。
pub(crate) fn run_real(dir: &str, args: &[&str], token: Option<&Token>) -> CmdOut {
    let prefs = super::prefs_path();
    let on_auth = |ok: bool| {
        if let Some(token) = token {
            crate::github::auth_result(token, ok);
        }
    };
    Builtin { http: &crate::github::RealHttp, token, on_auth: &on_auth, prefs: &prefs }.run(dir, args)
}
