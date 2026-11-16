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

/// 属于这个仓库的会话 PR，最新在前；排除面板已经按分支显示的那个。repoUrl 形如 https://github.com/o/r。
export function sessionPrsInRepo(urls: readonly string[], repoUrl: string | null | undefined, shown: string | null | undefined, limit = 3): string[] {
  if (!repoUrl) return [];
  const prefix = repoUrl.replace(/\/+$/, "").toLowerCase() + "/pull/";
  return urls.filter((url) => url.toLowerCase().startsWith(prefix) && url !== shown).reverse().slice(0, limit);
}

const GH_PR = /(?:^|[\s;&|(`"'])gh(?:\.exe)?\s+pr\s+(\S+)/i;
const LISTING = new Set(["list", "status"]);
const PR_URL = /https:\/\/[^\s/"'<>()]+\/[^\s/"'<>()]+\/[^\s/"'<>()]+\/pull\/\d+(?![\d/])/g;

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
