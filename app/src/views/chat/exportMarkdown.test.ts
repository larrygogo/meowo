import { describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import { finalAnswer, transcriptMarkdown } from "./exportMarkdown";

const user = (id: string, text: string): ChatItem => ({ type: "user_text", id, timestamp: null, text } as ChatItem);
const agent = (id: string, text: string): ChatItem => ({ type: "assistant_text", id, timestamp: null, text } as ChatItem);
const tool = (id: string): ChatItem => ({ type: "tool_use", id, timestamp: null, name: "Read", summary: "a.ts" } as ChatItem);

describe("对话导出成 Markdown（对照 Kimi「复制全部对话」）", () => {
  it("只留人说的话与 agent 正文，按角色分段；工具调用与后台通知不进", () => {
    const items = [
      user("u1", "帮我看看登录页 [Image #1]"),
      tool("t1"),
      agent("a1", "看过了，问题在表单校验。"),
      user("u2", "<task-notification><task-id>x</task-id><status>completed</status></task-notification>"),
      user("u3", "<command-name>/model</command-name><command-args>opus</command-args>"),
      agent("a2", "已修好。"),
    ];
    expect(transcriptMarkdown(items, { title: "修登录页", user: "你", agent: "Claude Code" })).toBe(
      "# 修登录页\n\n---\n\n**你**\n\n帮我看看登录页\n\n---\n\n**Claude Code**\n\n看过了，问题在表单校验。"
      + "\n\n---\n\n**你**\n\n/model opus\n\n---\n\n**Claude Code**\n\n已修好。\n",
    );
  });

  it("最终回答取最后一条 agent 正文，没有就是 null", () => {
    expect(finalAnswer([user("u", "hi"), agent("a1", "一"), tool("t"), agent("a2", " 二 ")])).toBe("二");
    expect(finalAnswer([user("u", "hi")])).toBeNull();
  });
});
