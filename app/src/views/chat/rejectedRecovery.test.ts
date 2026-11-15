import { describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import { rejectedUndoTarget } from "./rejectedRecovery";

const user = (id: string, text = id): ChatItem => ({ type: "user_text", id, timestamp: null, text } as ChatItem);
const tool = (id: string): ChatItem => ({ type: "assistant_text", id, timestamp: null, text: "…" });
const rejected = (id: string): ChatItem => ({ type: "turn_error", id, timestamp: null, label: "请求被拒", text: "Invalid request Error" });
const other = (id: string): ChatItem => ({ type: "turn_error", id, timestamp: null, label: "API 请求出错", text: "busy" });

describe("rejectedUndoTarget", () => {
  // 实拍那个会话：第一轮读了动图后 400，之后三次「继续」和一句「为什么一直失败」全 400。
  const poisoned = [
    user("u0", "如何有 UX 上对比 t3来 分析"), tool("a0"), rejected("e0"),
    user("u1", "继续"), rejected("e1"),
    user("u2", "继续"), rejected("e2"),
    user("u3", "为什么一直失败"), rejected("e3"),
  ];
  const all = new Set(["u0", "u1", "u2", "u3"]);

  it("撤到最早那轮被拒的用户消息，原话是那一轮的提问", () => {
    expect(rejectedUndoTarget(poisoned, all)).toEqual({ id: "u0", text: "如何有 UX 上对比 t3来 分析" });
  });

  it("之前的好轮次不撤", () => {
    const items = [user("ok"), tool("a"), ...poisoned];
    expect(rejectedUndoTarget(items, new Set(["ok", ...all]))?.id).toBe("u0");
  });

  it("最早那条此刻不可撤（不在当前段、会话在跑）时不给撤回", () => {
    expect(rejectedUndoTarget(poisoned, new Set(["u3"]))).toBeNull();
    expect(rejectedUndoTarget(poisoned, undefined)).toBeNull();
  });

  it("末尾不是请求被拒时不给撤回", () => {
    expect(rejectedUndoTarget([user("u"), other("e")], new Set(["u"]))).toBeNull();
    expect(rejectedUndoTarget([...poisoned, user("u4"), tool("a4")], new Set([...all, "u4"]))).toBeNull();
  });

  it("中间夹着一般错误的轮次也算好轮次的边界", () => {
    const items = [user("u0"), other("e0"), user("u1"), rejected("e1")];
    expect(rejectedUndoTarget(items, new Set(["u0", "u1"]))?.id).toBe("u1");
  });
});
