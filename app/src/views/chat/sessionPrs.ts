/// 这个会话里 agent 建过、动过的 Pull Request。
///
/// 标题栏的 PR 原先只按会话目录的当前分支查。agent 常在临时 worktree 里另切分支建 PR，建完把
/// worktree 删掉，会话目录停在原来的分支上，于是标题栏显示的是那个分支的旧 PR（2026-11-16 实拍：
/// 刚建了 #536，标题栏和「更改」面板仍是已合并的 #530）。
///
/// 证据只取 agent 自己跑的 `gh pr …` 命令：命令行和它的输出里出现的 PR 链接。正文里提到的链接不算
/// （回顾旧 PR、贴参考链接都会出现），`gh pr list` / `gh pr status` 列出的是一串别人的 PR，也不算。
/// 五家都把命令类工具解析成同一种 command 详情（见 ToolDetail），这里不认任何一家的名字。
import { createContext } from "react";
import type { ChatItem } from "../../api";

/// 会话 PR 链接（最新在最后）交给「更改」面板：面板在对话窗之外几层，用 context 传，不逐层加属性。
export const SessionPrsContext = createContext<readonly string[]>([]);

/// 属于这个仓库的会话 PR，最新在前；排除面板已经按分支显示的那个和用户收起的。repoUrl 形如 https://github.com/o/r。
/// segment：仓库首页之后 PR 链接的路径段（GitLab 是 `/-/merge_requests/`，见 forgeWords.prPathSegment）。
export function sessionPrsInRepo(urls: readonly string[], repoUrl: string | null | undefined, shown: string | null | undefined, dismissed: readonly string[] = [], limit = 3, segment = "/pull/"): string[] {
  if (!repoUrl) return [];
  const prefix = repoUrl.replace(/\/+$/, "").toLowerCase() + segment;
  return urls.filter((url) => url.toLowerCase().startsWith(prefix) && url !== shown && !dismissed.includes(url)).reverse().slice(0, limit);
}

// gh pr …、glab mr …（GitLab）、tea pr / pulls …（Gitea / Forgejo）。
const GH_PR = /(?:^|[\s;&|(`"'])(?:gh(?:\.exe)?\s+pr|glab(?:\.exe)?\s+mr|tea(?:\.exe)?\s+(?:pr|pulls?))\s+(\S+)/i;
const LISTING = new Set(["list", "status"]);
// GitHub `/o/r/pull/N`、Gitea `/o/r/pulls/N`、Bitbucket `/w/r/pull-requests/N`、GitLab `/组…/r/-/merge_requests/N`。
const PR_URL = /https:\/\/[^\s/"'<>()]+\/[^\s/"'<>()]+\/[^\s/"'<>()]+\/(?:pull|pulls|pull-requests)\/\d+(?![\d/])|https:\/\/[^\s/"'<>()]+(?:\/[^\s/"'<>()]+){2,}\/-\/merge_requests\/\d+(?![\d/])/g;

/// 按最近一次出现的先后排好，最新的在最后。同一个 PR 只出现一次。
export function sessionPullRequestUrls(items: readonly ChatItem[]): string[] {
  const commands = new Map<string, string>();
  const seen = new Map<string, number>();
  let order = 0;
  const note = (text: string) => {
    for (const match of text.matchAll(PR_URL)) seen.set(match[0], order++);
  };
  for (const item of items) {
    if (item.type === "tool_use") {
      const command = item.detail?.kind === "command" ? item.detail.command : null;
      const sub = command ? GH_PR.exec(command)?.[1]?.toLowerCase() : undefined;
      if (!command || !sub || LISTING.has(sub)) continue;
      commands.set(item.id, command);
      note(command);
    } else if (item.type === "tool_result" && item.tool_use_id && commands.has(item.tool_use_id) && !item.is_error) {
      note(item.text);
    }
  }
  return [...seen.entries()].sort((a, b) => a[1] - b[1]).map(([url]) => url);
}

/// 标题栏该看哪个 PR：会话里最近动过的那个；一个都没有时交回按分支查。
export function latestSessionPullRequest(items: readonly ChatItem[]): string | null {
  const urls = sessionPullRequestUrls(items);
  return urls[urls.length - 1] ?? null;
}

/// 手动收起的 PR（「更改」面板里已合并、已关闭的那几条，点 × 不再列）。按链接记，跨会话通用：
/// 已经了结的 PR 在哪个会话里都不用再看。只是本机的显示偏好，读写失败就当没收起过。
const DISMISSED_KEY = "meowo-dismissed-prs";
const DISMISSED_LIMIT = 200;

export function loadDismissedPrs(): string[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(DISMISSED_KEY) ?? "[]");
    return Array.isArray(raw) ? raw.filter((url): url is string => typeof url === "string") : [];
  } catch {
    return [];
  }
}

/// 记下一条收起的 PR，返回新的列表。只留最近 200 条，旧的挤掉（早就不在会话里出现了）。
export function dismissPr(url: string): string[] {
  const next = [...loadDismissedPrs().filter((item) => item !== url), url].slice(-DISMISSED_LIMIT);
  try { localStorage.setItem(DISMISSED_KEY, JSON.stringify(next)); } catch { /* 存不下就只在这次生效 */ }
  return next;
}
