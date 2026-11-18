//! Gitee（gitee.com，码云，API v5 `https://gitee.com/api/v5`）的 Pull Request。Gitee 企业版与私有部署本轮不做。
//!
//! 依据（2026-11-18 取官方 Swagger `https://gitee.com/api/v5/doc_json`，并对公开仓库 openharmony/docs 不带令牌只读实测）：
//! - 认证：个人访问令牌。官方文档写的是 `access_token` 查询参数；实测 `Authorization: token <令牌>` 也被识别（不带头回
//!   `登录失效…40001`，带假令牌回 `401 Unauthorized: Access token does not exist`）。用请求头，令牌不进地址（地址会出现在
//!   传输层错误里）。生成页 https://gitee.com/profile/personal_access_tokens，权限 user_info、projects、pull_requests、notes。
//! - 按分支找：`GET /repos/{o}/{r}/pulls?state=open&head=<分支>`（head 只认分支名；`state=all` 或 `merged` 配 head 实测回 500），
//!   开着的找不到再列最近的 50 条（不带 head）按 head.ref 与 head 仓库筛。
//! - PR：`state` open / closed / merged，`draft`，`mergeable`（布尔，原因不给，false 按「还不知道」），`head{ref, sha, repo}`。
//! - 检查：`GET /repos/{o}/{r}/commits/{sha}/check-runs`（status queued / in_progress / completed，conclusion 同 GitHub 的
//!   CheckRun）。仓库没接检查时是空列表，就不显示检查，不编造。
//! - 合并：`PUT /pulls/{n}/merge`，`merge_method` merge / squash / rebase。**没有头提交保护参数**：合并前再读一次 PR 核对 head.sha。
//!   仓库级「允许的合并方式」接口没有，三种都列，服务器拒绝时按原话归类。
//! - 建 PR：`POST /pulls`（head 同仓库写分支名、fork 写 `path_with_namespace:分支`，`draft`）；已有 PR 的回包官方没写，
//!   建之前先查同分支开着的 PR。
//! - 评论：`GET /pulls/{n}/comments`（comment_type pr_comment / diff_comment，diff_comment 带 path 与 new_line）；
//!   `POST /pulls/{n}/comments`（行内评论带 commit_id、path、position = 在这个文件 diff 里的行序号）。
//! - 改动文件：`GET /pulls/{n}/files`（additions / deletions 是字符串，patch 是 `{diff: …}`）。

use super::api::{merge_fields, summarize, ForgeApi, NewPr, Target};
use super::http::{enc, http_error, json, message_of, ok, Client};
use super::platform::Platform;
use super::GhState;
use meowo_protocol::ipc::{PrCheckDto, PrCommentDto, PrFileDto, PrReviewDto, PullRequestDto};
use serde_json::{json, Value};

pub(crate) const HOST: &str = "gitee.com";

pub(crate) struct Gitee<'a> {
    pub(crate) client: Client<'a>,
    pub(crate) state: GhState,
    pub(crate) login: Option<String>,
}

fn s<'v>(v: &'v Value, pointer: &str) -> &'v str {
    v.pointer(pointer).and_then(|x| x.as_str()).unwrap_or_default()
}

fn opt(v: &Value, pointer: &str) -> Option<String> {
    v.pointer(pointer).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(str::to_string)
}

/// 数字字段（Gitee 的 additions / deletions 是字符串）。
fn num(v: &Value, key: &str) -> u32 {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0) as u32,
        Some(Value::String(t)) => t.parse().unwrap_or(0),
        _ => 0,
    }
}

/// check-run → pass / fail / pending / skipped（与 GitHub 的 CheckRun 同一套 status / conclusion）。
pub(crate) fn check_state(run: &Value) -> &'static str {
    if s(run, "/status") != "completed" {
        return "pending";
    }
    match s(run, "/conclusion") {
        "success" | "neutral" => "pass",
        "skipped" => "skipped",
        _ => "fail",
    }
}

pub(crate) fn map_checks(runs: &Value) -> Vec<PrCheckDto> {
    runs.get("check_runs")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|run| PrCheckDto {
            name: s(run, "/name").into(),
            workflow: None,
            state: check_state(run).into(),
            url: opt(run, "/details_url").or_else(|| opt(run, "/html_url")),
        })
        .collect()
}

pub(crate) fn map_pull(pr: &Value, checks: Vec<PrCheckDto>) -> Option<PullRequestDto> {
    let number = u32::try_from(pr.get("number")?.as_u64()?).ok()?;
    let draft = pr.get("draft").and_then(|d| d.as_bool()).unwrap_or(false);
    let merged = pr.get("merged_at").is_some_and(|m| !m.is_null());
    let state = match (s(pr, "/state"), merged) {
        ("merged", _) | (_, true) => "merged",
        ("open", _) if draft => "draft",
        ("open", _) => "open",
        ("closed", _) => "closed",
        _ => return None,
    };
    // mergeable 只是布尔，不给原因（冲突、待审查、待测试都是 false）：false 不当冲突，按「还不知道」。
    let unknown = state == "open" && pr.get("mergeable").and_then(|m| m.as_bool()) != Some(true);
    let (mergeable, merge_state) = merge_fields(false, unknown, draft, false);
    Some(PullRequestDto {
        number,
        state: state.into(),
        url: s(pr, "/html_url").into(),
        title: s(pr, "/title").into(),
        base_ref: opt(pr, "/base/ref"),
        head_ref: opt(pr, "/head/ref"),
        head_oid: opt(pr, "/head/sha"),
        mergeable: Some(mergeable),
        merge_state: Some(merge_state),
        checks_summary: Some(summarize(&checks)),
        checks,
        queued: false,
        forge: Some("gitee".into()),
    })
}

/// 列表里挑 head 分支与 head 仓库对得上的，开着的优先。
pub(crate) fn pick_pull<'v>(list: &'v [Value], branch: &str, head_repo: &str) -> Option<&'v Value> {
    let fit = |p: &&Value| s(p, "/head/ref") == branch && s(p, "/head/repo/full_name").eq_ignore_ascii_case(head_repo);
    list.iter()
        .filter(fit)
        .find(|p| s(p, "/state") == "open")
        .or_else(|| list.iter().find(fit))
}

pub(crate) fn map_comments(items: &[Value]) -> Vec<PrCommentDto> {
    items
        .iter()
        .filter_map(|c| {
            let id = c.get("id")?.as_u64()?;
            let path = opt(c, "/path").filter(|_| s(c, "/comment_type") == "diff_comment");
            let line = c.get("new_line").and_then(|l| l.as_i64());
            Some(PrCommentDto {
                id: format!("comment:{id}"),
                author: opt(c, "/user/login").unwrap_or_else(|| "unknown".into()),
                body: s(c, "/body").into(),
                created_at: s(c, "/created_at").into(),
                url: opt(c, "/html_url"),
                state: None,
                line: path.as_ref().and(line),
                side: path.as_ref().map(|_| "RIGHT".into()),
                path,
            })
        })
        .collect()
}

/// 审查人点了「通过」的算批准（assignees 是审查人，accept = 已通过）。
pub(crate) fn map_reviews(pr: &Value) -> Vec<PrCommentDto> {
    pr.get("assignees")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|a| a.get("accept").and_then(|x| x.as_bool()).unwrap_or(false))
        .filter_map(|a| {
            let who = opt(a, "/login")?;
            Some(PrCommentDto {
                id: format!("review:approval:{who}"),
                author: who,
                body: String::new(),
                created_at: String::new(),
                url: None,
                state: Some("APPROVED".into()),
                path: None,
                line: None,
                side: None,
            })
        })
        .collect()
}

pub(crate) fn map_files(items: &[Value]) -> Vec<PrFileDto> {
    items
        .iter()
        .filter_map(|f| {
            Some(PrFileDto {
                path: opt(f, "/filename")?,
                status: match s(f, "/status") {
                    "added" => "added",
                    "removed" | "deleted" => "removed",
                    "renamed" => "renamed",
                    _ => "modified",
                }
                .into(),
                additions: num(f, "additions"),
                deletions: num(f, "deletions"),
                patch: opt(f, "/patch/diff").or_else(|| opt(f, "/patch")),
            })
        })
        .collect()
}

/// 行内评论的 position：这一行在这个文件 diff 里的序号（第一个 `@@` 之后从 1 数起，后面的 `@@` 也算一行，同 GitHub
/// 旧版 position 的数法）。side：NEW / OLD。找不到（行不在 diff 里）回 None。
pub(crate) fn diff_line_position(patch: &str, side: &str, line: i64) -> Option<i64> {
    let old_side = side == "OLD";
    let (mut old_no, mut new_no, mut position) = (0i64, 0i64, 0i64);
    let mut started = false;
    for text in patch.lines() {
        if let Some(header) = text.strip_prefix("@@ ") {
            if started {
                position += 1;
            }
            started = true;
            let mut parts = header.split_whitespace();
            let start = |part: Option<&str>, sign: char| -> Option<i64> { part?.strip_prefix(sign)?.split(',').next()?.parse().ok() };
            old_no = start(parts.next(), '-')?;
            new_no = start(parts.next(), '+')?;
            continue;
        }
        if !started {
            continue;
        }
        position += 1;
        match text.chars().next() {
            Some('+') => {
                if !old_side && new_no == line {
                    return Some(position);
                }
                new_no += 1;
            }
            Some('-') => {
                if old_side && old_no == line {
                    return Some(position);
                }
                old_no += 1;
            }
            Some('\\') => {}
            _ => {
                if (old_side && old_no == line) || (!old_side && new_no == line) {
                    return Some(position);
                }
                old_no += 1;
                new_no += 1;
            }
        }
    }
    None
}

/// 合并被拒的原话归类（官方没列原文，按关键词）。
pub(crate) fn classify_refusal(message: &str, url: &str) -> String {
    let low = message.to_lowercase();
    if low.contains("conflict") || message.contains("冲突") {
        format!("gh/merge_conflict: {url}")
    } else if message.contains("审查") || message.contains("评审") || low.contains("review") {
        "gh/review_required".into()
    } else if message.contains("合并方式") || message.contains("合并方法") || low.contains("merge_method") {
        "gh/method_not_allowed".into()
    } else {
        format!("gh/merge_blocked: {message}")
    }
}

/// 列表地址是不是 Gitee 实测会回 500 的组合：带 head 且 state 不是 open（2026-11-18 openharmony/docs 实测
/// `state=all&head=…`、`state=merged&head=…` 都回 500）。
pub(crate) fn tolerates_500(url: &str) -> bool {
    let query = url.split_once('?').map(|(_, q)| q).unwrap_or_default();
    let mut head = false;
    let mut open = false;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("head", v)) if !v.is_empty() => head = true,
            Some(("state", "open")) => open = true,
            _ => {}
        }
    }
    head && !open
}

impl Gitee<'_> {
    fn repo(&self, slug: &str) -> String {
        let (owner, name) = slug.split_once('/').unwrap_or((slug, ""));
        format!("https://{HOST}/api/v5/repos/{}/{}", enc(owner), enc(name))
    }

    fn checks(&self, slug: &str, sha: &str) -> Vec<PrCheckDto> {
        if sha.is_empty() {
            return Vec::new();
        }
        self.client
            .get(&format!("{}/commits/{}/check-runs?per_page=100", self.repo(slug), enc(sha)))
            .map(|v| map_checks(&v))
            .unwrap_or_default()
    }

    fn with_checks(&self, slug: &str, pr: &Value) -> Result<PullRequestDto, String> {
        let checks = if s(pr, "/state") == "open" {
            self.checks(slug, s(pr, "/head/sha"))
        } else {
            Vec::new()
        };
        map_pull(pr, checks).ok_or_else(|| "gh/failed: bad pull request json".into())
    }

    fn list(&self, url: &str) -> Result<Vec<Value>, String> {
        let resp = self.client.call("GET", url, None)?;
        // 只有实测会 500 的组合（head 配 state=all / merged）当没找到；别的 5xx 照常报错（复核指出：一律当空会让 PR 区块
        // 消失、建 PR 查重被跳过、行内评论报 invalid_line）。
        if resp.status >= 500 && tolerates_500(url) {
            return Ok(Vec::new());
        }
        if !ok(&resp) {
            return Err(http_error(&resp));
        }
        Ok(json(&resp.body).as_array().cloned().unwrap_or_default())
    }

    fn find_raw(&self, slug: &str, target: &Target) -> Result<Option<Value>, String> {
        match target {
            Target::Number(n) => self.client.get_opt(&format!("{}/pulls/{n}", self.repo(slug))),
            Target::Branch { branch, head_slug, .. } => {
                let head_repo = head_slug.unwrap_or(slug);
                let open = self.list(&format!("{}/pulls?state=open&head={}&per_page=30", self.repo(slug), enc(branch)))?;
                if let Some(found) = pick_pull(&open, branch, head_repo) {
                    return Ok(Some(found.clone()));
                }
                let recent = self.list(&format!("{}/pulls?state=all&sort=updated&direction=desc&per_page=50", self.repo(slug)))?;
                Ok(pick_pull(&recent, branch, head_repo).cloned())
            }
        }
    }
}

impl ForgeApi for Gitee<'_> {
    fn platform(&self) -> Platform {
        Platform::Gitee
    }

    fn state(&self, _refresh: bool) -> GhState {
        self.state
    }

    fn merge_methods(&self, _dir: &str, _slug: &str) -> Result<Vec<String>, String> {
        Ok(vec!["merge".into(), "squash".into(), "rebase".into()])
    }

    fn find(&self, _dir: &str, slug: Option<&str>, target: &Target) -> Result<Option<PullRequestDto>, String> {
        let slug = slug.ok_or("gh/ambiguous_repo")?;
        match self.find_raw(slug, target)? {
            Some(pr) => self.with_checks(slug, &pr).map(Some),
            None => Ok(None),
        }
    }

    fn find_url(&self, _dir: &str, url: &str) -> Result<Option<PullRequestDto>, String> {
        let (_, slug, number, platform) = super::platform::parse_pr_url(url).ok_or("gh/failed: bad url")?;
        if platform != Platform::Gitee {
            return Ok(None);
        }
        let Some(pr) = self.client.get_opt(&format!("{}/pulls/{number}", self.repo(&slug)))? else {
            return Ok(None);
        };
        Ok(map_pull(&pr, Vec::new()).map(|pr| PullRequestDto {
            number: pr.number,
            state: pr.state,
            url: pr.url,
            title: pr.title,
            forge: pr.forge,
            ..Default::default()
        }))
    }

    fn create(&self, _dir: &str, slug: Option<&str>, pr: &NewPr) -> Result<String, String> {
        let slug = slug.ok_or("gh/ambiguous_repo")?;
        let head_repo = pr.head_slug.unwrap_or(slug);
        let open = self.list(&format!("{}/pulls?state=open&head={}&per_page=30", self.repo(slug), enc(pr.branch)))?;
        if let Some(existing) = pick_pull(&open, pr.branch, head_repo).filter(|p| s(p, "/state") == "open") {
            return Err(format!("gh/pr_exists: {}", s(existing, "/html_url")));
        }
        let head = match pr.head_slug {
            Some(fork) => format!("{fork}:{}", pr.branch),
            None => pr.branch.to_string(),
        };
        let body = json!({ "title": pr.title, "head": head, "base": pr.base, "body": pr.body, "draft": pr.draft });
        let resp = self.client.call("POST", &format!("{}/pulls", self.repo(slug)), Some(body))?;
        if ok(&resp) {
            return Ok(s(&json(&resp.body), "/html_url").to_string());
        }
        Err(http_error(&resp))
    }

    fn merge(&self, _dir: &str, slug: &str, pr: &PullRequestDto, method: &str, head: &str) -> Result<bool, String> {
        // 没有头提交参数：合并前再读一次，头提交变了就不合。
        let fresh = self
            .client
            .get_opt(&format!("{}/pulls/{}", self.repo(slug), pr.number))?
            .ok_or("gh/head_changed")?;
        if s(&fresh, "/head/sha") != head {
            return Err("gh/head_changed".into());
        }
        let resp = self.client.call(
            "PUT",
            &format!("{}/pulls/{}/merge", self.repo(slug), pr.number),
            Some(json!({ "merge_method": method })),
        )?;
        if ok(&resp) {
            return Ok(false);
        }
        let message = message_of(&resp);
        Err(match resp.status {
            400 | 405 | 409 | 422 => classify_refusal(&message, &pr.url),
            _ => http_error(&resp),
        })
    }

    fn review(&self, _dir: &str, slug: &str, branch: &str) -> Result<Option<PrReviewDto>, String> {
        let target = Target::Branch {
            qualified: branch,
            branch,
            head_slug: None,
        };
        let Some(listed) = self.find_raw(slug, &target)? else { return Ok(None) };
        let number = listed.get("number").and_then(|n| n.as_u64()).unwrap_or_default();
        let full = self.client.get(&format!("{}/pulls/{number}", self.repo(slug)))?;
        let pr = self.with_checks(slug, &full)?;
        let base = format!("{}/pulls/{number}", self.repo(slug));
        let mut comments = Vec::new();
        for page in 1..=10 {
            let list = self.list(&format!("{base}/comments?per_page=100&page={page}"))?;
            let done = list.len() < 100;
            comments.extend(map_comments(&list));
            if done {
                break;
            }
        }
        let files = map_files(&self.list(&format!("{base}/files"))?);
        Ok(Some(PrReviewDto {
            reviews: map_reviews(&full),
            pr,
            comments,
            files,
            monitor: None,
        }))
    }

    fn comment(&self, _dir: &str, slug: &str, number: u32, body: &str, _review: bool) -> Result<(), String> {
        let resp = self.client.call(
            "POST",
            &format!("{}/pulls/{number}/comments", self.repo(slug)),
            Some(json!({ "body": body })),
        )?;
        if ok(&resp) {
            Ok(())
        } else {
            Err(http_error(&resp))
        }
    }

    fn line_comment(&self, _dir: &str, slug: &str, number: u32, path: &str, line: i64, side: &str, body: &str) -> Result<(), String> {
        let base = format!("{}/pulls/{number}", self.repo(slug));
        let pr = self.client.get_opt(&base)?.ok_or("pr/not_found")?;
        let files = self.list(&format!("{base}/files"))?;
        let patch = files
            .iter()
            .find(|f| s(f, "/filename") == path)
            .and_then(|f| opt(f, "/patch/diff").or_else(|| opt(f, "/patch")))
            .unwrap_or_default();
        let position = diff_line_position(&patch, side, line).ok_or("pr/invalid_line")?;
        let comment = json!({ "body": body, "commit_id": s(&pr, "/head/sha"), "path": path, "position": position });
        let resp = self.client.call("POST", &format!("{base}/comments"), Some(comment))?;
        if ok(&resp) {
            Ok(())
        } else {
            Err(http_error(&resp))
        }
    }

    fn viewer(&self, _dir: &str) -> Option<String> {
        self.login.clone()
    }
}

/// 令牌好不好用、是谁：`GET /api/v5/user`（要 user_info 权限）。
pub(crate) fn current_user(client: &Client) -> Result<(String, Option<String>), String> {
    let v = client.get(&format!("https://{HOST}/api/v5/user"))?;
    let login = opt(&v, "/login").ok_or("gh/failed: no login")?;
    Ok((login, opt(&v, "/name")))
}
