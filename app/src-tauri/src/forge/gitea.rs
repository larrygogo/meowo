//! Gitea / Forgejo（含 codeberg.org，REST `/api/v1`）的 PR。
//!
//! 依据（2026-11 查证，Gitea 与 Forgejo 的 Swagger、源码 routers/api/v1/repo/pull.go，Codeberg 实测）：
//! - 认证头 `Authorization: token <令牌>`（`bearer` 也认）。
//! - 按分支找 PR：Forgejo 的 `GET /pulls` 认 `head=<分支>`，Gitea 不认（忽略这个参数）。两边都按回包的
//!   `head.ref` 再筛一遍，先看打开的，再看最近关闭的。
//! - PR：`state` open/closed + `merged`；`draft` 由标题前缀（默认 `WIP:` / `[WIP]`）推出；`mergeable`。
//! - 检查：`GET /commits/{sha}/status` 的 `statuses[]`（pending/success/error/failure/warning/skipped），
//!   `target_url` 可能是相对路径，要拼上主机。
//! - 合并：`POST /pulls/{n}/merge`，`Do` = merge/rebase/squash，`head_commit_id` 钉住头提交（不符回 409
//!   `head out of date`）；405 = 方式不允许 / 草稿 / 正在检查 / 没权限；409 = 冲突或头提交变了。
//!   仓库允许的方式：`GET /repos/{o}/{r}` 的 allow_merge_commits / allow_squash_merge / allow_rebase。
//! - 建 PR：`POST /pulls`（head 可写 `owner:分支`）；已存在回 409，原话里 `issue_id: N` 就是已有 PR 的号。
//! - 评论：普通评论走 issue（`/issues/{n}/comments`，令牌要 issue 写权限）；评审 `/pulls/{n}/reviews`，评审里的
//!   行内评论 `/pulls/{n}/reviews/{id}/comments`；发行内评论 = 建一个 COMMENT 评审带 comments。

use super::api::{merge_fields, summarize, ForgeApi, NewPr, Target};
use super::http::{enc, get_all, http_error, json, message_of, ok, Client};
use super::platform::Platform;
use super::GhState;
use meowo_protocol::ipc::{PrCheckDto, PrCommentDto, PrFileDto, PrReviewDto, PullRequestDto};
use serde_json::{json, Value};

pub(crate) struct Gitea<'a> {
    pub(crate) client: Client<'a>,
    /// 主机（可带端口），API 在 `https://<主机>/api/v1`。
    pub(crate) host: String,
    pub(crate) state: GhState,
    pub(crate) login: Option<String>,
}

fn s<'v>(v: &'v Value, pointer: &str) -> &'v str {
    v.pointer(pointer).and_then(|x| x.as_str()).unwrap_or_default()
}

fn opt(v: &Value, pointer: &str) -> Option<String> {
    v.pointer(pointer).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(str::to_string)
}

/// 提交状态 → pass / fail / pending / skipped（warning 是「通过但有提醒」，不挡合并）。
pub(crate) fn status_state(status: &str) -> &'static str {
    match status {
        "success" | "warning" => "pass",
        "failure" | "error" => "fail",
        "skipped" => "skipped",
        _ => "pending",
    }
}

/// 合并状态：combined status 的 statuses[] → 检查列表（相对链接拼上主机）。
pub(crate) fn map_statuses(host: &str, combined: &Value) -> Vec<PrCheckDto> {
    combined
        .get("statuses")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|item| PrCheckDto {
            name: s(item, "/context").into(),
            workflow: None,
            state: status_state(s(item, "/status")).into(),
            url: opt(item, "/target_url").map(|u| if u.starts_with('/') { format!("https://{host}{u}") } else { u }),
        })
        .collect()
}

/// PR → 统一的 PR（检查另给）。
pub(crate) fn map_pull(pr: &Value, checks: Vec<PrCheckDto>) -> Option<PullRequestDto> {
    let number = u32::try_from(pr.get("number")?.as_u64()?).ok()?;
    let draft = pr.get("draft").and_then(|d| d.as_bool()).unwrap_or(false);
    let merged = pr.get("merged").and_then(|m| m.as_bool()).unwrap_or(false);
    let state = match (s(pr, "/state"), merged) {
        (_, true) => "merged",
        ("open", _) if draft => "draft",
        ("open", _) => "open",
        ("closed", _) => "closed",
        _ => return None,
    };
    // Gitea 只给 mergeable 一个布尔，不给状态：models/issues/pull.go 的 Mergeable() 在「还在检查冲突」「有冲突」「检查出错」
    // 「草稿（WIP）」时都回 false，区分不了。所以 false 不当冲突，按「还不知道」（合并按钮说稍后再试）；真有冲突时
    // 合并接口回 409，再归为 gh/merge_conflict。草稿另有 DRAFT。
    let mergeable = pr.get("mergeable").and_then(|m| m.as_bool());
    let unknown = mergeable != Some(true) && !draft && state == "open";
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
        forge: Some("gitea".into()),
    })
}

/// 列表里挑 head 分支对得上的（fork 还要 head 仓库对得上）。列表本身是「新的在前」。
pub(crate) fn pick_pull<'v>(list: &'v [Value], branch: &str, head_repo: &str) -> Option<&'v Value> {
    list.iter()
        .find(|p| s(p, "/head/ref") == branch && s(p, "/head/repo/full_name").eq_ignore_ascii_case(head_repo))
}

fn comment_of(item: &Value, review: bool) -> Option<PrCommentDto> {
    let id = item.get("id")?.as_u64()?;
    let position = item.get("position").and_then(|p| p.as_i64()).filter(|p| *p > 0);
    let original = item.get("original_position").and_then(|p| p.as_i64()).filter(|p| *p > 0);
    let path = opt(item, "/path");
    Some(PrCommentDto {
        id: if review { format!("review:{id}") } else { format!("comment:{id}") },
        author: opt(item, "/user/login").unwrap_or_else(|| "unknown".into()),
        body: s(item, "/body").into(),
        created_at: opt(item, "/created_at").or_else(|| opt(item, "/submitted_at")).unwrap_or_default(),
        url: opt(item, "/html_url"),
        state: if review { opt(item, "/state") } else { None },
        line: path.as_ref().and(position.or(original)),
        side: path.as_ref().map(|_| if position.is_some() { "RIGHT".into() } else { "LEFT".into() }),
        path,
    })
}

pub(crate) fn map_comments(items: &[Value]) -> Vec<PrCommentDto> {
    items.iter().filter_map(|item| comment_of(item, false)).collect()
}

/// 评审结论（「请求评审」不是结论，不算；待提交的草稿评审不算）。
pub(crate) fn map_reviews(items: &[Value]) -> Vec<PrCommentDto> {
    items
        .iter()
        .filter(|r| !matches!(s(r, "/state"), "REQUEST_REVIEW" | "PENDING"))
        .filter_map(|r| comment_of(r, true))
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
                    "deleted" | "removed" => "removed",
                    "renamed" => "renamed",
                    _ => "modified",
                }
                .into(),
                additions: f.get("additions").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
                deletions: f.get("deletions").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
                patch: None,
            })
        })
        .collect()
}

/// 409 原话里的 `issue_id: N`（已有 PR 的号）。
pub(crate) fn existing_index(message: &str) -> Option<u32> {
    let (_, rest) = message.split_once("issue_id: ")?;
    rest.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()
}

impl Gitea<'_> {
    fn repo(&self, slug: &str) -> String {
        let (owner, name) = slug.split_once('/').unwrap_or((slug, ""));
        format!("https://{}/api/v1/repos/{}/{}", self.host, enc(owner), enc(name))
    }

    fn checks(&self, slug: &str, sha: &str) -> Vec<PrCheckDto> {
        if sha.is_empty() {
            return Vec::new();
        }
        self.client
            .get(&format!("{}/commits/{}/status?limit=100", self.repo(slug), enc(sha)))
            .map(|v| map_statuses(&self.host, &v))
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

    fn find_raw(&self, slug: &str, target: &Target) -> Result<Option<Value>, String> {
        match target {
            Target::Number(n) => self.client.get_opt(&format!("{}/pulls/{n}", self.repo(slug))),
            Target::Branch { branch, head_slug, .. } => {
                let head_repo = head_slug.unwrap_or(slug);
                for state in ["open", "closed"] {
                    let url = format!("{}/pulls?state={state}&head={}&sort=recentupdate&limit=50", self.repo(slug), enc(branch));
                    let pages = if state == "open" { 3 } else { 1 };
                    let list = get_all(&self.client, &self.host, &url, pages)?;
                    if let Some(found) = pick_pull(&list, branch, head_repo) {
                        return Ok(Some(found.clone()));
                    }
                }
                Ok(None)
            }
        }
    }
}

impl ForgeApi for Gitea<'_> {
    fn platform(&self) -> Platform {
        Platform::Gitea
    }

    fn state(&self, _refresh: bool) -> GhState {
        self.state
    }

    fn merge_methods(&self, _dir: &str, slug: &str) -> Result<Vec<String>, String> {
        let repo = self.client.get(&self.repo(slug))?;
        let allowed = |key: &str| repo.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
        Ok([
            ("allow_merge_commits", "merge"),
            ("allow_squash_merge", "squash"),
            ("allow_rebase", "rebase"),
        ]
        .into_iter()
        .filter(|(key, _)| allowed(key))
        .map(|(_, name)| name.to_string())
        .collect())
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
        if platform != Platform::Gitea {
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
        let title = if pr.draft { format!("WIP: {}", pr.title) } else { pr.title.to_string() };
        let body = json!({ "head": pr.head, "base": pr.base, "title": title, "body": pr.body });
        let resp = self.client.call("POST", &format!("{}/pulls", self.repo(slug)), Some(body))?;
        if ok(&resp) {
            return Ok(s(&json(&resp.body), "/html_url").to_string());
        }
        let message = message_of(&resp);
        if resp.status == 409 && message.contains("already exists") {
            let url = existing_index(&message)
                .map(|n| format!("https://{}/{slug}/pulls/{n}", self.host))
                .unwrap_or_default();
            return Err(format!("gh/pr_exists: {url}"));
        }
        Err(http_error(&resp))
    }

    fn merge(&self, _dir: &str, slug: &str, pr: &PullRequestDto, method: &str, head: &str) -> Result<bool, String> {
        let body = json!({ "Do": method, "head_commit_id": head, "delete_branch_after_merge": false });
        let resp = self
            .client
            .call("POST", &format!("{}/pulls/{}/merge", self.repo(slug), pr.number), Some(body))?;
        if ok(&resp) {
            return Ok(false);
        }
        let message = message_of(&resp);
        let low = message.to_lowercase();
        Err(match resp.status {
            405 if low.contains("already merged") => return Ok(false),
            405 if low.contains("merge style") => "gh/method_not_allowed".into(),
            405 if low.contains("work in progress") => "gh/merge_blocked: draft".into(),
            405 if low.contains("try again later") => "gh/checks_pending".into(),
            405 if low.contains("not allowed to merge") => format!("forge/forbidden: {message}"),
            405 => format!("gh/merge_blocked: {message}"),
            409 if low.contains("out of date") && !low.contains("push") => "gh/head_changed".into(),
            409 if low.contains("conflict") || message.is_empty() || message.starts_with('{') => format!("gh/merge_conflict: {}", pr.url),
            409 => format!("gh/merge_blocked: {message}"),
            423 => format!("gh/merge_blocked: {message}"),
            _ => http_error(&resp),
        })
    }

    fn review(&self, _dir: &str, slug: &str, branch: &str) -> Result<Option<PrReviewDto>, String> {
        let target = Target::Branch {
            qualified: branch,
            branch,
            head_slug: None,
        };
        let Some(raw) = self.find_raw(slug, &target)? else { return Ok(None) };
        let pr = self.with_checks(slug, &raw)?;
        let base = self.repo(slug);
        let mut comments = map_comments(&get_all(
            &self.client,
            &self.host,
            &format!("{base}/issues/{}/comments?limit=50", pr.number),
            10,
        )?);
        let reviews = get_all(&self.client, &self.host, &format!("{base}/pulls/{}/reviews?limit=50", pr.number), 5)?;
        // 评审里的行内评论（每个带评论的评审一次请求）。
        for review in reviews
            .iter()
            .filter(|r| r.get("comments_count").and_then(|c| c.as_u64()).unwrap_or(0) > 0)
            .take(30)
        {
            let Some(id) = review.get("id").and_then(|i| i.as_u64()) else { continue };
            if let Ok(list) = self.client.get(&format!("{base}/pulls/{}/reviews/{id}/comments", pr.number)) {
                comments.extend(map_comments(list.as_array().map(Vec::as_slice).unwrap_or_default()));
            }
        }
        let files = map_files(&get_all(
            &self.client,
            &self.host,
            &format!("{base}/pulls/{}/files?limit=50", pr.number),
            10,
        )?);
        Ok(Some(PrReviewDto {
            pr,
            comments,
            reviews: map_reviews(&reviews),
            files,
            monitor: None,
        }))
    }

    fn comment(&self, _dir: &str, slug: &str, number: u32, body: &str, review: bool) -> Result<(), String> {
        let resp = if review {
            self.client.call(
                "POST",
                &format!("{}/pulls/{number}/reviews", self.repo(slug)),
                Some(json!({ "event": "COMMENT", "body": body })),
            )?
        } else {
            self.client.call(
                "POST",
                &format!("{}/issues/{number}/comments", self.repo(slug)),
                Some(json!({ "body": body })),
            )?
        };
        if ok(&resp) {
            Ok(())
        } else {
            Err(http_error(&resp))
        }
    }

    fn line_comment(&self, _dir: &str, slug: &str, number: u32, path: &str, line: i64, side: &str, body: &str) -> Result<(), String> {
        let pr = self
            .client
            .get_opt(&format!("{}/pulls/{number}", self.repo(slug)))?
            .ok_or("pr/not_found")?;
        let head = opt(&pr, "/head/sha").ok_or("gh/head_changed")?;
        let (new_position, old_position) = if side == "OLD" { (0, line) } else { (line, 0) };
        let review = json!({
            "event": "COMMENT",
            "body": "",
            "commit_id": head,
            "comments": [{ "path": path, "body": body, "new_position": new_position, "old_position": old_position }],
        });
        let resp = self
            .client
            .call("POST", &format!("{}/pulls/{number}/reviews", self.repo(slug)), Some(review))?;
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

/// 令牌好不好用、是谁：`GET /api/v1/user`（要 user 读权限）。
pub(crate) fn current_user(client: &Client, host: &str) -> Result<(String, Option<String>), String> {
    let v = client.get(&format!("https://{host}/api/v1/user"))?;
    let login = opt(&v, "/login").ok_or("gh/failed: no login")?;
    Ok((login, opt(&v, "/full_name")))
}
