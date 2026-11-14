import { beforeEach, describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import { anchorFrom, createOutgoingStore, ECHO_CONFIRM_MS, reconcileOutgoing, type OutgoingPrompt } from "./outgoingPrompts";

const user = (id: string, text: string): ChatItem => ({ type: "user_text", id, text, timestamp: null, interjected: false });
const pending = (kind: "echo" | "queued" = "echo"): OutgoingPrompt => ({
  id: 1, kind, text: "保住这条消息", attachmentOnly: false, at: 0, phase: "pending", settledAt: null,
  anchor: anchorFrom([], null), draft: { prompt: "保住这条消息", attachments: [] },
});
const evidence = (ended = false, now = 0, items: ChatItem[] = []) => ({ items, lastUserText: null, ended, now });

beforeEach(() => localStorage.clear());

describe("跨 Provider 未确认消息保护", () => {
  it.each(["echo", "queued"] as const)("%s：退出不是消费证据，保留原文为未确认", (kind) => {
    const original = pending(kind);
    expect(reconcileOutgoing([original], evidence(true))).toMatchObject([
      { phase: "uncertain", draft: original.draft, text: original.text },
    ]);
  });

  it("退出时仍先核对真实落盘，已消费的消息不再留下未确认副本", () => {
    expect(reconcileOutgoing([pending()], evidence(true, 0, [user("u1", "保住这条消息")]))).toEqual([]);
  });

  it("排队没有接收证据也要在确认期限后给出未确认反馈，保留原文且不自动重发", () => {
    const result = reconcileOutgoing([pending("queued")], evidence(false, ECHO_CONFIRM_MS + 1));
    expect(result).toMatchObject([{ phase: "uncertain", text: "保住这条消息" }]);
    expect(reconcileOutgoing(result, evidence(false, 60_000, [user("u2", "另一个问题")]))).toMatchObject([
      { phase: "uncertain", text: "保住这条消息" },
    ]);
  });

  it.each(["sending", "pending", "uncertain"] as const)("页面回收：%s恢复为未确认并保留附件草稿及对账锚点", (phase) => {
    const old = user("old", "保住这条消息");
    const anchor = anchorFrom([old], "保住这条消息");
    const first = createOutgoingStore();
    first.add(42, { kind: "echo", text: "保住这条消息", attachmentOnly: false, phase, at: 10,
      anchor, draft: { prompt: "保住这条消息", attachments: [] } });
    const second = createOutgoingStore();
    const restored = second.get(42);
    expect(restored).toMatchObject([{ phase: "uncertain", draft: { prompt: "保住这条消息" } }]);
    expect(restored[0].anchor.priorItemIds.has("old")).toBe(true);
    second.reconcile(42, evidence(false, 100_000, [old]));
    expect(second.get(42)).toHaveLength(1);
    second.dismiss(42, restored[0].id);
    expect(createOutgoingStore().get(42)).toEqual([]);
  });

  it("只有接收回执后页面被回收，仍保留已排队与原文，不冒充已经消费", () => {
    const first = createOutgoingStore();
    first.add(43, { kind: "queued", text: "a", phase: "accepted", attachmentOnly: false, anchor: anchorFrom([], null), draft: { prompt: "a", attachments: [] } });
    expect(createOutgoingStore().get(43)).toMatchObject([{ phase: "accepted", text: "a" }]);
  });

  it("临时会话认领时先合并目标会话的持久消息", () => {
    createOutgoingStore().add(44, { kind: "echo", text: "旧消息", phase: "failed", attachmentOnly: false, anchor: anchorFrom([], null) });
    const current = createOutgoingStore();
    current.add(-1, { kind: "echo", text: "新消息", phase: "pending", attachmentOnly: false, anchor: anchorFrom([], null) });
    current.move(-1, 44);
    expect(current.get(44).map((p) => p.text)).toEqual(["旧消息", "新消息"]);
  });
});
