import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import type { ChatItem } from "../../api";
import { reduceChatEvents } from "../../chat/reducer";
import { ChatCwdContext } from "./ToolActivity";
import { Transcript } from "./Transcript";
import { collectBackgroundTasks } from "./TasksPanel";
import { resetFoldMemory } from "./foldMemory";
import { indexToolResults, liveTurnStart, toolRunState, turnActivityOf, type TurnActivity } from "./toolState";

/// 运行态的显式终态（走查 A1、A3、A4、A10、C6）。
///
/// 数据全部取自五家的真实会话样本（后端回放快照）。「调用发出了、回执还没到」这个瞬间，
/// 用把真实时间线截到那条调用为止来得到——那正是轮询读到一半时界面手里的数据。

const FIXTURES = join(__dirname, "../../../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay");
const AGENTS = ["claude", "kimi", "codex", "gemini", "opencode"] as const;

function raw(agent: string): ChatItem[] {
  return JSON.parse(readFileSync(join(FIXTURES, `${agent}.items.json`), "utf8")) as ChatItem[];
}

/** 真实时间线截到第一条满足条件的条目为止（含它）。 */
function upTo(agent: string, pick: (item: ChatItem) => boolean): ChatItem[] {
  const items = raw(agent);
  const at = items.findIndex(pick);
  expect(at, `${agent} 样本里没有要找的条目`).toBeGreaterThanOrEqual(0);
  return reduceChatEvents([], items.slice(0, at + 1), true);
}

function show(items: ChatItem[], activity: TurnActivity, alive = activity !== "idle") {
  return render(
    <ChatCwdContext.Provider value={null}>
      <Transcript sessionId={7} items={items} activity={activity} alive={alive} />
    </ChatCwdContext.Provider>,
  );
}

/** 已结束的过程块默认收起：像用户那样点开，才看得到里面的工具行与子任务块。 */
function openFolds(root: HTMLElement) {
  for (let guard = 0; guard < 20; guard += 1) {
    const closed = root.querySelector<HTMLElement>(".turn-fold:not(.is-open) > .turn-fold-head");
    if (!closed) return;
    fireEvent.click(closed);
  }
}

const lastTool = (root: HTMLElement) => [...root.querySelectorAll<HTMLElement>("details.chat-tool")].pop()!;

afterEach(() => {
  cleanup();
  resetFoldMemory();
  vi.useRealTimers();
});

describe("没有回执的工具调用", () => {
  it.each(AGENTS)("%s：回合已不在进行时显示已中断，不计时；回合在跑时才是运行中", (agent) => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-20T00:00:00Z"));
    const items = upTo(agent, (item) => item.type === "tool_use" && !item.subagent);
    const view = show(items, "idle");
    openFolds(view.container);
    const row = () => lastTool(view.container);
    expect(row().getAttribute("data-state")).toBe("interrupted");
    expect(row().querySelector(".chat-tool-out")?.textContent).toBe("已中断");
    // 不标红、不亮状态点。
    expect(row().classList.contains("is-error")).toBe(false);
    expect(row().querySelector(".chat-tool-state")).toBeNull();
    // 过了一分钟还是这句话：没有在走的表。此前这里是「运行中… (几天)」。
    act(() => { vi.advanceTimersByTime(60_000); });
    expect(row().querySelector(".chat-tool-out")?.textContent).toBe("已中断");
    expect(view.container.textContent).not.toContain("运行中");

    view.rerender(
      <ChatCwdContext.Provider value={null}>
        <Transcript sessionId={7} items={items} activity="running" alive />
      </ChatCwdContext.Provider>,
    );
    expect(row().getAttribute("data-state")).toBe("running");
    expect(row().querySelector(".chat-tool-out.is-running")?.textContent).toMatch(/^运行中… \(/);
    expect(row().querySelector(".chat-tool-state")).not.toBeNull();
  });

  it("上一回合里没回执的调用，即使会话此刻在跑也是已中断", () => {
    // codex 样本：命令被用户打断后（中断分隔），会话又开始了新的一轮。
    const items = raw("codex");
    const cut = items.findIndex((item) => item.type === "tool_use" && item.kind === "run");
    const earlier = reduceChatEvents([], [
      ...items.slice(0, cut + 1),
      { type: "user_text", id: "next-turn", timestamp: null, text: "继续" } as ChatItem,
    ], true);
    const view = show(earlier, "running");
    openFolds(view.container);
    expect(lastTool(view.container).getAttribute("data-state")).toBe("interrupted");
    expect(liveTurnStart(earlier)).toBe(earlier.length - 1);
  });
});

describe("等待审批时的过程块", () => {
  it("不收起，未批准的调用不计入「已运行」，那一行写待处理；回合停掉后才收起并单列已中断", () => {
    // claude 样本：读、搜索都跑完了，写文件的调用刚发出（正卡在审批上）。
    const items = upTo("claude", (item) => item.type === "tool_use" && item.kind === "write");
    const view = show(items, "waiting");
    const fold = () => view.container.querySelector<HTMLElement>(".turn-fold")!;
    expect(fold().classList.contains("is-open")).toBe(true);
    expect(fold().querySelector(".turn-fold-lead")?.textContent).toBe("待处理");
    const summary = () => fold().querySelector(".turn-fold-summary")?.textContent ?? "";
    expect(summary()).toContain("读取了 1 个文件");
    expect(summary()).toContain("搜索了 1 个模式");
    expect(summary()).not.toContain("已写入");
    const row = lastTool(view.container);
    expect(row.getAttribute("data-state")).toBe("waiting");
    expect(row.querySelector(".chat-tool-verb")?.textContent).toBe("写入");
    expect(row.querySelector(".chat-tool-body")?.textContent).toContain("待处理");
    // 等人不是在跑：不亮状态点，也没有在走的表。
    expect(row.querySelector(".chat-tool-state")).toBeNull();
    expect(view.container.textContent).not.toContain("运行中…");
    expect(view.container.textContent).not.toContain("正在执行");

    // 用户拒绝、回合停了：这次调用没跑，过程块收起，摘要照实说 1 次已中断。
    view.rerender(
      <ChatCwdContext.Provider value={null}>
        <Transcript sessionId={7} items={items} activity="idle" alive />
      </ChatCwdContext.Provider>,
    );
    expect(fold().classList.contains("is-open")).toBe(false);
    expect(summary()).toContain("1 次已中断");
    expect(summary()).not.toContain("已写入");
  });

  it("会话状态到时间线活动态的换算：待处理算回合仍在进行", () => {
    expect(turnActivityOf("running")).toBe("running");
    expect(turnActivityOf("pending")).toBe("waiting");
    for (const tone of ["waiting", "offline", "ended", "error"] as const) expect(turnActivityOf(tone)).toBe("idle");
  });
});

describe("没有回执的子任务", () => {
  it.each(["claude", "kimi"] as const)("%s：回合已不在进行时显示已中断，不计时；在跑时是进行中", (agent) => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-20T00:00:00Z"));
    const items = upTo(agent, (item) => item.type === "tool_use" && !!item.subagent);
    const view = show(items, "idle");
    openFolds(view.container);
    const badge = () => view.container.querySelector(".chat-subagent-status")?.textContent;
    expect(badge()).toBe("已中断");
    // 已中断不显示耗时（此前按「进行中」每秒滴答）。
    expect(view.container.querySelector(".chat-subagent-time")).toBeNull();
    act(() => { vi.advanceTimersByTime(60_000); });
    expect(view.container.querySelector(".chat-subagent-time")).toBeNull();

    view.rerender(
      <ChatCwdContext.Provider value={null}>
        <Transcript sessionId={7} items={items} activity="running" alive />
      </ChatCwdContext.Provider>,
    );
    expect(badge()).toBe("进行中");

    // 任务面板同一口径：回合不在进行 = 已中断，不计入运行中。
    expect(collectBackgroundTasks(items, { activity: "idle", alive: false }).subagents.map((task) => task.state)).toEqual(["stop"]);
    expect(collectBackgroundTasks(items, { activity: "running", alive: true }).subagents.map((task) => task.state)).toEqual(["run"]);
  });
});

describe("调用与回执的配对", () => {
  it("回执隔着别的块才到也能配上：codex 一段脚本两个调用，两条回执排在两个调用之后", () => {
    const items = reduceChatEvents([], raw("codex"), true);
    const results = indexToolResults(items);
    const calls = items.filter((item): item is Extract<ChatItem, { type: "tool_use" }> => item.type === "tool_use");
    // 样本里只有被打断之外的调用都有回执。
    const unresolved = calls.filter((call) => !results.has(call.id));
    expect(unresolved).toEqual([]);
    const multi = calls.filter((call) => /#1$/.test(call.id));
    expect(multi.length).toBeGreaterThan(0);
    for (const call of multi) expect(toolRunState(results.get(call.id), "idle")).toBe("done");
  });

  it.each(AGENTS)("%s：时间线从一条回执开始时，不画孤立的「工具结果」行", (agent) => {
    // 首屏只读尾部一屏，窗口可能正好从回执切开：调用在窗口之外。
    const items = raw(agent);
    const at = items.findIndex((item) => item.type === "tool_result");
    const view = show(reduceChatEvents([], items.slice(at), true), "idle");
    expect(view.container.querySelector(".chat-result")).toBeNull();
    expect(view.container.textContent).not.toContain("工具结果");
    cleanup();
    // 只有一条回执时什么都不画，也不留空行。
    const lone = show([items[at]], "idle");
    expect(lone.container.querySelector("details")).toBeNull();
    expect(lone.container.textContent).toBe("");
  });
});

describe("思考中", () => {
  it("逐块流进来的思考排在末尾且会话在跑时写「思考中…」，想完或回合停了就是「思考过程」", () => {
    // kimi 是唯一逐块落盘思考的一家：截到思考那条为止。
    const all = raw("kimi");
    const at = all.findIndex((item) => item.type === "reasoning_delta");
    expect(at).toBeGreaterThanOrEqual(0);
    const thinking = reduceChatEvents([], all.slice(0, at + 1), true);
    const view = show(thinking, "running");
    const label = () => view.container.querySelector(".chat-reasoning > summary")?.textContent ?? "";
    expect(label()).toContain("思考中…");

    // 后面有了正文：想完了。
    view.rerender(
      <ChatCwdContext.Provider value={null}>
        <Transcript sessionId={7} items={reduceChatEvents(thinking, all.slice(at + 1, at + 2), false)} activity="running" alive />
      </ChatCwdContext.Provider>,
    );
    expect(label()).toContain("思考过程");
    expect(label()).not.toContain("思考中");

    cleanup();
    // 回合停了（历史里的思考）：不是思考中。
    const stopped = show(thinking, "idle");
    expect(stopped.container.querySelector(".chat-reasoning > summary")?.textContent).toContain("思考过程");
  });

  it("整条落盘的思考（claude、gemini、opencode）写完才出现，不显示思考中", () => {
    const items = upTo("gemini", (item) => item.type === "reasoning");
    const view = show(items, "running");
    expect(view.container.querySelector(".chat-reasoning > summary")?.textContent).toContain("思考过程");
  });
});
