import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";
import type { ChatItem } from "../../api";
import { collectSubagentReceipts, reduceChatEvents } from "../../chat/reducer";
import { ChatCwdContext } from "./ToolActivity";
import { Transcript } from "./Transcript";
import { collectBackgroundTasks } from "./TasksPanel";
import { resetFoldMemory } from "./foldMemory";
import { liveTurnStart, type TurnActivity } from "./toolState";
import { findInSamples, REPLAY_AGENTS, replaySample, replaySamples, type ReplayAgent } from "../../test/replaySamples";

/// 回合边界与「没跑完的调用」只认记录里的显式信号（做空 ③、验收 14）。
///
/// 做空复现的回退：时间线是「用户消息 → 没有回执的命令 → 又一条用户消息」、会话在跑，那条命令被
/// 显示成「已中断」。原因是把后一条用户消息当成了回合边界，而它可能只是回合中途送进去的一句话。
///
/// 各家的记录怎么写（2026-11-14 用本机真实会话核对，数字见 progress-fix-display.md）：
/// - claude：回合中途的话是 queued_command，后端标 interjected；不带标记的用户消息只写在回合开头。
///   会话在调用途中结束，CC 接续时会给悬空的调用补一条回执（41896 次调用里只有 3 次真的没有回执）。
/// - kimi：回合中途的话带 origin.inTurn，后端标 interjected。171 份主链里没有回执的调用只有 1 次。
/// - codex：回合中途的话与回合开头那条写法相同，只是 turn_id 与本回合第一条相同（15 次都是），
///   而且都排在它之前那批调用的回执之后。没有回执的调用 5 次（回合收尾 3、被打断 1、文件末尾 1）。
/// - gemini：调用连同结果一起落盘（44 次全部带结果），时间线里不会有没回执的调用。
/// - opencode：导出器只导出已收场的调用；回合中途发的话当场落库，排在还在跑的那一步后面，
///   导出器等那一步收场再往下导（否则会把在跑的调用当成被打断，且改不回来）。
///
/// 数据取自五家的真实会话样本；截到某条为止 = 轮询读到一半时界面手里的数据。只有「回合中途那句话」
/// 在 claude、kimi 的样本里没有现成的，按后端产出的形态（interjected）给出，逐处注明。

type ToolUse = Extract<ChatItem, { type: "tool_use" }>;

function show(items: ChatItem[], activity: TurnActivity, sessionId = 31) {
  const view = render(
    <ChatCwdContext.Provider value={null}>
      <Transcript sessionId={sessionId} items={items} activity={activity} alive={activity !== "idle"} />
    </ChatCwdContext.Provider>,
  );
  for (let guard = 0; guard < 40; guard += 1) {
    const closed = view.container.querySelector<HTMLElement>(".turn-fold:not(.is-open) > .turn-fold-head");
    if (!closed) break;
    fireEvent.click(closed);
  }
  return view;
}

/** 每条工具行：「状态｜摘要」。 */
const rows = (root: HTMLElement) => [...root.querySelectorAll<HTMLElement>("details.chat-tool")]
  .map((row) => `${row.getAttribute("data-state")}｜${row.querySelector(".chat-tool-summary")?.textContent ?? ""}`);
const lastRow = (root: HTMLElement) => [...root.querySelectorAll<HTMLElement>("details.chat-tool")].pop()!;

/** 这一家第一条普通工具调用所在的样本，截到那条调用为止（它的回执还没到）。 */
function pendingCall(agent: ReplayAgent): { items: ChatItem[]; call: ToolUse } {
  const { items, at } = findInSamples(agent, (item) => item.type === "tool_use" && !item.subagent);
  return { items: items.slice(0, at + 1), call: items[at] as ToolUse };
}

const user = (id: string, extra: Partial<Extract<ChatItem, { type: "user_text" }>> = {}): ChatItem =>
  ({ type: "user_text", id, timestamp: null, text: "顺便把日志也看一下", ...extra });
const interruptedDivider: ChatItem = { type: "meta", id: "divider", timestamp: null, kind: "interrupted" };

afterEach(() => {
  cleanup();
  resetFoldMemory();
});

/// 这三家的时间线里会出现「调用发出了、回执还没到」。回合中途那句话各按自己记录的形态给。
const MID_TURN: Record<"claude" | "kimi" | "codex", (items: ChatItem[]) => ChatItem> = {
  // claude 的 queued_command、kimi 的 origin.inTurn：后端都产出 interjected（各自插件的单测覆盖）。
  claude: () => user("mid", { interjected: true }),
  kimi: () => user("mid", { interjected: true }),
  // codex：写法与回合开头那条相同，turn_id 也相同。
  codex: (items) => {
    const opener = [...items].reverse().find((item) => item.type === "user_text" && !!item.turn_id);
    return user("mid", { turn_id: opener?.type === "user_text" ? opener.turn_id : "turn-1" });
  },
};

describe("回合进行中又发了一条", () => {
  it.each(["claude", "kimi", "codex"] as const)("%s：还没回执的调用仍是运行中，不因为后面多了一条用户消息变成已中断", (agent) => {
    const { items, call } = pendingCall(agent);
    // codex 的样本里那条调用之前不一定有带 turn_id 的用户消息：补上回合开头那条（真实形态）。
    const opened = agent === "codex" && !items.some((item) => item.type === "user_text" && item.turn_id)
      ? [user("opener", { text: "先跑一遍检查", turn_id: "turn-1" }), ...items]
      : items;
    const timeline = reduceChatEvents([], [...opened, MID_TURN[agent](opened)], true);
    const running = show(timeline, "running");
    expect(rows(running.container).pop()).toBe(`running｜${lastRow(running.container).querySelector(".chat-tool-summary")?.textContent}`);
    expect(lastRow(running.container).querySelector(".chat-tool-out.is-running")?.textContent).toMatch(/^运行中…/);
    expect(running.container.textContent).not.toContain("已中断");
    // 回合中途那句话不是回合边界。
    expect(liveTurnStart(timeline)).toBeLessThan(timeline.findIndex((item) => item.id === call.id));
    cleanup();
    resetFoldMemory();

    // 等审批时同理：是待批准，不是已中断。
    const waiting = show(timeline, "waiting");
    expect(lastRow(waiting.container).getAttribute("data-state")).toBe("waiting");
    expect(waiting.container.textContent).not.toContain("已中断");
  });

  it("过程块也不收起：前面跑完了几条、最后一条还在跑时又发了一句，标题仍是正在运行，不写已中断", () => {
    // codex 会话 01a1cea4 的「继续」那一轮，截到第三条调用刚发出；再接上回合中途的那句话（同一个 turn_id）。
    const all = reduceChatEvents([], replaySample("codex.01a1cea4"), true);
    const from = all.findIndex((item) => item.type === "user_text" && item.text === "继续");
    const at = all.findIndex((item) => item.type === "tool_use" && item.summary.startsWith("Get-Content app/src-tauri/CLAUDE.md"));
    const opener = all[from] as Extract<ChatItem, { type: "user_text" }>;
    const timeline = [...all.slice(from, at + 1), user("mid", { turn_id: opener.turn_id })];
    const fold = (root: HTMLElement) => root.querySelector<HTMLElement>(".turn-fold")!;
    const view = render(
      <ChatCwdContext.Provider value={null}><Transcript sessionId={33} items={timeline} activity="running" alive /></ChatCwdContext.Provider>,
    );
    expect(fold(view.container).classList.contains("is-open")).toBe(true);
    expect(fold(view.container).classList.contains("is-running")).toBe(true);
    expect(fold(view.container).querySelector(".turn-fold-lead")?.textContent).toBe("正在运行命令");
    expect(fold(view.container).querySelector(".turn-fold-summary")?.textContent).not.toContain("已中断");
    expect(lastRow(view.container).getAttribute("data-state")).toBe("running");
    // agent 接着说话了：这段过程到此结束，过程块收起；那条命令的回执也到了。
    const receipt = all.find((item) => item.type === "tool_result" && item.tool_use_id === (all[at] as ToolUse).id)!;
    const reply: ChatItem = { type: "assistant_text", id: "reply", timestamp: null, text: "日志也看过了。" };
    view.rerender(
      <ChatCwdContext.Provider value={null}><Transcript sessionId={33} items={[...timeline, receipt, reply]} activity="running" alive /></ChatCwdContext.Provider>,
    );
    expect(fold(view.container).classList.contains("is-open")).toBe(false);
    expect(fold(view.container).querySelector(".turn-fold-summary")?.textContent).not.toContain("已中断");
    cleanup();
    resetFoldMemory();
    // 回合停了：收起，并照实说 1 次已中断。
    const stopped = render(
      <ChatCwdContext.Provider value={null}><Transcript sessionId={34} items={timeline} activity="idle" alive={false} /></ChatCwdContext.Provider>,
    );
    expect(fold(stopped.container).classList.contains("is-open")).toBe(false);
    expect(fold(stopped.container).querySelector(".turn-fold-summary")?.textContent).toContain("1 次已中断");
  });

  it.each(["claude", "kimi", "codex"] as const)("%s：回合已结束或已中止时，同一条调用是已中断", (agent) => {
    const { items } = pendingCall(agent);
    const mid = MID_TURN[agent](items);
    // 回合结束：会话不在跑了。有没有回合中途那句话都一样。
    for (const timeline of [items, [...items, mid]]) {
      const stopped = show(reduceChatEvents([], timeline, true), "idle");
      expect(lastRow(stopped.container).getAttribute("data-state")).toBe("interrupted");
      expect(lastRow(stopped.container).querySelector(".chat-tool-out")?.textContent).toBe("已中断");
      expect(stopped.container.textContent).not.toContain("运行中…");
      cleanup();
      resetFoldMemory();
    }
    // 回合中止：记录里有中断分隔。之后会话又跑起来（新的一轮），被打断的那条仍是已中断。
    const aborted = show(reduceChatEvents([], [...items, interruptedDivider, user("next", { turn_id: "turn-next" })], true), "running");
    expect(lastRow(aborted.container).getAttribute("data-state")).toBe("interrupted");
    cleanup();
    resetFoldMemory();
    // 新回合开始：claude、kimi 不带 interjected 的用户消息，codex 换了 turn_id 的用户消息。
    const next = show(reduceChatEvents([], [...items, user("next", agent === "codex" ? { turn_id: "turn-next" } : {})], true), "running");
    expect(lastRow(next.container).getAttribute("data-state")).toBe("interrupted");
  });

  it("codex 真实记录：回合中途的话与回合开头那条同一个 turn_id，不算回合边界", () => {
    // 会话 01a1cea4：「- 完整闭环」开启回合，13 分钟后回合还在跑，用户又发了一条。
    const items = reduceChatEvents([], replaySample("codex.01a1cea4"), true);
    const users = items.flatMap((item, index) => item.type === "user_text" ? [{ item, index }] : []);
    const opener = users.find(({ item }) => item.text === "- 完整闭环")!;
    const mid = users.find(({ item, index }) => index > opener.index && item.turn_id === opener.item.turn_id)!;
    expect(mid.item.text).toContain("其他的provider的兼容都很差");
    expect(mid.item.interjected).toBeFalsy();
    // 截到回合中途那条为止：当前回合仍从「- 完整闭环」算起。
    const upToMid = items.slice(0, mid.index + 1);
    expect(liveTurnStart(upToMid)).toBe(opener.index);
    // 真实记录里，那条话排在它之前那批调用的回执之后（本机 15 次都是）。
    const { results } = collectSubagentReceipts(upToMid);
    const before = upToMid.slice(opener.index, mid.index).filter((item): item is ToolUse => item.type === "tool_use");
    expect(before.length).toBeGreaterThan(0);
    expect(before.every((call) => results.has(call.id))).toBe(true);
    // 万一有调用跨过了这句话（让到后台的命令）：把紧挨着它的那条命令的回执拿掉，它仍是运行中。
    const pending = [...before].reverse().find((call) => call.kind === "run")!;
    const withheld = upToMid.filter((item) => !(item.type === "tool_result" && item.tool_use_id === pending.id));
    const view = show(withheld, "running");
    const row = [...view.container.querySelectorAll<HTMLElement>("details.chat-tool")]
      .find((el) => el.querySelector(".chat-tool-summary")?.textContent === pending.summary)!;
    expect(row.getAttribute("data-state")).toBe("running");
    // 下一个回合（「继续」，换了 turn_id）才是边界。
    const next = users.find(({ item }) => item.text === "继续")!;
    expect(next.item.turn_id).not.toBe(opener.item.turn_id);
    expect(liveTurnStart(items.slice(0, next.index + 1))).toBe(next.index);
  });

  it("opencode 真实记录：回合中途发的话排在还在跑的那一步之后，那一步按真实结果显示", () => {
    // 会话 KbLGZo：ping 还在跑（03:40:53 到 03:41:12）时用户发了「reply only QUEUED-OK」（03:40:55）。
    const items = reduceChatEvents([], replaySample("opencode.KbLGZo"), true);
    const ping = items.findIndex((item) => item.type === "tool_use");
    const queued = items.findIndex((item) => item.type === "user_text" && item.text === "reply only QUEUED-OK");
    expect(queued).toBeGreaterThan(ping);
    expect(Date.parse(items[queued].timestamp!)).toBeLessThan(Date.parse((items[ping + 1] as Extract<ChatItem, { type: "tool_result" }>).timestamp!));
    for (const activity of ["running", "idle"] as const) {
      const view = show(items.slice(0, queued + 1), activity);
      expect(rows(view.container)).toEqual(["done｜ping -n 20 127.0.0.1"]);
      expect(view.container.textContent).not.toContain("已中断");
      cleanup();
      resetFoldMemory();
    }
  });

  it.each(["gemini", "opencode"] as const)("%s：时间线里没有不带回执的调用", (agent) => {
    // gemini 的调用连同结果一起落盘；opencode 的导出器只导出已收场的调用。所以这两家不存在
    // 「调用在跑、后面又来了一条用户消息」的时间线，回合边界对它们的工具运行态没有影响。
    for (const sample of replaySamples(agent)) {
      const { results } = collectSubagentReceipts(sample.items);
      const calls = sample.items.filter((item): item is ToolUse => item.type === "tool_use");
      expect(calls.filter((call) => !results.has(call.id)).map((call) => call.summary), sample.name).toEqual([]);
    }
  });
});

describe("真实会话里没跑完的调用", () => {
  it("claude：会话在调用途中结束，CC 补写的回执显示成已中断，不是红色报错", () => {
    // 会话 a7bec18a（2.1.316）：AskUserQuestion 发出后会话结束，5 分钟后接续时 CC 补了一条回执。
    const items = reduceChatEvents([], replaySample("claude.a7bec18a"), true);
    const receipt = items.find((item): item is Extract<ChatItem, { type: "tool_result" }> => item.type === "tool_result")!;
    expect(receipt.text).toMatch(/^\[Tool call interrupted: the session ended before/);
    expect(receipt.interrupted).toBe(true);
    // 会话此刻在不在跑都一样：这是记录里写明的终态。
    for (const activity of ["running", "idle"] as const) {
      const view = show(items, activity);
      const row = lastRow(view.container);
      expect(row.getAttribute("data-state")).toBe("interrupted");
      expect(row.classList.contains("is-error")).toBe(false);
      expect(row.querySelector(":scope > summary > .chat-tool-status")?.textContent).toBe("已中断");
      expect(view.container.textContent).not.toContain("运行中…");
      cleanup();
      resetFoldMemory();
    }
  });

  it("kimi：命令没等到回执，用户开了新回合，旧调用是已中断、新回合的调用照常", () => {
    // 会话 4f41d9c0：taskkill 那条命令杀掉了宿主进程，记录里没有它的回执；20 分钟后用户发了新消息。
    const items = reduceChatEvents([], replaySample("kimi.4f41d9c0"), true);
    const { results } = collectSubagentReceipts(items);
    const orphan = items.find((item): item is ToolUse => item.type === "tool_use" && !results.has(item.id))!;
    expect(orphan.summary).toContain("taskkill");
    const reopened = items.findIndex((item) => item.type === "user_text");
    expect(reopened).toBeGreaterThan(items.indexOf(orphan));
    expect((items[reopened] as Extract<ChatItem, { type: "user_text" }>).interjected).toBeFalsy();
    // 新回合在跑：旧调用已中断，不再计时。
    const running = show(items, "running");
    const states = rows(running.container);
    expect(states.filter((row) => row.startsWith("interrupted｜"))).toHaveLength(1);
    expect(states.find((row) => row.includes("taskkill"))).toMatch(/^interrupted｜/);
    expect(states[states.length - 1]).toMatch(/^done｜/);
    expect(running.container.textContent).not.toContain("运行中…");
    cleanup();
    resetFoldMemory();
    // 截到那条命令刚发出、会话还在跑：这时它确实在跑。
    const live = show(items.slice(0, items.indexOf(orphan) + 1), "running");
    expect(lastRow(live.container).getAttribute("data-state")).toBe("running");
  });

  it("codex：回合收尾、被 Esc 打断、文件到此为止，三种没有回执的调用", () => {
    // 会话 01a13ed0：三段脚本各两条命令，都没有回执。
    const items = reduceChatEvents([], replaySample("codex.01a13ed0"), true);
    const { results } = collectSubagentReceipts(items);
    const calls = items.filter((item): item is ToolUse => item.type === "tool_use");
    expect(calls).toHaveLength(6);
    expect(calls.every((call) => !results.has(call.id))).toBe(true);
    // 会话已结束：全部已中断。
    const ended = show(items, "idle");
    expect(rows(ended.container).every((row) => row.startsWith("interrupted｜"))).toBe(true);
    expect(ended.container.textContent).not.toContain("运行中…");
    cleanup();
    resetFoldMemory();
    // 会话在跑：前两段分别被新回合（换了 turn_id 的用户消息）和中断分隔越过，是已中断；
    // 最后一段在时间线末尾、之后没有任何终止信号，按在跑算。
    const running = show(items, "running");
    expect(rows(running.container).map((row) => row.split("｜")[0])).toEqual([
      "interrupted", "interrupted", "interrupted", "interrupted", "running", "running",
    ]);
  });
});

describe("codex 的子代理是子任务块", () => {
  const badges = (root: HTMLElement) => [...root.querySelectorAll<HTMLElement>("details.chat-subagent")]
    .map((block) => `${block.querySelector(".chat-subagent-desc")?.textContent}｜${block.querySelector(".chat-subagent-status")?.textContent ?? ""}`);
  const timeline = () => reduceChatEvents([], replaySample("codex.01a1cea4"), true);
  const upTo = (pick: (item: ChatItem) => boolean) => {
    const items = timeline();
    return items.slice(0, items.findIndex(pick) + 1);
  };

  it("与 claude、kimi 同一个组件，标题是任务名", () => {
    for (const agent of ["claude", "kimi", "codex"] as const) {
      const { items } = findInSamples(agent, (item) => item.type === "tool_use" && !!item.subagent);
      const view = show(reduceChatEvents([], items, true), "idle", 40 + REPLAY_AGENTS.indexOf(agent));
      expect(view.container.querySelectorAll("details.chat-subagent").length, agent).toBeGreaterThan(0);
      // 子任务不再画成一行普通工具。
      const verbs = [...view.container.querySelectorAll(".chat-tool-verb")].map((verb) => verb.textContent);
      expect(verbs, agent).not.toContain("子任务");
      cleanup();
      resetFoldMemory();
    }
  });

  it("开始、跑完、被叫停，状态取自记录里的 SubAgentActivity", () => {
    // 刚派出（spawn_agent 调用已写，started 还没到）：回合在跑 = 运行中，回合停了 = 已中断。
    const spawned = upTo((item) => item.type === "tool_use" && item.name === "spawn_agent");
    expect(badges(show(spawned, "running").container)).toEqual(["lead_design_one｜运行中"]);
    cleanup();
    resetFoldMemory();
    expect(badges(show(spawned, "idle").container)).toEqual(["lead_design_one｜已中断"]);
    cleanup();
    resetFoldMemory();

    // started 到了：在跑。它跑在自己的线程里，主回合停了、进程还在，它仍在跑；进程没了才是已中断。
    const started = upTo((item) => item.type === "tool_result" && item.subagent?.task_id === "/root/lead_design_two" && item.subagent.running === 1);
    expect(badges(show(started, "running").container)).toEqual(["lead_design_one｜运行中", "lead_design_two｜运行中"]);
    expect(collectBackgroundTasks(started, { activity: "running", alive: true }).subagents.map((task) => `${task.name}｜${task.state}`))
      .toEqual(["lead_design_one｜run", "lead_design_two｜run"]);
    expect(collectBackgroundTasks(started, { activity: "idle", alive: false }).subagents.map((task) => task.state)).toEqual(["stop", "stop"]);
    cleanup();
    resetFoldMemory();

    // 两个都跑完一轮（completed 只带路径，按路径归回各自的委派）。
    const completed = upTo((item) => item.type === "tool_result" && item.subagent?.task_id === "/root/lead_design_one" && item.subagent.completed === 1);
    expect(badges(show(completed, "running").container)).toEqual(["lead_design_one｜已完成", "lead_design_two｜已完成"]);
    cleanup();
    resetFoldMemory();

    // 之后父线程调了 interrupt_agent 叫停 lead_design_one：已中断（不是失败，也不是完成）。
    const whole = show(timeline(), "idle");
    expect(badges(whole.container)).toEqual(["lead_design_one｜已中断", "lead_design_two｜已完成", "gemini_e2e｜已完成"]);
    expect(whole.container.querySelector(".chat-subagent-status.is-failed")).toBeNull();
    expect(collectBackgroundTasks(timeline(), { activity: "idle", alive: false }).subagents.map((task) => `${task.name}｜${task.state}`))
      .toEqual(["lead_design_one｜stop", "lead_design_two｜done", "gemini_e2e｜done"]);
    // interrupt_agent 自己那一行照常显示它的回执，没有被结局回执顶掉。
    expect(rows(whole.container)).toContain("done｜lead_design_one");
  });
});
