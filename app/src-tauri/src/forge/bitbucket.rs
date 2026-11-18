//! Bitbucket Cloud（REST 2.0，`https://api.bitbucket.org/2.0`）的 PR。
//!
//! 依据（2026-11 查证，官方 OpenAPI swagger.v3.json 与 developer.atlassian.com/cloud/bitbucket/rest，公开仓库实测）：
//! - 认证：App password 2025-09-09 起不能新建，2026-07-28 彻底失效；现在用 Atlassian 账号的 API token
//!   （Basic：Atlassian 邮箱 + 令牌），权限 read:user / read:repository / read:pullrequest / write:pullrequest
//!   （:bitbucket 后缀）。
//! - 按分支找：`GET …/pullrequests?q=source.branch.name="<分支>"`，state 可重复传；`fields=` 只取要的字段
//!   （PR 的 summary / description 动辄上百 KB）。`source.commit.hash` 是 12 位短 hash。
//! - 检查：`GET …/pullrequests/{id}/statuses`（SUCCESSFUL / FAILED / INPROGRESS / STOPPED）。
//! - 合并：`POST …/pullrequests/{id}/merge`，`merge_strategy` = merge_commit / squash / rebase_fast_forward …；
//!   **不支持钉住头提交**，合并前再读一次 PR 核对 `source.commit.hash`（中间仍有一小段竞态窗口）。
//!   202 = 转成异步或进了合并队列；400 = 不能合并；409 = 合并途中 ref 变了；555 = 超时。
//!   目标分支允许的方式在 PR 的 `destination.branch.merge_strategies`（登录后才有）。
//! - 建 PR：`POST …/pullrequests`；同分支已有 PR 时的回包官方没写，建之前先查一次打开的 PR。
//! - 评论：`…/comments`（`inline{path, from, to}`，`deleted`），分页看回包里的 `next`。
//! - 改动文件：`…/diffstat`（会 302 到仓库级的 diffstat，同一台主机，照跟）。

use super::api::{merge_fields, summarize, ForgeApi, NewPr, Target};
use super::http::{enc, http_error, json, message_of, ok, Client};
use super::platform::Platform;
use super::GhState;
use meowo_protocol::ipc::{PrCheckDto, PrCommentDto, PrFileDto, PrReviewDto, PullRequestDto};
use serde_json::{json, Value};

pub(crate) const API_HOST: &str = "api.bitbucket.org";

/// 列表只取这些字段。
const LIST_FIELDS: &str = "next,values.id,values.title,values.state,values.draft,values.links.html.href,values.source.branch.name,values.source.commit.hash,values.source.repository.full_name,values.destination.branch.name,values.created_on";

pub(crate) struct Bitbucket<'a> {
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

/// 统一的合并方式 → Bitbucket 的 merge_strategy（rebase 对 GitHub 的「变基后快进」）。
pub(crate) fn strategy(method: &str) -> Option<&'static str> {
    match method {
        "merge" => Some("merge_commit"),
        "squash" => Some("squash"),
        "rebase" => Some("rebase_fast_forward"),
        _ => None,
    }
}

pub(crate) fn status_state(state: &str) -> &'static str {
    match state {
        "SUCCESSFUL" => "pass",
        // STOPPED：构建被人停掉，没有通过，按失败挡合并。
        "FAILED" | "STOPPED" => "fail",
        _ => "pending",
    }
}

pub(crate) fn map_statuses(statuses: &[Value]) -> Vec<PrCheckDto> {
    statuses
        .iter()
        .map(|st| PrCheckDto {
            name: opt(st, "/name").or_else(|| opt(st, "/key")).unwrap_or_default(),
            workflow: None,
            state: status_state(s(st, "/state")).into(),
            url: opt(st, "/url"),
        })
        .collect()
}

pub(crate) fn map_pr(pr: &Value, checks: Vec<PrCheckDto>) -> Option<PullRequestDto> {
    let number = u32::try_from(pr.get("id")?.as_u64()?).ok()?;
    let draft = pr.get("draft").and_then(|d| d.as_bool()).unwrap_or(false);
    let state = match s(pr, "/state") {
        "OPEN" if draft => "draft",
        "OPEN" => "open",
        "MERGED" => "merged",
        "DECLINED" | "SUPERSEDED" => "closed",
        _ => return None,
    };
    // 列表与单个 PR 都不给冲突信息（合并前检查在合并时由服务器判，400 时归类）。
    let (mergeable, merge_state) = merge_fields(false, false, draft, false);
    Some(PullRequestDto {
        number,
        state: state.into(),
        url: s(pr, "/links/html/href").into(),
        title: s(pr, "/title").into(),
        base_ref: opt(pr, "/destination/branch/name"),
        head_ref: opt(pr, "/source/branch/name"),
        head_oid: opt(pr, "/source/commit/hash"),
        mergeable: Some(mergeable),
        merge_state: Some(merge_state),
        checks_summary: Some(summarize(&checks)),
        checks,
        queued: false,
        forge: Some("bitbucket".into()),
    })
}

/// 列表里挑：源仓库对得上（fork 时是 fork 的仓库）；打开的排前面；其余按创建时间新的在前。
pub(crate) fn pick_pr<'v>(list: &'v [Value], head_repo: &str) -> Option<&'v Value> {
    let mut sorted: Vec<&Value> = list
        .iter()
        .filter(|p| s(p, "/source/repository/full_name").eq_ignore_ascii_case(head_repo))
        .collect();
    sorted.sort_by(|a, b| {
        (s(a, "/state") != "OPEN")
            .cmp(&(s(b, "/state") != "OPEN"))
            .then_with(|| s(b, "/created_on").cmp(s(a, "/created_on")))
    });
    sorted.into_iter().next()
}

pub(crate) fn map_comments(items: &[Value]) -> Vec<PrCommentDto> {
    items
        .iter()
        .filter(|c| !c.get("deleted").and_then(|d| d.as_bool()).unwrap_or(false))
        .filter_map(|c| {
            let id = c.get("id")?.as_u64()?;
            let inline = c.get("inline").filter(|i| !i.is_null());
            let to = inline.and_then(|i| i.get("to")).and_then(|v| v.as_i64());
            let from = inline.and_then(|i| i.get("from")).and_then(|v| v.as_i64());
            Some(PrCommentDto {
                id: format!("comment:{id}"),
                author: opt(c, "/user/nickname")
                    .or_else(|| opt(c, "/user/display_name"))
                    .unwrap_or_else(|| "unknown".into()),
                body: s(c, "/content/raw").into(),
                created_at: s(c, "/created_on").into(),
                url: opt(c, "/links/html/href"),
                state: None,
                path: inline.and_then(|i| opt(i, "/path")),
                line: to.or(from),
                side: inline.map(|_| if to.is_some() { "RIGHT".into() } else { "LEFT".into() }),
            })
        })
        .collect()
}

/// 参与者里的结论：批准、要求修改。
pub(crate) fn map_reviews(pr: &Value) -> Vec<PrCommentDto> {
    pr.get("participants")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|p| {
            let state = if p.get("approved").and_then(|a| a.as_bool()).unwrap_or(false) {
                "APPROVED"
            } else if s(p, "/state") == "changes_requested" {
                "CHANGES_REQUESTED"
            } else {
                return None;
            };
            let who = opt(p, "/user/nickname")
                .or_else(|| opt(p, "/user/display_name"))
                .unwrap_or_else(|| "unknown".into());
            Some(PrCommentDto {
                id: format!("review:{}:{who}", state.to_lowercase()),
                author: who,
                body: String::new(),
                created_at: s(p, "/participated_on").into(),
                url: None,
                state: Some(state.into()),
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
                path: opt(f, "/new/path").or_else(|| opt(f, "/old/path"))?,
                status: match s(f, "/status") {
                    "added" => "added",
                    "removed" => "removed",
                    "renamed" => "renamed",
                    _ => "modified",
                }
                .into(),
                additions: f.get("lines_added").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
                deletions: f.get("lines_removed").and_then(|n| n.as_u64()).unwrap_or(0) as u32,
                patch: None,
            })
        })
        .collect()
}

/// 400 的原话归类（官方没列原文，按关键词）。
pub(crate) fn classify_refusal(message: &str, url: &str) -> String {
    let low = message.to_lowercase();
    if low.contains("conflict") {
        format!("gh/merge_conflict: {url}")
    } else if low.contains("approv") || low.contains("changes requested") {
        "gh/review_required".into()
    } else if low.contains("build") || low.contains("check") {
        "gh/checks_failed".into()
    } else if low.contains("strategy") {
        "gh/method_not_allowed".into()
    } else {
        format!("gh/merge_blocked: {message}")
    }
}

impl Bitbucket<'_> {
    fn repo(&self, slug: &str) -> String {
        let (ws, name) = slug.split_once('/').unwrap_or((slug, ""));
        format!("https://{API_HOST}/2.0/repositories/{}/{}", enc(ws), enc(name))
    }

    /// 按 `next` 翻页取全 values（最多 pages 页，下一页必须还在 API 主机上）。
    fn all(&self, url: &str, pages: usize) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        let mut next = Some(url.to_string());
        for _ in 0..pages {
            let Some(current) = next.take() else { break };
            let page = self.client.get(&current)?;
            out.extend(page.get("values").and_then(|v| v.as_array()).cloned().unwrap_or_default());
            next = opt(&page, "/next").filter(|n| super::http::allowed(API_HOST, n));
        }
        Ok(out)
    }

    fn checks(&self, slug: &str, id: u32) -> Vec<PrCheckDto> {
        self.all(&format!("{}/pullrequests/{id}/statuses?pagelen=100", self.repo(slug)), 3)
            .map(|v| map_statuses(&v))
            .unwrap_or_default()
    }

    fn list_branch(&self, slug: &str, branch: &str, open_only: bool) -> Result<Vec<Value>, String> {
        let q = format!("source.branch.name=\"{}\"", branch.replace('\\', "\\\\").replace('"', "\\\""));
        let states = if open_only {
            "&state=OPEN".to_string()
        } else {
            "&state=OPEN&state=MERGED&state=DECLINED&state=SUPERSEDED".to_string()
        };
        let url = format!(
            "{}/pullrequests?q={}{states}&sort=-created_on&pagelen=30&fields={}",
            self.repo(slug),
            enc(&q),
            enc(LIST_FIELDS)
        );
        Ok(self
            .client
            .get(&url)?
            .get("values")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default())
    }

    fn find_raw(&self, slug: &str, target: &Target) -> Result<Option<Value>, String> {
        match target {
            Target::Number(n) => self.client.get_opt(&format!("{}/pullrequests/{n}", self.repo(slug))),
            Target::Branch { branch, head_slug, .. } => {
                let list = self.list_branch(slug, branch, false)?;
                Ok(pick_pr(&list, head_slug.unwrap_or(slug)).cloned())
            }
        }
    }
}

impl ForgeApi for Bitbucket<'_> {
    fn platform(&self) -> Platform {
        Platform::Bitbucket
    }

    fn state(&self, _refresh: bool) -> GhState {
        self.state
    }

    /// 仓库级没有「允许的合并方式」接口（在目标分支上，随 PR 给）：先列三种，合并时按 PR 的目标分支再核对。
    fn merge_methods(&self, _dir: &str, _slug: &str) -> Result<Vec<String>, String> {
        Ok(vec!["merge".into(), "squash".into(), "rebase".into()])
    }

    fn find(&self, _dir: &str, slug: Option<&str>, target: &Target) -> Result<Option<PullRequestDto>, String> {
        let slug = slug.ok_or("gh/ambiguous_repo")?;
        let Some(pr) = self.find_raw(slug, target)? else { return Ok(None) };
        let number = pr.get("id").and_then(|i| i.as_u64()).unwrap_or_default() as u32;
        let checks = if s(&pr, "/state") == "OPEN" {
            self.checks(slug, number)
        } else {
            Vec::new()
        };
        map_pr(&pr, checks).map(Some).ok_or_else(|| "gh/failed: bad pull request json".into())
    }

    fn find_url(&self, _dir: &str, url: &str) -> Result<Option<PullRequestDto>, String> {
        let (_, slug, number, platform) = super::platform::parse_pr_url(url).ok_or("gh/failed: bad url")?;
        if platform != Platform::Bitbucket {
            return Ok(None);
        }
        let Some(pr) = self.client.get_opt(&format!(
            "{}/pullrequests/{number}?fields={}",
            self.repo(&slug),
            enc(&LIST_FIELDS.replace("values.", ""))
        ))?
        else {
            return Ok(None);
        };
        Ok(map_pr(&pr, Vec::new()).map(|pr| PullRequestDto {
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
        // 同分支已有打开的 PR：直接给出那个（Bitbucket 不保证回「已存在」）。
        let open = self.list_branch(slug, pr.branch, true)?;
        if let Some(existing) = pick_pr(&open, head_repo) {
            return Err(format!("gh/pr_exists: {}", s(existing, "/links/html/href")));
        }
        let mut source = json!({ "branch": { "name": pr.branch } });
        if pr.head_slug.is_some() {
            source["repository"] = json!({ "full_name": head_repo });
        }
        let body = json!({
            "title": pr.title,
            "description": pr.body,
            "source": source,
            "destination": { "branch": { "name": pr.base } },
            "draft": pr.draft,
            "close_source_branch": false,
        });
        let resp = self.client.call("POST", &format!("{}/pullrequests", self.repo(slug)), Some(body))?;
        if ok(&resp) {
            return Ok(s(&json(&resp.body), "/links/html/href").to_string());
        }
        Err(http_error(&resp))
    }

    fn merge(&self, _dir: &str, slug: &str, pr: &PullRequestDto, method: &str, head: &str) -> Result<bool, String> {
        let strategy = strategy(method).ok_or("gh/method_not_allowed")?;
        // 没有 sha 参数：合并前再读一次，头提交变了就不合。
        let fresh = self
            .client
            .get_opt(&format!("{}/pullrequests/{}", self.repo(slug), pr.number))?
            .ok_or("gh/head_changed")?;
        let current = s(&fresh, "/source/commit/hash");
        if current.is_empty() || !(head.starts_with(current) || current.starts_with(head)) {
            return Err("gh/head_changed".into());
        }
        if let Some(allowed) = fresh.pointer("/destination/branch/merge_strategies").and_then(|m| m.as_array()) {
            if !allowed.iter().any(|m| m.as_str() == Some(strategy)) {
                return Err("gh/method_not_allowed".into());
            }
        }
        let body = json!({ "merge_strategy": strategy, "close_source_branch": false });
        let resp = self
            .client
            .call("POST", &format!("{}/pullrequests/{}/merge", self.repo(slug), pr.number), Some(body))?;
        match resp.status {
            200 | 201 => Ok(false),
            // 转成异步或进了合并队列：按「排队中」报，下一次刷新以服务器为准。
            202 => Ok(true),
            400 => Err(classify_refusal(&message_of(&resp), &pr.url)),
            409 => Err("gh/head_changed".into()),
            555 => Err("gh/failed: timeout".into()),
            _ => Err(http_error(&resp)),
        }
    }

    fn review(&self, _dir: &str, slug: &str, branch: &str) -> Result<Option<PrReviewDto>, String> {
        let target = Target::Branch {
            qualified: branch,
            branch,
            head_slug: None,
        };
        let Some(listed) = self.find_raw(slug, &target)? else { return Ok(None) };
        let number = listed.get("id").and_then(|i| i.as_u64()).unwrap_or_default() as u32;
        let base = format!("{}/pullrequests/{number}", self.repo(slug));
        let full = self.client.get(&format!("{base}?fields=-summary,-rendered,-description"))?;
        let checks = if s(&full, "/state") == "OPEN" {
            self.checks(slug, number)
        } else {
            Vec::new()
        };
        let pr = map_pr(&full, checks).ok_or("gh/failed: bad pull request json")?;
        let comments = map_comments(&self.all(&format!("{base}/comments?pagelen=100"), 10)?);
        let files = map_files(&self.all(&format!("{base}/diffstat?pagelen=500"), 5)?);
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
            &format!("{}/pullrequests/{number}/comments", self.repo(slug)),
            Some(json!({ "content": { "raw": body } })),
        )?;
        if ok(&resp) {
            Ok(())
        } else {
            Err(http_error(&resp))
        }
    }

    fn line_comment(&self, _dir: &str, slug: &str, number: u32, path: &str, line: i64, side: &str, body: &str) -> Result<(), String> {
        let inline = if side == "OLD" {
            json!({ "path": path, "from": line })
        } else {
            json!({ "path": path, "to": line })
        };
        let resp = self.client.call(
            "POST",
            &format!("{}/pullrequests/{number}/comments", self.repo(slug)),
            Some(json!({ "content": { "raw": body }, "inline": inline })),
        )?;
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

/// 令牌好不好用、是谁：`GET /2.0/user`（要 read:user:bitbucket）。展示名优先用 nickname（官方建议替代 username）。
pub(crate) fn current_user(client: &Client) -> Result<(String, Option<String>), String> {
    let v = client.get(&format!("https://{API_HOST}/2.0/user"))?;
    let login = opt(&v, "/nickname").or_else(|| opt(&v, "/username")).ok_or("gh/failed: no nickname")?;
    Ok((login, opt(&v, "/display_name")))
}
