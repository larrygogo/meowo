import { describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import { chatUi } from "../../test/agents";
import { typedUndoTarget } from "./undoTerminal";

const items = [
  { type: "user_text", id: "u1", timestamp: null, text: "先做甲" },
  { type: "assistant_text", id: "a1", timestamp: null, text: "甲做完了" },
  { type: "user_text", id: "u2", timestamp: null, text: "再做乙" },
  { type: "user_text", id: "u3", timestamp: null, text: "顺便看看", interjected: true },
  { type: "assistant_text", id: "a2", timestamp: null, text: "乙做完了" },
] as unknown as ChatItem[];

// 走查输入 A4：手敲的撤回命令要和消息上的「撤回」按钮走同一条流程。这里只认「敲的是不是撤回命令、撤哪一条」。
describe("手敲的撤回命令", () => {
  it("opencode 的 /undo 指最近一轮（插话不算一轮）", () => {
    expect(typedUndoTarget("/undo", chatUi("opencode")!.undo, items)).toEqual({ id: "u2", text: "再做乙" });
  });
  it("opencode 不收参数：带了参数就不是撤回命令，原样发给 CLI", () => {
    expect(typedUndoTarget("/undo 2", chatUi("opencode")!.undo, items)).toBeNull();
  });
  it("kimi 的 /undo N 指倒数第 N 轮，不带 N 是最近一轮", () => {
    expect(typedUndoTarget("/undo 2", chatUi("kimi")!.undo, items)).toEqual({ id: "u1", text: "先做甲" });
    expect(typedUndoTarget("/undo", chatUi("kimi")!.undo, items)).toEqual({ id: "u2", text: "再做乙" });
    expect(typedUndoTarget("/undo 9", chatUi("kimi")!.undo, items)).toBeNull();
    expect(typedUndoTarget("/undo abc", chatUi("kimi")!.undo, items)).toBeNull();
  });
  it("别的命令、没有撤回能力、菜单式撤回（claude /rewind 走菜单识别）都不拦", () => {
    expect(typedUndoTarget("/undoing", chatUi("opencode")!.undo, items)).toBeNull();
    expect(typedUndoTarget("/undo", chatUi("codex")!.undo, items)).toBeNull();
    expect(typedUndoTarget("/rewind", chatUi("claude")!.undo, items)).toBeNull();
    expect(typedUndoTarget("/undo", chatUi("opencode")!.undo, [])).toBeNull();
  });
});
