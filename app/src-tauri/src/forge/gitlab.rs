//! GitLab（gitlab.com 与自管实例，REST v4）的合并请求（MR）。
//!
//! 依据（2026-11 查证，见 .lead/20261118-multi-forge/progress.md）：
//! - MR：docs.gitlab.com/api/merge_requests（`state` opened/closed/locked/merged；`draft`；`detailed_merge_status`
//!   全部取值；`head_pipeline` 只给看得到流水线的人；fork 的流水线在源项目里，jobs 要按 `head_pipeline.project_id` 查）。
//! - 流水线 job：docs.gitlab.com/api/jobs（状态全集；`allow_failure`）。
//! - 合并：`PUT …/merge`，`sha` 不符回 409，`squash`；合并方式（merge / rebase_merge / ff）由项目设置决定，
//!   API 只能选要不要 squash（`squash_option` never/always/default_on/default_off，`GET /projects/:id`）。
//!   不能合并时回 405 或 422；没权限合并也回 401（不代表令牌失效，这里再问一次 /user 区分）。
//! - 建 MR：没有 draft 参数，用标题前缀 `Draft:`；同一源分支已有打开的 MR 时回 409
//!   `Another open merge request already exists for this source branch: !N`。
//! - 讨论：`GET …/discussions`（`system` 备注不算评论；DiffNote 带 position）；行内评论 `POST …/discussions`
//!   带 position（base/start/head sha 取 MR 的 diff_refs）。
//! - 项目 id 可写 URL 编码的路径（`group%2Fsub%2Frepo`）。
//!
//! 一次「按分支找 MR」最多 3 个请求：列表（按源分支，30 条，新的在前）→ 单个 MR（流水线、diff_refs、合并状态）
//! → 流水线的 job。fork 的 MR 另要 1 个请求拿源项目 id。

use super::api::{merge_fields, summarize, ForgeApi, NewPr, Target};
use super::http::{enc, get_all, http_error, message_of, ok, Client};
use super::platform::Platform;
use super::GhState;
use meowo_protocol::ipc::{PrCheckDto, PrCommentDto, PrFileDto, PrReviewDto, PullRequestDto};
use serde_json::{json, Value};

pub(crate) struct Gitlab<'a> {
    pub(crate) client: Client<'a>,
    /// 主机（可带端口），API 在 `https://<主机>/api/v4`。
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

/// job 状态 → pass / fail / pending / skipped。允许失败的 job 失败了不算挡合并（GitLab 显示为警告）；
/// 手动 job 默认不挡流水线，`allow_failure: false` 的手动 job 会让流水线停在 blocked，按进行中。
pub(crate) fn job_state(status: &str, allow_failure: bool) -> &'static str {
    match status {
        "success" => "pass",
        "failed" if allow_failure => "skipped",
        "failed" | "canceled" | "canceling" => "fail",
        "skipped" => "skipped",
        "manual" if allow_failure => "skipped",
        _ => "pending", // created / waiting_for_resource / preparing / waiting_for_callback / pending / running / scheduled / manual(阻塞)
    }
}

/// 流水线整体状态 → 同上（拿不到 job 明细时用它当一项检查）。
pub(crate) fn pipeline_state(status: &str) -> &'static str {
    match status {
        "success" => "pass",
        "failed" | "canceled" | "canceling" => "fail",
        "skipped" | "manual" => "skipped",
        _ => "pending",
    }
}

/// `detailed_merge_status`（+ `has_conflicts`）→ 统一的 mergeable / merge_state。
pub(crate) fn merge_status(mr: &Value) -> (String, String) {
    let status = s(mr, "/detailed_merge_status");
    let conflict = mr.get("has_conflicts").and_then(|c| c.as_bool()).unwrap_or(false) || matches!(status, "conflict" | "broken_status");
    let unknown = matches!(status, "unchecked" | "checking" | "preparing" | "approvals_syncing" | "");
    let draft = status == "draft_status" || mr.get("draft").and_then(|d| d.as_bool()).unwrap_or(false);
    let (mergeable, mut state) = merge_fields(conflict, unknown, draft, status != "mergeable");
    if status == "need_rebase" && !conflict {
        state = "BEHIND".into();
    }
    (mergeable, state)
}

/// MR（单个 MR 的回包）+ job 列表 → 统一的 PR。
pub(crate) fn map_mr(mr: &Value, jobs: Option<&[Value]>) -> Option<PullRequestDto> {
    let number = u32::try_from(mr.get("iid")?.as_u64()?).ok()?;
    let draft = mr.get("draft").and_then(|d| d.as_bool()).unwrap_or(false);
    let state = match s(mr, "/state") {
        "opened" | "locked" if draft => "draft",
        "opened" | "locked" => "open",
        "merged" => "merged",
        "closed" => "closed",
        _ => return None,
    };
    let checks: Vec<PrCheckDto> = match (jobs, mr.get("head_pipeline").filter(|p| !p.is_null())) {
        (Some(jobs), _) if !jobs.is_empty() => jobs
            .iter()
            .map(|job| PrCheckDto {
                name: s(job, "/name").to_string(),
                workflow: opt(job, "/stage"),
                state: job_state(s(job, "/status"), job.get("allow_failure").and_then(|a| a.as_bool()).unwrap_or(false)).into(),
                url: opt(job, "/web_url"),
            })
            .collect(),
        (_, Some(pipeline)) => vec![PrCheckDto {
            name: format!("pipeline #{}", pipeline.get("id").and_then(|i| i.as_u64()).unwrap_or_default()),
            workflow: None,
            state: pipeline_state(s(pipeline, "/status")).into(),
            url: opt(pipeline, "/web_url"),
        }],
        _ => Vec::new(),
    };
    let (mergeable, merge_state) = merge_status(mr);
    Some(PullRequestDto {
        number,
        state: state.into(),
        url: s(mr, "/web_url").into(),
        title: s(mr, "/title").into(),
        base_ref: opt(mr, "/target_branch"),
        head_ref: opt(mr, "/source_branch"),
        head_oid: opt(mr, "/sha"),
        mergeable: Some(mergeable),
        merge_state: Some(merge_state),
        checks_summary: Some(summarize(&checks)),
        checks,
        queued: false,
        forge: Some("gitlab".into()),
    })
}

/// 列表里挑哪个 MR（照 gh 的挑法）：源项目对得上；打开的排前面；其余按列表顺序（新的在前）。
pub(crate) fn pick_mr(list: &[Value], source_project: Option<u64>) -> Option<&Value> {
    let source_ok = |m: &Value| {
        let src = m.get("source_project_id").and_then(|v| v.as_u64());
        match source_project {
            Some(id) => src == Some(id),
            // 同仓库：源项目就是目标项目。
            None => src.is_some() && src == m.get("target_project_id").and_then(|v| v.as_u64()),
        }
    };
    let mut sorted: Vec<&Value> = list.iter().filter(|m| source_ok(m)).collect();
    sorted.sort_by_key(|m| s(m, "/state") != "opened");
    sorted.into_iter().next()
}

/// diff 文本里的增删行数（GitLab 的 diffs 接口不给行数）。
pub(crate) fn count_lines(diff: &str) -> (u32, u32) {
    diff.lines().fold((0, 0), |(add, del), line| {
        if line.starts_with('+') && !line.starts_with("+++") {
            (add + 1, del)
        } else if line.starts_with('-') && !line.starts_with("---") {
            (add, del + 1)
        } else {
            (add, del)
        }
    })
}

pub(crate) fn map_files(diffs: &[Value]) -> Vec<PrFileDto> {
    diffs
        .iter()
        .map(|d| {
            let flag = |k: &str| d.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
            let status = if flag("new_file") {
                "added"
            } else if flag("deleted_file") {
                "removed"
            } else if flag("renamed_file") {
                "renamed"
            } else {
                "modified"
            };
            let diff = s(d, "/diff");
            let (additions, deletions) = count_lines(diff);
            PrFileDto {
                path: opt(d, "/new_path").or_else(|| opt(d, "/old_path")).unwrap_or_default(),
                status: status.into(),
                additions,
                deletions,
                patch: (!diff.is_empty()).then(|| diff.to_string()),
            }
        })
        .collect()
}

/// 讨论 → 评论（系统备注不算；DiffNote 带文件与行，新行算 RIGHT、只有旧行算 LEFT，与 GitHub 的叫法一致）。
pub(crate) fn map_discussions(discussions: &[Value], web_url: &str) -> Vec<PrCommentDto> {
    discussions
        .iter()
        .flat_map(|d| d.get("notes").and_then(|n| n.as_array()).cloned().unwrap_or_default())
        .filter(|note| !note.get("system").and_then(|v| v.as_bool()).unwrap_or(false))
        .filter_map(|note| {
            let id = note.get("id")?.as_u64()?;
            let new_line = note.pointer("/position/new_line").and_then(|v| v.as_i64());
            let old_line = note.pointer("/position/old_line").and_then(|v| v.as_i64());
            let path = opt(&note, "/position/new_path").or_else(|| opt(&note, "/position/old_path"));
            Some(PrCommentDto {
                id: format!("comment:{id}"),
                author: opt(&note, "/author/username").unwrap_or_else(|| "unknown".into()),
                body: s(&note, "/body").into(),
                created_at: s(&note, "/created_at").into(),
                url: Some(format!("{web_url}#note_{id}")),
                state: None,
                path: path.filter(|_| note.get("position").is_some_and(|p| !p.is_null())),
                line: new_line.or(old_line),
                side: note
                    .get("position")
                    .filter(|p| !p.is_null())
                    .map(|_| if new_line.is_some() { "RIGHT".into() } else { "LEFT".into() }),
            })
        })
        .collect()
}

/// 批准 → 评审结论（GitLab 的「批准」相当于 GitHub 的 APPROVED 评审）。
pub(crate) fn map_approvals(approvals: &Value) -> Vec<PrCommentDto> {
    approvals
        .get("approved_by")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|a| {
            let user = opt(a, "/user/username")?;
            Some(PrCommentDto {
                id: format!("review:approval:{user}"),
                author: user,
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

/// 一侧的行号 → (旧文件行号, 新文件行号)：新增行只有新、删除行只有旧、上下文行（含 diff 块之外没改动的行）两个都有。
/// side：NEW / OLD（面板的写法）。diff 是 GitLab diffs 接口里这个文件的统一 diff 文本。
pub(crate) fn diff_position(diff: &str, side: &str, line: i64) -> (Option<i64>, Option<i64>) {
    let old_side = side == "OLD";
    // 块外的行：两侧行号差一个固定偏移（之前各块的增删累计）。
    let mut delta = 0i64; // new - old
    let (mut old_no, mut new_no) = (0i64, 0i64);
    let mut in_hunk = false;
    let outside = |delta: i64| {
        if old_side {
            (Some(line), Some(line + delta))
        } else {
            (Some(line - delta), Some(line))
        }
    };
    for text in diff.lines() {
        if let Some(header) = text.strip_prefix("@@ ") {
            // @@ -a,b +c,d @@
            let mut parts = header.split_whitespace();
            let parse = |part: Option<&str>, sign: char| -> Option<(i64, i64)> {
                let body = part?.strip_prefix(sign)?;
                let (start, count) = body.split_once(',').unwrap_or((body, "1"));
                Some((start.parse().ok()?, count.parse().ok()?))
            };
            let (Some((old_start, old_count)), Some((new_start, new_count))) = (parse(parts.next(), '-'), parse(parts.next(), '+')) else {
                continue;
            };
            // 要找的行在这一块之前（块与块之间没改动的部分）。
            let before = if old_side { line < old_start } else { line < new_start };
            if before {
                return outside(delta);
            }
            old_no = old_start;
            new_no = new_start;
            in_hunk = true;
            delta = (new_start + new_count) - (old_start + old_count);
            continue;
        }
        if !in_hunk {
            continue;
        }
        match text.chars().next() {
            Some('+') => {
                if !old_side && new_no == line {
                    return (None, Some(line));
                }
                new_no += 1;
            }
            Some('-') => {
                if old_side && old_no == line {
                    return (Some(line), None);
                }
                old_no += 1;
            }
            Some('\\') => {}
            _ => {
                if (old_side && old_no == line) || (!old_side && new_no == line) {
                    return (Some(old_no), Some(new_no));
                }
                old_no += 1;
                new_no += 1;
            }
        }
    }
    outside(delta)
}

/// 409 的原话里抠出已有 MR 的号：`… source branch: !7`。
pub(crate) fn existing_iid(message: &str) -> Option<u32> {
    let (_, rest) = message.rsplit_once('!')?;
    rest.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()
}

impl Gitlab<'_> {
    fn api(&self, path: &str) -> String {
        format!("https://{}/api/v4/{path}", self.host)
    }

    fn project(&self, slug: &str) -> String {
        self.api(&format!("projects/{}", enc(slug)))
    }

    fn mr_url(&self, slug: &str, iid: u32) -> String {
        format!("{}/merge_requests/{iid}", self.project(slug))
    }

    fn project_id(&self, slug: &str) -> Result<u64, String> {
        let v = self.client.get(&self.project(slug))?;
        v.get("id").and_then(|i| i.as_u64()).ok_or_else(|| "gh/failed: bad project json".into())
    }

    /// 单个 MR + 流水线 job → 统一的 PR。
    fn load(&self, slug: &str, iid: u32) -> Result<Option<(Value, PullRequestDto)>, String> {
        let Some(mr) = self.client.get_opt(&self.mr_url(slug, iid))? else {
            return Ok(None);
        };
        let jobs = match mr.get("head_pipeline").filter(|p| !p.is_null()) {
            Some(pipeline) => {
                let project = pipeline.get("project_id").and_then(|p| p.as_u64());
                let id = pipeline.get("id").and_then(|p| p.as_u64());
                match (project, id) {
                    // 拿不到 job（权限、网络）：退回用流水线整体状态当一项检查。
                    (Some(project), Some(id)) => self
                        .client
                        .get(&self.api(&format!("projects/{project}/pipelines/{id}/jobs?per_page=100")))
                        .ok()
                        .and_then(|v| v.as_array().cloned()),
                    _ => None,
                }
            }
            None => None,
        };
        let pr = map_mr(&mr, jobs.as_deref()).ok_or("gh/failed: bad merge request json")?;
        Ok(Some((mr, pr)))
    }

    fn find_mr(&self, slug: &str, target: &Target) -> Result<Option<(Value, PullRequestDto)>, String> {
        match target {
            Target::Number(n) => self.load(slug, *n),
            Target::Branch { branch, head_slug, .. } => {
                let url = format!(
                    "{}/merge_requests?source_branch={}&order_by=created_at&sort=desc&per_page=30",
                    self.project(slug),
                    enc(branch)
                );
                let list = self.client.get(&url)?;
                let list = list.as_array().cloned().unwrap_or_default();
                if list.is_empty() {
                    return Ok(None);
                }
                let source = match head_slug {
                    Some(head) => Some(self.project_id(head)?),
                    None => None,
                };
                let Some(found) = pick_mr(&list, source) else { return Ok(None) };
                // 只剩已合并 / 已关闭的，而这个分支就是默认分支：不算（同 gh，默认分支上的旧 MR 没意义）。
                if s(found, "/state") != "opened" && head_slug.is_none() {
                    let project = self.client.get(&self.project(slug))?;
                    if project.get("default_branch").and_then(|b| b.as_str()) == Some(branch) {
                        return Ok(None);
                    }
                }
                let iid = found
                    .get("iid")
                    .and_then(|i| i.as_u64())
                    .and_then(|i| u32::try_from(i).ok())
                    .ok_or("gh/failed: bad merge request json")?;
                self.load(slug, iid)
            }
        }
    }

    /// 合并被拒时按 MR 当前的 detailed_merge_status 说清楚原因。
    fn merge_refused(&self, slug: &str, pr: &PullRequestDto, message: &str) -> String {
        let status = self
            .client
            .get(&self.mr_url(slug, pr.number))
            .ok()
            .map(|mr| s(&mr, "/detailed_merge_status").to_string())
            .unwrap_or_default();
        match status.as_str() {
            "not_approved" | "requested_changes" => "gh/review_required".into(),
            "conflict" | "broken_status" => format!("gh/merge_conflict: {}", pr.url),
            "ci_still_running" => "gh/checks_pending".into(),
            "ci_must_pass" | "status_checks_must_pass" => "gh/checks_failed".into(),
            "checking" | "unchecked" | "preparing" | "approvals_syncing" => "gh/checks_pending".into(),
            "" => format!("gh/merge_blocked: {message}"),
            other => format!("gh/merge_blocked: {other}"),
        }
    }
}

impl ForgeApi for Gitlab<'_> {
    fn platform(&self) -> Platform {
        Platform::Gitlab
    }

    fn state(&self, _refresh: bool) -> GhState {
        self.state
    }

    fn merge_methods(&self, _dir: &str, slug: &str) -> Result<Vec<String>, String> {
        let project = self.client.get(&self.project(slug))?;
        // merge 指「按项目的合并方式（合并提交 / 变基后合并 / 快进）」，squash 指压缩后再按项目方式合。
        Ok(match project.get("squash_option").and_then(|o| o.as_str()) {
            Some("always") => vec!["squash".into()],
            Some("never") => vec!["merge".into()],
            _ => vec!["merge".into(), "squash".into()],
        })
    }

    fn find(&self, _dir: &str, slug: Option<&str>, target: &Target) -> Result<Option<PullRequestDto>, String> {
        let slug = slug.ok_or("gh/ambiguous_repo")?;
        Ok(self.find_mr(slug, target)?.map(|(_, pr)| pr))
    }

    fn find_url(&self, _dir: &str, url: &str) -> Result<Option<PullRequestDto>, String> {
        let (_, slug, iid, platform) = super::platform::parse_pr_url(url).ok_or("gh/failed: bad url")?;
        if platform != Platform::Gitlab {
            return Ok(None);
        }
        let Some(mr) = self.client.get_opt(&self.mr_url(&slug, iid))? else {
            return Ok(None);
        };
        Ok(map_mr(&mr, None).map(|pr| PullRequestDto {
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
        // fork：在源项目上建，目标项目写 target_project_id。
        let (source, target_project) = match pr.head_slug {
            Some(head) => (head, Some(self.project_id(slug)?)),
            None => (slug, None),
        };
        let title = if pr.draft {
            format!("Draft: {}", pr.title)
        } else {
            pr.title.to_string()
        };
        let mut body = json!({ "source_branch": pr.branch, "target_branch": pr.base, "title": title, "description": pr.body });
        if let Some(id) = target_project {
            body["target_project_id"] = json!(id);
        }
        let resp = self
            .client
            .call("POST", &format!("{}/merge_requests", self.project(source)), Some(body))?;
        if ok(&resp) {
            return Ok(s(&super::http::json(&resp.body), "/web_url").to_string());
        }
        let message = message_of(&resp);
        if resp.status == 409 {
            let url = existing_iid(&message)
                .map(|iid| format!("https://{}/{slug}/-/merge_requests/{iid}", self.host))
                .unwrap_or_default();
            return Err(format!("gh/pr_exists: {url}"));
        }
        Err(http_error(&resp))
    }

    fn merge(&self, _dir: &str, slug: &str, pr: &PullRequestDto, method: &str, head: &str) -> Result<bool, String> {
        let body = json!({ "sha": head, "squash": method == "squash", "should_remove_source_branch": false });
        let resp = self.client.call("PUT", &format!("{}/merge", self.mr_url(slug, pr.number)), Some(body))?;
        if ok(&resp) {
            return Ok(false);
        }
        let message = message_of(&resp);
        Err(match resp.status {
            409 => "gh/head_changed".into(),
            405 | 406 | 422 => self.merge_refused(slug, pr, &message),
            // 没有合并权限时 GitLab 也回 401：令牌还好用（/user 成功）就是权限问题，不标「要重新登录」。
            401 if self.client.get(&self.api("user")).is_ok() => format!("forge/forbidden: {message}"),
            _ => http_error(&resp),
        })
    }

    fn review(&self, _dir: &str, slug: &str, branch: &str) -> Result<Option<PrReviewDto>, String> {
        let target = Target::Branch {
            qualified: branch,
            branch,
            head_slug: None,
        };
        let Some((mr, pr)) = self.find_mr(slug, &target)? else { return Ok(None) };
        let base = self.mr_url(slug, pr.number);
        let host = self.host.clone();
        let discussions = get_all(&self.client, &host, &format!("{base}/discussions?per_page=100"), 10)?;
        let approvals = self.client.get(&format!("{base}/approvals")).unwrap_or(Value::Null);
        let diffs = match get_all(&self.client, &host, &format!("{base}/diffs?per_page=100"), 10) {
            Ok(diffs) => diffs,
            // 15.7 以前的自管实例没有 /diffs：退回 /changes。
            Err(e) if e.starts_with("forge/not_found") => self
                .client
                .get(&format!("{base}/changes"))?
                .get("changes")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default(),
            Err(e) => return Err(e),
        };
        Ok(Some(PrReviewDto {
            comments: map_discussions(&discussions, s(&mr, "/web_url")),
            reviews: map_approvals(&approvals),
            files: map_files(&diffs),
            pr,
            monitor: None,
        }))
    }

    fn comment(&self, _dir: &str, slug: &str, number: u32, body: &str, _review: bool) -> Result<(), String> {
        let resp = self
            .client
            .call("POST", &format!("{}/notes", self.mr_url(slug, number)), Some(json!({ "body": body })))?;
        if ok(&resp) {
            Ok(())
        } else {
            Err(http_error(&resp))
        }
    }

    fn line_comment(&self, _dir: &str, slug: &str, number: u32, path: &str, line: i64, side: &str, body: &str) -> Result<(), String> {
        let mr = self.client.get_opt(&self.mr_url(slug, number))?.ok_or("pr/not_found")?;
        let refs = mr.get("diff_refs").filter(|r| !r.is_null()).ok_or("gh/head_changed")?;
        // GitLab 文档（discussions「Create a new thread in the merge request diff」）：新增行只填 new_line、删除行只填
        // old_line、没改动的上下文行两个都要填；重命名的文件 old_path 是旧路径。面板只给「哪一侧第几行」，这里按 MR 的
        // diff 推出另一侧的行号与旧路径；找不到这个文件的 diff 时按面板给的一侧填。
        let diffs = get_all(&self.client, &self.host, &format!("{}/diffs?per_page=100", self.mr_url(slug, number)), 10).unwrap_or_default();
        let entry = diffs
            .iter()
            .find(|d| s(d, "/new_path") == path)
            .or_else(|| diffs.iter().find(|d| s(d, "/old_path") == path));
        let old_path = entry.map(|d| s(d, "/old_path")).filter(|p| !p.is_empty()).unwrap_or(path);
        let new_path = entry.map(|d| s(d, "/new_path")).filter(|p| !p.is_empty()).unwrap_or(path);
        let (old_line, new_line) = match entry {
            Some(d) => diff_position(s(d, "/diff"), side, line),
            None if side == "OLD" => (Some(line), None),
            None => (None, Some(line)),
        };
        let mut position = json!({
            "position_type": "text",
            "base_sha": s(refs, "/base_sha"),
            "start_sha": s(refs, "/start_sha"),
            "head_sha": s(refs, "/head_sha"),
            "old_path": old_path,
            "new_path": new_path,
        });
        if let Some(old) = old_line {
            position["old_line"] = json!(old);
        }
        if let Some(new) = new_line {
            position["new_line"] = json!(new);
        }
        let resp = self.client.call(
            "POST",
            &format!("{}/discussions", self.mr_url(slug, number)),
            Some(json!({ "body": body, "position": position })),
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

/// 令牌好不好用、是谁：`GET /api/v4/user`（个人访问令牌要 `read_user` 或 `api` 权限）。
pub(crate) fn current_user(client: &Client, host: &str) -> Result<(String, Option<String>), String> {
    let v = client.get(&format!("https://{host}/api/v4/user"))?;
    let login = opt(&v, "/username").ok_or("gh/failed: no username")?;
    Ok((login, opt(&v, "/name")))
}
