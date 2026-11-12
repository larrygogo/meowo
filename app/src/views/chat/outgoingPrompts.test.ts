import { describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import {
  anchorFrom,
  createOutgoingStore,
  ECHO_CONFIRM_MS,
  freshAnchor,
  INSERTED_LINGER_MS,
  promptKey,
  reconcileOutgoing,
  SENDING_STALL_MS,
  type OutgoingKind,
  type OutgoingPrompt,
  type PromptAck,
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
  // 先上屏再发送：没写进去的回执不参与对账；发送中、发送失败的都不因会话结束被收走
  // （给已结束会话发消息会先把它拉起来，那时历史还是 ended）。
  it("发送失败的回执不对账，发送中与失败的会话结束也不收走", () => {
    const before = [reply("a1")];
    const list = [prompt("echo", "在写", before, { phase: "sending" }), prompt("echo", "没写进去", before, { phase: "failed", at: -ECHO_CONFIRM_MS * 2 })];
    expect(reconcileOutgoing(list, ev([...before, user("u2", "没写进去")]))).toBe(list);
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

  /// 用户实拍（2026-11-09）：新建会话页带图发出的第一句话，agent 跑了十几步后才对账，
  /// 它已不在尾部 12 条里，空锚点认不出，「未确认送达」的气泡叠在真消息下面。
  it("新会话第一句话被挤出尾部也能认出", () => {
    const first = user("u1", "[Image #1] [Image #2]1. 新建会话为什么\n2. 列表无法滚动");
    const later = Array.from({ length: 20 }, (_, i) => reply(`a${i}`));
    const list = [prompt("echo", "1. 新建会话为什么\n2. 列表无法滚动", [], { anchor: freshAnchor() })];
    expect(reconcileOutgoing(list, ev([first, ...later]))).toEqual([]);
    // 对照：普通空锚点仍只看尾部（保持既有水位线语义）。
    const plain = [prompt("echo", "1. 新建会话为什么\n2. 列表无法滚动", [])];
    expect(reconcileOutgoing(plain, ev([first, ...later]))).toBe(plain);
  });

  it("新会话第一句话不在加载窗口里：有后续用户消息就收起，且不占用它", () => {
    const fresh = prompt("echo", "第一句", [], { anchor: freshAnchor() });
    const second = prompt("echo", "第二句", [reply("a0")]);
    const items = [reply("a0"), user("u2", "第二句")];
    expect(reconcileOutgoing([fresh, second], ev(items))).toEqual([]);
    // 还没有任何用户消息落盘时留着
    expect(reconcileOutgoing([fresh], ev([]))).toEqual([fresh]);
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
  /// hook 只算「CLI 收下了」（已收到），不算已读到，否则回执当场收走、消息却还没被读到。
  it("hook 记的最近输入：没有 transcript 时算读到，有 transcript 时只算收下", () => {
    const before = [reply("a0")];
    const withTranscript = [prompt("queued", "插一句", before)];
    expect(reconcileOutgoing(withTranscript, ev(before, 1_000, { lastUserText: "插一句" }))).toMatchObject([{ phase: "accepted" }]);
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
    const ack = (id: string, text: string, s: number): PromptAck => ({ type: "prompt_queued", id, timestamp: at(s), text });
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

/// leader 历次实拍的送达误判（调研 real-usage #1），脱敏后只留对账要用的字段。
/// 时间戳取自当次会话记录；正文换成同形的短句或保留无隐私的原文（公开仓库链接）。
describe("送达对账：历次实拍回归", () => {
  const at = (iso: string) => iso;
  const stamped = (item: ChatItem, iso: string): ChatItem => ({ ...item, timestamp: at(iso) });

  /// 11-09 07:27（code-review 会话，CC 2.1.3xx）：新建页开会话 2 秒后又发了一个链接。
  /// 落盘形态：queue-operation enqueue 07:25:25.479 → attachment queued_command 07:25:25.367
  /// （origin human、humanTurn，后端转成 interjected 的 user_text）。Meowo 写入途中临时编号
  /// 换成正式编号，「写入完成」按旧编号结算落空，气泡一直「发送中」叠在真消息下面。
  /// 修前：发送中的回执不参与对账，记录里有这句也无视。
  it("11-09 07:27 发送中的回执：CLI 记录先到就按记录收起", () => {
    const before = [stamped(user("0f0d05e0", "帮我装一下这个 code review 的skill"), "2026-11-09T07:25:25.069Z")];
    const list = [prompt("echo", "https://github.com/mattpocock/skills", before, { phase: "sending", at: 0 })];
    const landed = [...before, stamped(user("8608d6ee", "https://github.com/mattpocock/skills", true), "2026-11-09T07:25:25.367Z")];
    expect(reconcileOutgoing(list, ev(landed, 2_000))).toEqual([]);
  });

  /// 同一实例在仓库层：临时编号下登记、写入途中搬到正式编号、CLI 记录先到收起，
  /// 迟到的「写入完成」按回执编号结算时找不到它，也不会把它复活。
  it("11-09 07:27 编号搬家后迟到的写入完成不复活已收起的回执", () => {
    const store = createOutgoingStore();
    const before = [stamped(reply("a0"), "2026-11-09T07:25:25.069Z")];
    const id = store.add(-1, { kind: "echo", text: "https://github.com/mattpocock/skills", attachmentOnly: false, anchor: anchorFrom(before, null), phase: "sending" });
    store.move(-1, 42);
    store.reconcile(42, ev([...before, stamped(user("u1", "https://github.com/mattpocock/skills", true), "2026-11-09T07:25:25.367Z")], 2_000));
    expect(store.get(42)).toEqual([]);
    store.settle(id, { phase: "pending", at: 3_000 });
    expect(store.get(42)).toEqual([]);
  });

  /// 写入的 promise 再不落定（链路卡死）也不能永远转圈：超时转「未确认送达」给出口。
  it("发送中卡死超过上限转未确认，给重发与放回输入框", () => {
    const before = [reply("a0")];
    const list = [prompt("echo", "在吗", before, { phase: "sending", at: 0 })];
    expect(reconcileOutgoing(list, ev(before, SENDING_STALL_MS))).toBe(list);
    expect(reconcileOutgoing(list, ev(before, SENDING_STALL_MS + 1))).toMatchObject([{ phase: "uncertain" }]);
  });

  /// 11-01 08:19（a7bec18a，「为什么发消息现在经常提示未送达」）：发出后切回侧栏，回来时首读
  /// 只给最新一屏（200 条），Agent 期间干的活把这条话和发送时的锚点都挤出了窗口。
  /// 修前：窗口里找不到这条话，15 秒后标「未确认送达」。窗口整个晚于发送划线 = 看不见，不是没收到。
  it("11-01 首读窗口整个落在发送之后：看不见不判未确认", () => {
    const before = [stamped(user("u0", "继续"), "2026-11-01T08:00:00Z"), stamped(reply("a0"), "2026-11-01T08:00:01Z")];
    const list = [prompt("echo", "修一下这个", before, { at: 0 })];
    const page = Array.from({ length: 200 }, (_, i) => stamped(reply(`w${i}`), new Date(Date.UTC(2026, 10, 1, 8, 1, 0) + i * 1_000).toISOString()));
    expect(reconcileOutgoing(list, ev(page, ECHO_CONFIRM_MS * 2))).toEqual([]);
    // 对照：窗口还包着发送前的记录、却没有这条话，照旧转未确认（真没送达的情形）。
    expect(reconcileOutgoing(list, ev(before, ECHO_CONFIRM_MS * 2))).toMatchObject([{ phase: "uncertain" }]);
  });

  /// 11-09 08:27（新建会话「侧边栏与变更列表滚动」）：带三张图的第一句话，CLI 落盘成
  /// 「[Image #1] [Image #2] [Image #3]1. …」，agent 跑了十几步后它已不在尾部 12 条。
  it("11-09 08:27 新会话带图的第一句话：贴图前缀与挤出尾部都认得", () => {
    const first = stamped(user("u1", "[Image #1] [Image #2] [Image #3]1. 新建会话为什么\n2. 列表无法滚动"), "2026-11-09T08:25:00Z");
    const later = Array.from({ length: 20 }, (_, i) => stamped(reply(`a${i}`), `2026-11-09T08:25:${String(10 + i).padStart(2, "0")}Z`));
    const list = [prompt("echo", "1. 新建会话为什么\n2. 列表无法滚动", [], { anchor: freshAnchor() })];
    expect(reconcileOutgoing(list, ev([first, ...later]))).toEqual([]);
  });

  /// 11-11（2f8ca755）：终端停在子代理标签页，对话页发的话被 CLI 交给了子代理，主链没有这条。
  /// 后端从子代理侧车取回、标 to_agent 挂到主时间线（#164），对账照常认领，不再报未确认。
  it("11-11 发给子代理的话按 to_agent 记录认领", () => {
    const before = [stamped(reply("a0"), "2026-11-11T06:00:00Z")];
    const list = [prompt("echo", "这个先别改", before, { at: 0 })];
    const landed = [...before, { ...stamped(user("s1", "这个先别改"), "2026-11-11T06:00:05Z"), to_agent: "general-purpose" } as ChatItem];
    expect(reconcileOutgoing(list, ev(landed, ECHO_CONFIRM_MS * 2))).toEqual([]);
  });

  /// 11-02（0e34f0a6）：这段话停在 CLI 输入框里没提交（CLI 侧），记录里确实没有它。
  /// 这一类 Meowo 无法从记录里确认，照实转未确认；读终端画面给「提交」在 useOutgoingUpkeep。
  it("11-02 CLI 没收到（停在输入框）：照实转未确认，不悄悄消失", () => {
    const before = [stamped(reply("a0"), "2026-11-02T03:00:00Z")];
    const list = [prompt("echo", "继续", before, { at: 0 })];
    expect(reconcileOutgoing(list, ev(before, ECHO_CONFIRM_MS + 1))).toMatchObject([{ phase: "uncertain" }]);
  });

  /// CC 2.1.218 交互式 --resume 后 transcript 静默不写（上游 #53417，memory cc-resume-writer-bug），
  /// 但 UserPromptSubmit hook 照常触发。修前：有 transcript 时不认 hook，每条都报未确认。
  it("transcript 不落盘、hook 已收到：回显标已收到，过 15 秒也不判未确认", () => {
    const before = [stamped(reply("a0"), "2026-11-03T01:00:00Z")];
    const list = [prompt("echo", "看下日志", before, { at: 0 })];
    const received = reconcileOutgoing(list, ev(before, 1_000, { lastUserText: "看下日志\n\n请读取并结合以下本地附件…" }));
    expect(received).toMatchObject([{ phase: "received" }]);
    expect(reconcileOutgoing(received, ev(before, ECHO_CONFIRM_MS * 4, { lastUserText: "看下日志" }))).toBe(received);
    // 正文晚些落盘照常收起
    expect(reconcileOutgoing(received, ev([...before, stamped(user("u1", "看下日志"), "2026-11-03T01:00:09Z")], 20_000))).toEqual([]);
    // 迟到的「写入完成」不把已收到拨回 pending 重新计时
    const store = createOutgoingStore();
    const id = store.add(7, { kind: "echo", text: "看下日志", attachmentOnly: false, anchor: anchorFrom(before, null), phase: "sending" });
    store.reconcile(7, ev(before, 500, { lastUserText: "看下日志" }));
    store.settle(id, { phase: "pending", at: 600 });
    store.settle(id, { phase: "failed" });
    expect(store.get(7)).toMatchObject([{ phase: "received" }]);
  });

  it("hook 只记最近一句：一次变化只确认一条，发送前就有的旧值不算", () => {
    const before = [reply("a0")];
    const list = [
      prompt("echo", "继续", before, { anchor: anchorFrom(before, "继续") }),
      prompt("echo", "继续吧", before),
      prompt("echo", "继续吧", before),
    ];
    expect(reconcileOutgoing(list, ev(before, 1_000, { lastUserText: "继续" })).map((p) => p.phase)).toEqual(["pending", "pending", "pending"]);
    expect(reconcileOutgoing(list, ev(before, 1_000, { lastUserText: "继续吧" })).map((p) => p.phase)).toEqual(["pending", "received", "pending"]);
  });

  /// CLI 改写落盘正文的几种形态（本机 transcript 实拍，CC 2.1.3xx）：斜杠命令包成三段标签，
  /// ! 前缀包成 <bash-input>。修前按原文「包含」比，永远对不上，气泡叠成两遍。
  it("斜杠命令与 bash 模式按 CLI 改写规则认领", () => {
    const before = [stamped(reply("a0"), "2026-11-09T06:12:00Z")];
    const list = [prompt("echo", "/model fable", before), prompt("echo", "! ls -la", before)];
    const landed = [
      ...before,
      stamped(user("c1", "<command-name>/model</command-name>\n<command-message>model</command-message>\n<command-args>fable</command-args>"), "2026-11-09T06:12:01Z"),
      stamped(user("b1", "<bash-input> ls -la</bash-input>"), "2026-11-09T06:12:02Z"),
    ];
    expect(reconcileOutgoing(list, ev(landed))).toEqual([]);
  });
});

describe("promptKey", () => {
  it("还原 CLI 的改写并去掉空白", () => {
    expect(promptKey("<command-message>mp-code-review</command-message>\n<command-name>/mp-code-review</command-name>")).toBe("/mp-code-review");
    expect(promptKey("<bash-input> ssh-copy-id -i x</bash-input>")).toBe(promptKey("! ssh-copy-id -i x"));
    expect(promptKey("[Image #1] [Image #2]看下\r\n这里")).toBe(promptKey("看下\n这里"));
    // 用户自己在正文中间提到标签不触发还原（只认行首的 bash-input）
    expect(promptKey("为什么显示 <bash-input>x</bash-input>")).toContain("为什么显示");
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
  it("负数临时 id 的失败回执不落盘，认领搬到真 id 后才存（编号跨重启复用，存了会串号）", () => {
    localStorage.clear();
    const first = createOutgoingStore();
    const id = first.add(-1, { kind: "echo", text: "第一句", attachmentOnly: false, anchor: anchorFrom([], null), phase: "sending", draft: { prompt: "第一句", attachments: [] } });
    first.settle(id, { phase: "failed" });
    expect(first.get(-1).map((prompt) => prompt.phase)).toEqual(["failed"]);
    expect(createOutgoingStore().get(-1)).toEqual([]);
    first.move(-1, 71);
    expect(createOutgoingStore().get(71).map((prompt) => [prompt.text, prompt.phase])).toEqual([["第一句", "failed"]]);
    expect(createOutgoingStore().get(-1)).toEqual([]);
  });
});
