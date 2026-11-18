//! 统一的 PR / MR 接口：forge 上层（「更改」面板、侧栏角标、标题栏、评审、worktree 清理）只经它说话，
//! 各平台各自实现。
//!
//! - GitHub：[`GhForge`] 交给内置实现（forge/builtin.rs）直接调 REST / GraphQL；github.com 用设备码登录或令牌的
//!   账号，企业版基址换成 `https://<主机>/api/v3` 与 `/api/graphql`（forge/ghe.rs）。不调用本机 gh 命令行。
//! - GitLab / Gitea（Forgejo、Codeberg）/ Bitbucket Cloud：forge/gitlab.rs、gitea.rs、bitbucket.rs，直接调
//!   各自的 REST，HTTP 可注入（forge/http.rs）。
//!
//! 错误一律用 forge 现有的错误码（`gh/not_authed`、`gh/head_changed`、`gh/merge_conflict: <链接>`、
//! `gh/review_required`、`gh/method_not_allowed`、`gh/merge_blocked: …`、`gh/checks_failed`、
//! `gh/checks_pending`、`gh/pr_exists: <链接>`、`gh/failed: …`），另加 `forge/forbidden: …`（403，权限不够）
//! 与 `forge/network: …`（连不上）。前端的错误文案按码翻译，与平台无关。
//!
//! PR 的字段统一映射到 GitHub 的叫法（state = open/draft/merged/closed；mergeable = MERGEABLE/CONFLICTING/
//! UNKNOWN；merge_state = CLEAN/BLOCKED/BEHIND/DIRTY/DRAFT/UNKNOWN；检查 = pass/fail/pending/skipped），合并前
//! 的判定（检查、冲突、头提交）在 forge 上层只写一遍。

use super::platform::Platform;
use super::{classify_gh_failure, parse_pr_full, says_unauthed, tail, CmdOut, GhState};
use meowo_protocol::ipc::{PrReviewDto, PullRequestDto};

/// 按什么找 PR。
#[derive(Debug, Clone, Copy)]
pub(crate) enum Target<'a> {
    /// 按 head 分支。`qualified`：head 的写法（fork 是 `<owner>:<分支>`，同仓库只是分支名）；`head_slug`：fork 时
    /// head 所在仓库的路径（GitLab 要按源项目区分同名分支）。
    Branch {
        qualified: &'a str,
        branch: &'a str,
        head_slug: Option<&'a str>,
    },
    Number(u32),
}

impl Target<'_> {
    /// 内置实现（照 gh 子命令组织的内部调用约定）的位置参数。
    pub(crate) fn gh_arg(&self) -> String {
        match self {
            Target::Branch { qualified, .. } => qualified.to_string(),
            Target::Number(n) => n.to_string(),
        }
    }
}

/// 开 PR 的参数。
#[derive(Debug, Clone)]
pub(crate) struct NewPr<'a> {
    pub(crate) title: &'a str,
    pub(crate) body: &'a str,
    pub(crate) base: &'a str,
    /// head 的写法（fork 带 owner）。
    pub(crate) head: &'a str,
    /// head 的分支名。
    pub(crate) branch: &'a str,
    /// fork 时 head 所在仓库的路径（同仓库为 None）。
    pub(crate) head_slug: Option<&'a str>,
    pub(crate) draft: bool,
}

/// 一个平台（一台主机 + 一个身份）上的 PR / MR 操作。`slug` 是仓库路径（`owner/repo`、GitLab 可带子组）；
/// 内置实现没给仓库时（只有测试会这样）按目录认。
pub(crate) trait ForgeApi {
    fn platform(&self) -> Platform;
    /// 能不能用：Ok / Unauthed（令牌失效，要重新登录）。refresh：保留给要重查的平台（现在都看账号记录，不联网）。
    fn state(&self, refresh: bool) -> GhState;
    /// 仓库允许的合并方式（`merge` / `squash` / `rebase`，按网页顺序）。Err = 查不了。
    fn merge_methods(&self, dir: &str, slug: &str) -> Result<Vec<String>, String>;
    /// 找 PR（含检查）。Ok(None) = 没有。
    fn find(&self, dir: &str, slug: Option<&str>, target: &Target) -> Result<Option<PullRequestDto>, String>;
    /// 按网页链接查（会话里 agent 建过的 PR）。只要号、状态、链接、标题。
    fn find_url(&self, dir: &str, url: &str) -> Result<Option<PullRequestDto>, String>;
    /// 开 PR，回网页链接。已有：`gh/pr_exists: <链接>`。
    fn create(&self, dir: &str, slug: Option<&str>, pr: &NewPr) -> Result<String, String>;
    /// 合并（head = 确认框里看到的头提交，平台支持时钉住）。Ok(true) = 进了合并队列而不是立刻合并。
    fn merge(&self, dir: &str, slug: &str, pr: &PullRequestDto, method: &str, head: &str) -> Result<bool, String>;
    /// 评审数据：PR、讨论、评审结论、改动文件。branch 是本地分支名。
    fn review(&self, dir: &str, slug: &str, branch: &str) -> Result<Option<PrReviewDto>, String>;
    /// 普通评论（review = 以评审的形式发，平台没有这个区分时同普通评论）。
    fn comment(&self, dir: &str, slug: &str, number: u32, body: &str, review: bool) -> Result<(), String>;
    /// 行内评论：side = NEW / OLD，钉在 PR 当前的头提交上。
    #[allow(clippy::too_many_arguments)]
    fn line_comment(&self, dir: &str, slug: &str, number: u32, path: &str, line: i64, side: &str, body: &str) -> Result<(), String>;
    /// 当前账号名（监控里「自己的评论不报」）。
    fn viewer(&self, dir: &str) -> Option<String>;
}

/// 内置实现的执行入口：(目录, gh 式的子命令参数) → gh 式的输出。只是 forge/builtin.rs 内部的调用约定（照 gh 的
/// 子命令与 JSON 字段组织，便于逐字段对照实录），不会启动任何外部程序；每个子命令都翻成 GitHub 的 REST / GraphQL 请求。
pub(crate) type GhRun<'a> = dyn Fn(&str, &[&str]) -> Option<CmdOut> + Sync + 'a;

/// GitHub（github.com 与企业版）：内置实现（forge/builtin.rs）直接调 GitHub 的 REST / GraphQL，令牌来自账号。
/// Meowo 不再调用本机 gh 命令行（leader 2026-11-18「取消gh」）。
pub(crate) struct GhForge<'a> {
    pub(crate) gh: Box<GhRun<'a>>,
    /// 账号状态：Ok，或令牌失效要重新登录（Unauthed）。
    pub(crate) state: GhState,
}

impl GhForge<'_> {
    fn run(&self, dir: &str, args: &[&str]) -> Result<CmdOut, String> {
        (self.gh)(dir, args).ok_or_else(|| "gh/failed: no executor".to_string())
    }

    fn fail(&self, out: &CmdOut, pr_url: Option<&str>) -> String {
        classify_gh_failure(out, pr_url)
    }

    fn view(&self, dir: &str, slug: Option<&str>, target: Option<&str>, fields: &str) -> Result<Option<CmdOut>, String> {
        let mut args = vec!["pr", "view"];
        args.extend(target);
        if let (Some(slug), Some(_)) = (slug, target) {
            args.extend(["--repo", slug]);
        }
        args.extend(["--json", fields]);
        let out = self.run(dir, &args)?;
        if out.ok() {
            return Ok(Some(out));
        }
        if out.both().to_lowercase().contains("no pull requests found") {
            return Ok(None);
        }
        if says_unauthed(&out) {
            return Err("gh/not_authed".into());
        }
        Err(format!("gh/failed: {}", tail(&out.both(), 4)))
    }
}

impl ForgeApi for GhForge<'_> {
    fn platform(&self) -> Platform {
        Platform::Github
    }

    fn state(&self, _refresh: bool) -> GhState {
        self.state
    }

    fn merge_methods(&self, dir: &str, slug: &str) -> Result<Vec<String>, String> {
        let out = self.run(
            dir,
            &["repo", "view", slug, "--json", "mergeCommitAllowed,squashMergeAllowed,rebaseMergeAllowed"],
        )?;
        if !out.ok() {
            if says_unauthed(&out) {
                return Err("gh/not_authed".into());
            }
            return Err(format!("gh/failed: {}", tail(&out.both(), 4)));
        }
        Ok(super::parse_merge_methods(&out.stdout))
    }

    fn find(&self, dir: &str, slug: Option<&str>, target: &Target) -> Result<Option<PullRequestDto>, String> {
        let arg = target.gh_arg();
        match self.view(dir, slug, Some(&arg), super::PR_FIELDS)? {
            Some(out) => parse_pr_full(&out.stdout).map(Some).ok_or_else(|| "gh/failed: bad json".into()),
            None => Ok(None),
        }
    }

    fn find_url(&self, dir: &str, url: &str) -> Result<Option<PullRequestDto>, String> {
        let out = self.run(dir, &["pr", "view", url, "--json", "number,state,isDraft,url,title"])?;
        if !out.ok() {
            return Err(self.fail(&out, None));
        }
        Ok(crate::git::parse_pull_request(&out.stdout))
    }

    fn create(&self, dir: &str, slug: Option<&str>, pr: &NewPr) -> Result<String, String> {
        let mut args = vec![
            "pr", "create", "--title", pr.title, "--body", pr.body, "--base", pr.base, "--head", pr.head,
        ];
        if pr.draft {
            args.push("--draft");
        }
        if let Some(slug) = slug {
            args.extend(["--repo", slug]);
        }
        let out = self.run(dir, &args)?;
        if !out.ok() {
            if out.both().contains("already exists") {
                let url = out
                    .both()
                    .lines()
                    .map(str::trim)
                    .find(|l| l.starts_with("https://"))
                    .unwrap_or_default()
                    .to_string();
                return Err(format!("gh/pr_exists: {url}"));
            }
            return Err(self.fail(&out, None));
        }
        Ok(out
            .stdout
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with("https://"))
            .unwrap_or_default()
            .to_string())
    }

    fn merge(&self, dir: &str, slug: &str, pr: &PullRequestDto, method: &str, head: &str) -> Result<bool, String> {
        let number = pr.number.to_string();
        let flag = format!("--{method}");
        // 不加 --delete-branch：远端分支留着，worktree 与本地分支的清理走 worktree.rs 的判定，只提示。
        let args = ["pr", "merge", number.as_str(), flag.as_str(), "--match-head-commit", head, "--repo", slug];
        let out = self.run(dir, &args)?;
        if !out.ok() {
            return Err(self.fail(&out, Some(&pr.url)));
        }
        // 仓库开了合并队列时内置实现改为入队，回「will be added to the merge queue」，PR 还是 OPEN。
        Ok(out.both().to_lowercase().contains("merge queue"))
    }

    fn review(&self, dir: &str, slug: &str, branch: &str) -> Result<Option<PrReviewDto>, String> {
        let fields = format!("{},comments,reviews", super::PR_FIELDS);
        let out = self.run(dir, &["pr", "view", branch, "--repo", slug, "--json", &fields])?;
        if !out.ok() {
            if out.both().to_lowercase().contains("no pull requests found") {
                return Ok(None);
            }
            return Err(classify_gh_failure(&out, None));
        }
        let value: serde_json::Value = serde_json::from_str(&out.stdout).map_err(|_| "gh/failed: bad json")?;
        let number = value.get("number").and_then(|n| n.as_i64()).ok_or("gh/failed: missing number")?;
        let base = format!("repos/{slug}/pulls/{number}");
        let file_out = self.run(dir, &["api", &format!("{base}/files"), "--paginate", "--slurp"])?;
        if !file_out.ok() {
            return Err(classify_gh_failure(&file_out, None));
        }
        let file_value: serde_json::Value = serde_json::from_str(&file_out.stdout).map_err(|_| "gh/failed: bad files json")?;
        let inline_out = self.run(dir, &["api", &format!("{base}/comments"), "--paginate", "--slurp"])?;
        if !inline_out.ok() {
            return Err(classify_gh_failure(&inline_out, None));
        }
        let inline: serde_json::Value = serde_json::from_str(&inline_out.stdout).map_err(|_| "gh/failed: bad comments json")?;
        super::review::parse_review(&value, &file_value, &inline)
            .map(Some)
            .ok_or_else(|| "gh/failed: bad review json".into())
    }

    fn comment(&self, dir: &str, slug: &str, number: u32, body: &str, review: bool) -> Result<(), String> {
        let number_text = number.to_string();
        let action = if review { "review" } else { "comment" };
        let mut args = vec!["pr", action, &number_text, "--repo", slug, "--body", body];
        if review {
            args.push("--comment");
        }
        let out = self.run(dir, &args)?;
        if out.ok() {
            Ok(())
        } else {
            Err(classify_gh_failure(&out, None))
        }
    }

    fn line_comment(&self, dir: &str, slug: &str, number: u32, path: &str, line: i64, side: &str, body: &str) -> Result<(), String> {
        let number_text = number.to_string();
        let out = self.run(dir, &["pr", "view", &number_text, "--repo", slug, "--json", "headRefOid"])?;
        if !out.ok() {
            return Err(classify_gh_failure(&out, None));
        }
        let value: serde_json::Value = serde_json::from_str(&out.stdout).map_err(|_| "gh/failed: bad json")?;
        let head = value.get("headRefOid").and_then(|h| h.as_str()).ok_or("gh/head_changed")?.to_string();
        let endpoint = format!("repos/{slug}/pulls/{number}/comments");
        let line_text = line.to_string();
        // GitHub REST 的 side 只收 RIGHT（新文件这一侧）/ LEFT（旧文件）；面板传的是 NEW / OLD。
        let side = if side == "OLD" { "LEFT" } else { "RIGHT" };
        let args = [
            "api",
            endpoint.as_str(),
            "-f",
            &format!("body={body}"),
            "-f",
            &format!("commit_id={head}"),
            "-f",
            &format!("path={path}"),
            "-F",
            &format!("line={line_text}"),
            "-f",
            &format!("side={side}"),
        ];
        let out = self.run(dir, &args)?;
        if out.ok() {
            Ok(())
        } else {
            Err(classify_gh_failure(&out, None))
        }
    }

    fn viewer(&self, dir: &str) -> Option<String> {
        let out = (self.gh)(dir, &["api", "user"]).filter(|o| o.ok())?;
        let value: serde_json::Value = serde_json::from_str(&out.stdout).ok()?;
        value.get("login").and_then(|l| l.as_str()).map(str::to_string)
    }
}

// ═══ 各 HTTP 平台共用的小件 ═══

/// 检查列表 → 汇总：没有任何检查 = none；有失败 = fail；有在跑 = pending；否则 pass（与 GitHub 同一规则）。
pub(crate) fn summarize(checks: &[meowo_protocol::ipc::PrCheckDto]) -> String {
    if checks.is_empty() {
        "none"
    } else if checks.iter().any(|c| c.state == "fail") {
        "fail"
    } else if checks.iter().any(|c| c.state == "pending") {
        "pending"
    } else {
        "pass"
    }
    .to_string()
}

/// 统一状态里的 merge_state 与 mergeable（合并前判定只认这几个值，见 forge.rs pr_merge_with）。
pub(crate) fn merge_fields(conflict: bool, unknown: bool, draft: bool, blocked: bool) -> (String, String) {
    let mergeable = if conflict {
        "CONFLICTING"
    } else if unknown {
        "UNKNOWN"
    } else {
        "MERGEABLE"
    };
    let state = if conflict {
        "DIRTY"
    } else if draft {
        "DRAFT"
    } else if unknown {
        "UNKNOWN"
    } else if blocked {
        "BLOCKED"
    } else {
        "CLEAN"
    };
    (mergeable.to_string(), state.to_string())
}
