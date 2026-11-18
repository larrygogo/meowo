import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render } from "@testing-library/react";
import type { ChatItem } from "../../api";
import { resetFoldMemory } from "./foldMemory";
import { ChatCwdContext } from "./ToolActivity";
import { Transcript } from "./Transcript";
import { ENTER_BULK, ENTER_CLASS, trackEntering } from "./enterMotion";

/// U6：实时新增的回复段落与工具行渐显一次；历史首屏、加载更早、切换会话不动。

const ts = "2026-11-18T08:00:00.000Z";
const user = (id: string, text = "问题"): ChatItem => ({ type: "user_text", id, timestamp: ts, text });
const reply = (id: string, text = "回答一段。"): ChatItem => ({ type: "assistant_text", id, timestamp: ts, text });
const tool = (id: string): ChatItem => ({ type: "tool_use", id, timestamp: ts, name: "Bash", kind: "run", summary: `echo ${id}` } as ChatItem);
const result = (id: string, of: string): ChatItem => ({ type: "tool_result", id, timestamp: ts, tool_use_id: of, text: "ok", is_error: false });

const ui = (sessionId: number, items: ChatItem[]) => (
  <ChatCwdContext.Provider value={null}>
    <Transcript sessionId={sessionId} items={items} activity="running" alive />
  </ChatCwdContext.Provider>
);
const entering = (root: HTMLElement) => [...root.querySelectorAll(`.${ENTER_CLASS}`)].map((el) => el.getAttribute("data-enter-id"));

afterEach(() => {
  cleanup();
  resetFoldMemory();
});

describe("trackEntering（判法）", () => {
  const history = [user("u1"), reply("a1"), tool("t1"), result("r1", "t1"), reply("a2")];

  it("第一次见到会话：全部记为已见，不动", () => {
    expect(trackEntering(null, 1, history).entering).toEqual([]);
  });

  it("末尾追加的回复与工具行算新到；用户消息与回执不算", () => {
    const first = trackEntering(null, 1, history).track;
    const next = [...history, user("u2"), tool("t2"), result("r2", "t2"), reply("a3")];
    expect(trackEntering(first, 1, next).entering).toEqual(["t2", "a3"]);
  });

  it("加载更早（插在头部）不动", () => {
    const first = trackEntering(null, 1, history).track;
    const earlier = [user("u0"), reply("a0"), ...history];
    expect(trackEntering(first, 1, earlier).entering).toEqual([]);
  });

  it("切换会话不动；切回来也按首屏处理", () => {
    const first = trackEntering(null, 1, history).track;
    const other = trackEntering(first, 2, [user("x1"), reply("x2")]);
    expect(other.entering).toEqual([]);
    expect(trackEntering(other.track, 1, [...history, reply("a9")]).entering).toEqual([]);
  });

  it("空会话第一次拿到历史（加载完成）不动", () => {
    const empty = trackEntering(null, 1, []).track;
    expect(trackEntering(empty, 1, history).entering).toEqual([]);
  });

  it(`一轮补进超过 ${ENTER_BULK} 条按整段补读处理，不动`, () => {
    const first = trackEntering(null, 1, history).track;
    const burst = Array.from({ length: ENTER_BULK + 1 }, (_, i) => reply(`b${i}`));
    expect(trackEntering(first, 1, [...history, ...burst]).entering).toEqual([]);
  });
});

describe("Transcript 渲染", () => {
  it("首屏渲染的条目没有渐显类；实时追加的回复段落与工具行有", () => {
    const history = [user("u1"), reply("a1")];
    const { container, rerender } = render(ui(7, history));
    expect(entering(container)).toEqual([]);
    rerender(ui(7, [...history, tool("t1")]));
    expect(entering(container)).toEqual(["t1"]);
    rerender(ui(7, [...history, tool("t1"), result("r1", "t1"), reply("a2")]));
    expect(entering(container)).toContain("a2");
  });

  it("加载更早插到头部的条目不挂类", () => {
    const history = [user("u1"), reply("a1")];
    const { container, rerender } = render(ui(7, history));
    rerender(ui(7, [user("u0"), reply("a0"), ...history]));
    expect(entering(container)).toEqual([]);
  });

  it("切换会话后的首屏不挂类", () => {
    const { container, rerender } = render(ui(7, [user("u1"), reply("a1")]));
    rerender(ui(8, [user("v1"), reply("v2"), tool("v3")]));
    expect(entering(container)).toEqual([]);
  });

  it("动画播完摘掉类（折叠块重挂载时不重播）", () => {
    const history = [user("u1"), reply("a1")];
    const { container, rerender } = render(ui(7, history));
    rerender(ui(7, [...history, reply("a2")]));
    const el = container.querySelector<HTMLElement>(`[data-enter-id="a2"]`)!;
    expect(el.classList.contains(ENTER_CLASS)).toBe(true);
    // 子元素冒泡上来的动画结束不算
    el.firstElementChild?.dispatchEvent(new Event("animationend", { bubbles: true }));
    expect(el.classList.contains(ENTER_CLASS)).toBe(true);
    el.dispatchEvent(new Event("animationend", { bubbles: true }));
    expect(el.classList.contains(ENTER_CLASS)).toBe(false);
  });
});
