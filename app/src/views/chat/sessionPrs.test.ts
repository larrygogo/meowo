import { describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import { latestSessionPullRequest, sessionPullRequestUrls } from "./sessionPrs";

let seq = 0;
const run = (command: string, output: string, isError = false): ChatItem[] => {
  const id = `t${++seq}`;
  return [
    { type: "tool_use", id, timestamp: null, name: "Shell", kind: "run", summary: command, detail: { kind: "command", command, description: null } } as ChatItem,
    { type: "tool_result", id: `${id}-r`, timestamp: null, tool_use_id: id, text: output, is_error: isError } as ChatItem,
  ];
};
const say = (text: string): ChatItem => ({ type: "assistant_text", id: `a${++seq}`, timestamp: null, text } as unknown as ChatItem);

describe("会话里 agent 建过、动过的 PR", () => {
  it("2026-11-16 实拍：临时 worktree 里先后建了两个仓库的 PR，最新的排最后", () => {
    const items = [
      ...run("gh pr view --json number,url", '{"number":530,"url":"https://github.com/acme/xbot-admin-front/pull/530"}'),
      ...run("cd /tmp/wt-admin && gh pr create --base main --title \"preset taskName\" --body x", "https://github.com/acme/xbot-admin-front/pull/536\n"),
      ...run("cd /tmp/wt-dash; gh pr create --base main --fill", "Creating pull request for feat/x into main\n\nhttps://github.com/acme/xbot-dashboard-front/pull/213\n"),
    ];
    expect(sessionPullRequestUrls(items)).toEqual([
      "https://github.com/acme/xbot-admin-front/pull/530",
      "https://github.com/acme/xbot-admin-front/pull/536",
      "https://github.com/acme/xbot-dashboard-front/pull/213",
    ]);
    expect(latestSessionPullRequest(items)).toBe("https://github.com/acme/xbot-dashboard-front/pull/213");
  });

  it("后来又去看了旧的那个：它变成最新", () => {
    const items = [
      ...run("gh pr create --fill", "https://github.com/o/r/pull/536"),
      ...run("gh pr view https://github.com/o/r/pull/530 --comments", "title: old"),
    ];
    expect(latestSessionPullRequest(items)).toBe("https://github.com/o/r/pull/530");
  });

  it("不算：正文里提到的链接、gh pr list 列出的、不是 gh pr 的命令、失败的命令", () => {
    const items = [
      say("参考 https://github.com/o/r/pull/1"),
      ...run("gh pr list --json url", '[{"url":"https://github.com/o/r/pull/2"}]'),
      ...run("gh pr status", "https://github.com/o/r/pull/3"),
      ...run("curl https://api.github.com/x", "https://github.com/o/r/pull/4"),
      ...run("gh pr create --fill", "https://github.com/o/r/pull/5", true),
      ...run("echo 'https://github.com/o/r/pull/6'", "https://github.com/o/r/pull/6"),
    ];
    expect(sessionPullRequestUrls(items)).toEqual([]);
    expect(latestSessionPullRequest(items)).toBeNull();
  });

  it("Windows 上的 gh.exe、PR 子页面链接只取 PR 本身", () => {
    const items = run("& gh.exe pr create --fill", "https://github.example.com/o/r/pull/9\nhttps://github.com/o/r/pull/9/files");
    expect(sessionPullRequestUrls(items)).toEqual(["https://github.example.com/o/r/pull/9"]);
  });
});
