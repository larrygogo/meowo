import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";
import type { ChatItem } from "../../api";
import { reduceChatEvents } from "../../chat/reducer";
import { resetFoldMemory } from "./foldMemory";
import type { TurnActivity } from "./toolState";
import { replaySample } from "../../test/replaySamples";

/// 活动态翻转时只重算受它影响的块（做空指出的性能回退）。
///
/// 活动态（在跑 / 等人 / 已不在进行）在回合起止、审批挂起与放行时都会翻。此前它一变，整条时间线的块全部
/// 重建：一份 2432 条的真实会话每翻一次重渲染 296 行、约 50ms，main 是 1 行、不到 1ms。只有当前回合起点
/// 之后的块才与活动态有关，更早的块原样复用。量法与前后数字见任务目录 progress-fix-display.md。

/// 记下每次真的执行了渲染函数的过程块与改动卡（按它们的第一条条目 id）。工具行、消息本身是 memo 组件，
/// 入参不变就不重渲染；被白白重渲染的是包着它们的过程块（children 每次是新节点）和改动卡（items 每次是
/// 新切片）。两者都是 memo：保留原来的比较，只把内层渲染函数换成带记录的。
const renders = vi.hoisted(() => ({ ids: [] as string[] }));
type Memoized<P> = { type: (props: P) => JSX.Element | null; compare?: null | ((a: P, b: P) => boolean) };
vi.mock("./TurnFold", async (original) => {
  const mod = await original<typeof import("./TurnFold")>();
  const { memo } = await import("react");
  const inner = mod.TurnFold as unknown as Memoized<{ id: string }>;
  const TurnFold = memo((props: { id: string }) => { renders.ids.push(props.id); return inner.type(props); }, inner.compare ?? undefined);
  return { ...mod, TurnFold };
});
vi.mock("./TurnChanges", async (original) => {
  const mod = await original<typeof import("./TurnChanges")>();
  const { memo } = await import("react");
  const inner = mod.TurnChanges as unknown as Memoized<{ items: ChatItem[] }>;
  const TurnChanges = memo((props: { items: ChatItem[] }) => { renders.ids.push(props.items[0].id); return inner.type(props); }, inner.compare ?? undefined);
  return { ...mod, TurnChanges };
});
import { ChatCwdContext } from "./ToolActivity";
import { Transcript } from "./Transcript";

type ToolUse = Extract<ChatItem, { type: "tool_use" }>;
const ui = (items: ChatItem[], activity: TurnActivity, foldTurns = false) => (
  <ChatCwdContext.Provider value={null}>
    <Transcript sessionId={52} items={items} activity={activity} alive={activity !== "idle"} foldTurns={foldTurns} />
  </ChatCwdContext.Provider>
);
const state = (root: HTMLElement, summary: string) => [...root.querySelectorAll<HTMLElement>("details.chat-tool")]
  .find((row) => row.querySelector(".chat-tool-summary")?.textContent === summary)?.getAttribute("data-state");
const openFolds = (root: HTMLElement) => {
  for (let guard = 0; guard < 40; guard += 1) {
    const closed = root.querySelector<HTMLElement>(".turn-fold:not(.is-open) > .turn-fold-head");
    if (!closed) return;
    fireEvent.click(closed);
  }
};

afterEach(() => {
  cleanup();
  resetFoldMemory();
  renders.ids = [];
});

describe("活动态翻转", () => {
  // codex 的一场真实会话（01a1cea4）：多个回合。截到最后一轮的一条命令刚发出（回执还没到）。
  const all = reduceChatEvents([], replaySample("codex.01a1cea4"), true);
  const pendingAt = all.findIndex((item) => item.type === "tool_use" && item.summary.startsWith("rustfmt --check"));
  const items = all.slice(0, pendingAt + 1);
  const pending = items[pendingAt] as ToolUse;
  const lastTurn = items.map((item) => item.type === "user_text" || (item.type === "meta" && item.kind === "interrupted")).lastIndexOf(true);
  const inLastTurn = new Set(items.slice(lastTurn + 1).map((item) => item.id));
  const earlier = items.slice(0, lastTurn).filter((item): item is ToolUse => item.type === "tool_use" && !item.subagent);

  it.each([[false], [true]])("只重渲染当前回合里的过程块与改动卡，之前回合的不动（消息自动折叠 %s）", (foldTurns) => {
    expect(earlier.length).toBeGreaterThan(8);
    const view = render(ui(items, "running", foldTurns));
    // 首次渲染时之前的回合里确实有过程块与改动卡。
    expect(renders.ids.filter((id) => !inLastTurn.has(id)).length).toBeGreaterThan(4);
    for (const next of ["waiting", "running", "idle", "running"] as const) {
      renders.ids = [];
      view.rerender(ui(items, next, foldTurns));
      expect(renders.ids.filter((id) => !inLastTurn.has(id)), `→ ${next}`).toEqual([]);
    }
  });

  it("翻转后的状态是对的：在跑、待批准、回合停了是已中断，再跑起来又是运行中", () => {
    const view = render(ui(items, "running"));
    openFolds(view.container);
    expect(state(view.container, pending.summary)).toBe("running");
    view.rerender(ui(items, "waiting"));
    expect(state(view.container, pending.summary)).toBe("waiting");
    view.rerender(ui(items, "idle"));
    openFolds(view.container);
    expect(state(view.container, pending.summary)).toBe("interrupted");
    view.rerender(ui(items, "running"));
    expect(state(view.container, pending.summary)).toBe("running");
    // 之前回合里被 Esc 打断的那条命令，怎么翻都是已中断。
    const aborted = earlier.find((call) => call.summary.includes("CARGO_TARGET_DIR") && all.some((item) => item.type === "tool_result" && item.tool_use_id === call.id && item.interrupted))!;
    expect(state(view.container, aborted.summary)).toBe("interrupted");
  });

  it("活动态没变、回合起点挪了：上一回合里没回执的调用变成已中断", () => {
    // 那条命令之后又说了两段话（它所在的块不再是末尾那块，会被缓存复用），然后用户开了新回合。
    const text = (id: string): ChatItem => ({ type: "assistant_text", id, timestamp: null, text: "先说明一下进度。" });
    const before = [...items, text("say-1"), text("say-2")];
    const view = render(ui(before, "running"));
    openFolds(view.container);
    expect(state(view.container, pending.summary)).toBe("running");
    const next: ChatItem = { type: "user_text", id: "next-turn", timestamp: null, text: "换个方向", turn_id: "turn-next" };
    view.rerender(ui([...before, next], "running"));
    openFolds(view.container);
    expect(state(view.container, pending.summary)).toBe("interrupted");
  });
});
