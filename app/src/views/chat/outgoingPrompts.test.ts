import { describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import {
  anchorFrom,
  createOutgoingStore,
  ECHO_CONFIRM_MS,
  INSERTED_LINGER_MS,
  reconcileOutgoing,
  type OutgoingKind,
  type OutgoingPrompt,
} from "./outgoingPrompts";

const user = (id: string, text: string, interjected?: boolean): ChatItem =>
  ({ type: "user_text", id, timestamp: null, text, ...(interjected ? { interjected } : {}) });
const reply = (id: string): ChatItem => ({ type: "assistant_text", id, timestamp: null, text: "好" });

let seq = 0;
function prompt(kind: OutgoingKind, text: string, items: ChatItem[], extra: Partial<OutgoingPrompt> = {}): OutgoingPrompt {
  seq += 1;
  return {
    id: seq, kind, text, attachmentOnly: false, at: 0, phase: "pending", settledAt: null,
    anchor: anchorFrom(items, null), ...extra,
  };
}
const ev = (items: ChatItem[], now = 1_000, extra: { lastUserText?: string | null; ended?: boolean } = {}) =>
  ({ items, lastUserText: extra.lastUserText ?? null, ended: extra.ended ?? false, now });

describe("reconcileOutgoing", () => {
  // 先上屏再发送：还在写 / 没写进去的回执不参与对账，也不因会话结束被收走
  // （给已结束会话发消息会先把它拉起来，那时历史还是 ended）。
  it("发送中与发送失败的回执不对账、会话结束也不收走", () => {
    const before = [reply("a1")];
    const list = [prompt("echo", "在写", before, { phase: "sending" }), prompt("echo", "没写进去", before, { phase: "failed", at: -ECHO_CONFIRM_MS * 2 })];
    expect(reconcileOutgoing(list, ev([...before, user("u1", "在写"), user("u2", "没写进去")]))).toBe(list);
    expect(reconcileOutgoing(list, ev(before, 1_000, { ended: true }))).toBe(list);
    const settled = [prompt("echo", "已写入", before)];
    expect(reconcileOutgoing([...list, ...settled], ev(before, 1_000, { ended: true }))).toEqual(list);
  });

  it("回显在新落盘的同文消息出现时收走", () => {
    const before = [reply("a1")];
    const list = [prompt("echo", "修一下测试", before)];
    expect(reconcileOutgoing(list, ev(before))).toBe(list);
    expect(reconcileOutgoing(list, ev([...before, user("u1", "修一下测试")]))).toEqual([]);
  });

  it("落盘文本带图片占位前缀与附件指令时仍能认出（归一化后包含）", () => {
    const list = [prompt("echo", "看下这个  图", [])];
    expect(reconcileOutgoing(list, ev([user("u1", "[Image #1] 看下这个 图\n请读取并结合以下本地附件…")]))).toEqual([]);
  });

  /// 水位线：短插话「ok」不能子串命中入队之前的旧消息。
  it("入队前已有的旧消息不算证据", () => {
    const before = [user("u0", "ok, run the tests"), reply("a0")];
    const list = [prompt("queued", "ok", before)];
    expect(reconcileOutgoing(list, ev(before))).toBe(list);
  });

  it("插话被读到后先转「已插入」，停留期过了才收起", () => {
    const before = [reply("a0")];
    const list = [prompt("queued", "顺便加个测试", before)];
    const landed = [...before, user("u1", "顺便加个测试", true)];
    const inserted = reconcileOutgoing(list, ev(landed, 1_000));
    expect(inserted).toMatchObject([{ phase: "inserted", settledAt: 1_000 }]);
    expect(reconcileOutgoing(inserted, ev(landed, 1_000 + INSERTED_LINGER_MS))).toBe(inserted);
    expect(reconcileOutgoing(inserted, ev(landed, 1_001 + INSERTED_LINGER_MS))).toEqual([]);
  });

  /// 连发两句同样的话，第一句落盘只能结算一张回执。
  it("一条落盘消息只结算一条回执", () => {
    const before = [reply("a0")];
    const list = [prompt("queued", "ok", before), prompt("queued", "ok", before)];
    const next = reconcileOutgoing(list, ev([...before, user("u1", "ok")]));
    expect(next.map((p) => p.phase)).toEqual(["inserted", "pending"]);
  });

  it("纯附件消息（占位文本永不落盘）认水位线后任意新用户消息", () => {
    const list = [prompt("queued", "（附件）", [], { attachmentOnly: true })];
    expect(reconcileOutgoing(list, ev([user("u1", "[Image: source: C:\\cache\\shot.png]")]))).toMatchObject([{ phase: "inserted" }]);
  });

  /// claude 在插话入队瞬间就改 lastUserText，正文要到工具边界才落盘：有 transcript 时
  /// 不能认 hook，否则回执当场收走、消息却还没被读到。
  it("hook 记的最近输入只在没有 transcript 时算证据", () => {
    const before = [reply("a0")];
    const withTranscript = [prompt("queued", "插一句", before)];
    expect(reconcileOutgoing(withTranscript, ev(before, 1_000, { lastUserText: "插一句" }))).toBe(withTranscript);
    const noTranscript = [prompt("queued", "插一句", [])];
    expect(reconcileOutgoing(noTranscript, ev([], 1_000, { lastUserText: "插一句" }))).toMatchObject([{ phase: "inserted" }]);
  });

  it("回显超时仍无证据转「未确认」并保留，后续有新消息落盘才收起", () => {
    const before = [reply("a0")];
    const list = [prompt("echo", "在吗", before, { at: 0 })];
    expect(reconcileOutgoing(list, ev(before, ECHO_CONFIRM_MS))).toBe(list);
    const uncertain = reconcileOutgoing(list, ev(before, ECHO_CONFIRM_MS + 1));
    expect(uncertain).toMatchObject([{ phase: "uncertain" }]);
    expect(reconcileOutgoing(uncertain, ev(before, 60_000))).toBe(uncertain);
    expect(reconcileOutgoing(uncertain, ev([...before, user("u9", "别的话")], 60_000))).toEqual([]);
  });

  it("排队回执没有超时：长任务里一直等到真被读到", () => {
    const before = [reply("a0")];
    const list = [prompt("queued", "等会再说", before, { at: 0 })];
    expect(reconcileOutgoing(list, ev(before, 10 * 60_000))).toBe(list);
  });

  /// 锚点被翻页挤出窗口时退回「尾部 12 条里不在快照中的」，不把整窗旧消息都当新证据。
  it("锚点不在窗口里时退回尾部快照水位线", () => {
    const before = [user("u0", "ok 先这样"), reply("a0")];
    const list = [prompt("queued", "ok", before)];
    const shifted = [user("u0", "ok 先这样"), reply("a5")];
    expect(reconcileOutgoing(list, ev(shifted))).toBe(list);
    expect(reconcileOutgoing(list, ev([...shifted, user("u6", "ok")]))).toMatchObject([{ phase: "inserted" }]);
  });

  it("会话结束整单清空", () => {
    const list = [prompt("queued", "x", []), prompt("echo", "y", [])];
    expect(reconcileOutgoing(list, ev([], 0, { ended: true }))).toEqual([]);
  });

  // 用户实拍：发出后切到侧栏，回来时首读只给最新一屏。Agent 期间产出几十条，发送时的锚点
  // 不在窗口里、这条话也不在尾部 12 条里——按落盘时间划线仍要认出来。
  it("锚点被挤出窗口时按落盘时间认领", () => {
    const at = (s: number) => new Date(Date.UTC(2026, 10, 1, 8, 0, s)).toISOString();
    const stamped = (item: ChatItem, s: number): ChatItem => ({ ...item, timestamp: at(s) });
    const before = [stamped(user("u0", "继续"), 0), stamped(reply("a0"), 1)];
    const list = [prompt("echo", "继续", before, { at: 0 })];
    const work = Array.from({ length: 20 }, (_, i) => stamped(reply(`w${i}`), 10 + i));
    const page = [stamped(user("u1", "继续"), 5), ...work];
    expect(reconcileOutgoing(list, ev(page, 1_000))).toEqual([]);
    // 同文的旧消息（早于划线）不算：只剩旧消息时不能被它收走。
    const stale = [stamped(user("u-old", "继续"), -30), ...work];
    expect(reconcileOutgoing(list, ev(stale, 1_000))).toBe(list);
  });

  // 对照 Kimi 的 prompt:accepted：CLI 正忙时 claude 把用户的话记一行入队，这就是送达回执。
  it("送达回执把消息推进到已排队，一条回执只认一条，发送前的旧回执不算", () => {
    const at = (s: number) => new Date(Date.UTC(2026, 10, 1, 8, 0, s)).toISOString();
    const ack = (id: string, text: string, s: number): ChatItem => ({ type: "prompt_queued", id, timestamp: at(s), text });
    const before = [{ ...reply("a0"), timestamp: at(10) }];
    const list = [prompt("queued", "继续", before), prompt("echo", "继续", before)];
    const stale = ack("q-old", "继续", 5);
    const first = reconcileOutgoing(list, { ...ev(before), acks: [stale, ack("q1", "继续", 12)] });
    expect(first.map((p) => p.phase)).toEqual(["accepted", "pending"]);
    const both = reconcileOutgoing(first, { ...ev(before), acks: [stale, ack("q1", "继续", 12), ack("q2", "继续", 13)] });
    expect(both.map((p) => [p.phase, p.ackId])).toEqual([["accepted", "q1"], ["accepted", "q2"]]);
    // 已排队的回显过了 15s 也不判未确认；正文落盘后照常收走。
    expect(reconcileOutgoing(both, { ...ev(before, ECHO_CONFIRM_MS * 4), acks: [] })).toBe(both);
    const landed = reconcileOutgoing(both, ev([...before, { ...user("u1", "继续", true), timestamp: at(30) }, { ...user("u2", "继续"), timestamp: at(40) }], 50_000));
    expect(landed.map((p) => [p.kind, p.phase])).toEqual([["queued", "inserted"]]);
  });
});

describe("createOutgoingStore", () => {
  it("按会话分仓：切走再切回回执还在，互不串", () => {
    const store = createOutgoingStore();
    store.add(1, { kind: "queued", text: "a", attachmentOnly: false, anchor: anchorFrom([], null) });
    store.add(2, { kind: "queued", text: "b", attachmentOnly: false, anchor: anchorFrom([], null) });
    expect(store.get(1).map((p) => p.text)).toEqual(["a"]);
    expect(store.get(2).map((p) => p.text)).toEqual(["b"]);
  });

  it("移除只动指定的那条，并通知订阅者；无变化时视图引用不变", () => {
    const store = createOutgoingStore();
    let calls = 0;
    store.subscribe(() => { calls += 1; });
    const first = store.add(1, { kind: "queued", text: "第一句", attachmentOnly: false, anchor: anchorFrom([], null) });
    store.add(1, { kind: "queued", text: "第二句", attachmentOnly: false, anchor: anchorFrom([], null) });
    store.dismiss(1, first);
    expect(store.get(1).map((p) => p.text)).toEqual(["第二句"]);
    const view = store.get(1);
    store.reconcile(1, ev([reply("a0")]));
    expect(store.get(1)).toBe(view);
    expect(calls).toBe(3);
  });
});

/** 发送失败的回执落 localStorage：刷新/页面被回收后换一个仓库实例，失败的那条和原文还在。 */
describe("createOutgoingStore 失败回执持久化", () => {
  it("失败的回执跨仓库实例保留，收掉后不再恢复；发送中的不落盘", () => {
    localStorage.clear();
    const first = createOutgoingStore();
    const failedId = first.add(7, { kind: "echo", text: "没发出去", attachmentOnly: false, anchor: anchorFrom([], null), phase: "sending", draft: { prompt: "没发出去", attachments: [] } });
    first.add(7, { kind: "echo", text: "还在发", attachmentOnly: false, anchor: anchorFrom([], null), phase: "sending" });
    first.update(7, failedId, { phase: "failed" });

    const second = createOutgoingStore();
    const restored = second.get(7);
    expect(restored.map((prompt) => [prompt.text, prompt.phase])).toEqual([["没发出去", "failed"]]);
    expect(restored[0].draft).toEqual({ prompt: "没发出去", attachments: [] });
    // 取快照是稳定引用（useSyncExternalStore 的要求）。
    expect(second.get(7)).toBe(restored);

    second.dismiss(7, restored[0].id);
    expect(createOutgoingStore().get(7)).toEqual([]);
  });
});
