import { describe, expect, it } from "vitest";
import type { ChatHistory } from "../../api";
import { isHandoffInjectedPrompt, sameHistoryMeta } from "./historyMeta";

const base = { items: [], offset: 0, reset: false, agentModes: [{ dimension: "permission", value: "default" }], todos: [{ content: "a", status: "pending" }], connected: true } as unknown as ChatHistory;
const next = (patch: Record<string, unknown>) => ({ ...base, ...patch }) as unknown as ChatHistory;

describe("sameHistoryMeta", () => {
  it("第一次（没有旧值）算不同", () => expect(sameHistoryMeta(null, base)).toBe(false));
  it("只有 items、offset、reset 变了算相同，这三样另有短路", () => {
    expect(sameHistoryMeta(base, next({ items: [{}], offset: 9, reset: true }))).toBe(true);
  });
  it("模式、待办按内容比，新数组同内容算相同", () => {
    expect(sameHistoryMeta(base, next({ agentModes: [{ dimension: "permission", value: "default" }], todos: [{ content: "a", status: "pending" }] }))).toBe(true);
    expect(sameHistoryMeta(base, next({ agentModes: [{ dimension: "permission", value: "plan" }] }))).toBe(false);
    expect(sameHistoryMeta(base, next({ todos: [{ content: "a", status: "completed" }] }))).toBe(false);
  });
  it("其余字段默认参与比较：后端新增字段不用回来改这里", () => {
    expect(sameHistoryMeta(base, next({ connected: false }))).toBe(false);
    expect(sameHistoryMeta(base, next({ someNewField: 1 }))).toBe(false);
  });
  it("旧后端没有 todos 字段不崩", () => {
    const old = next({ todos: undefined });
    expect(sameHistoryMeta(old, old)).toBe(true);
  });
});

describe("isHandoffInjectedPrompt", () => {
  it("两种语言的交接注入语都认，普通消息不认", () => {
    expect(isHandoffInjectedPrompt("请先完整阅读文件 C:/x/meowo-handoff/a.md")).toBe(true);
    expect(isHandoffInjectedPrompt("Please read the file /tmp/meowo-handoff/a.md")).toBe(true);
    expect(isHandoffInjectedPrompt("请先完整阅读文件 README.md")).toBe(false);
    expect(isHandoffInjectedPrompt("聊聊 meowo-handoff 目录")).toBe(false);
  });
});
