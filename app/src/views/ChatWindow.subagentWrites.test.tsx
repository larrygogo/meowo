// 终端停在子代理视图时，对话页的五个写入口（正文、贴图、/btw、中断、撤回）都要先切回主会话再写；
// 切不回就不写（做空第二轮 B/C）。中断尤其危险：子代理视图里 Esc 中断的是被看的子代理。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke, openDialog, pickAttachments, adapter } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";

const { setup, box, writes } = adapter;
const RULE = "─".repeat(60);

// 2.1.322 探针形态（同 chat/subagentView.test.ts）：停在子代理视图、面板没画出来——切不回。
const STUCK = ["● 已派出", `${"─".repeat(40)} 后端实现 Excel 附件解析 ─`, "❯ Message @general-purpose…", RULE];
// 停在子代理视图、面板焦点在 main 行：回车即切回。
const MAIN_SELECTED = [`${"─".repeat(40)} 后端实现 Excel 附件解析 ─`, "❯ Message @general-purpose…", RULE, "  ↑/↓ to select · Enter to view", "❯ ◯ main", "  ● general-purpose  后端实现 Excel 附件解析      11s"];
// 回到主会话：普通 composer。
const MAIN_IDLE = ["● 已派出", RULE, "❯ ", RULE, "  ⏵⏵ auto mode on (shift+tab to cycle) · ← for agents"];

type Screen = { lines: string[] };

/// 会话夹具 + 可切换的屏幕：在子代理视图里收到回车（面板选中 main 时）就回到主会话。
function mount(sessionId: number, start: string[], status = "waiting", items: unknown[] = []) {
  const base = setup(sessionId, "claude", status);
  const screenNow: Screen = { lines: start };
  invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
    if (command === "get_chat_history") return base(command, args).then((h: object) => ({ ...h, offset: 5, items: args?.offset ? [] : items }));
    if (command === "confirm_dialog") return Promise.resolve(true);
    if (command === "managed_terminal_screen") return Promise.resolve(screenNow.lines);
    if (command === "write_managed_terminal" && args?.data === "\r" && screenNow.lines === MAIN_SELECTED) screenNow.lines = MAIN_IDLE;
    return base(command, args);
  });
  render(<ChatWindow />);
  return screenNow;
}

async function ready(sessionId: number) {
  await screen.findByText(`适配-${sessionId}`);
  await waitFor(() => expect(invoke.mock.calls.some(([command]) => command === "agent_chat_ui")).toBe(true));
  // 等 chatUi（含 subagent_view 声明）真正落进状态：只等到调用发出时偶发还没生效，
  // 中断就跳过了切回（5 次里 1 次）。
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 100)); });
}

const notice = () => screen.findByText(zh.chat.subagentViewNotice("后端实现 Excel 附件解析"), {}, { timeout: 4000 });

describe("终端停在子代理视图：写入前先切回主会话", () => {
  it("正文：先切回再写；切不回不写", async () => {
    mount(601, MAIN_SELECTED);
    await ready(601);
    fireEvent.change(box(), { target: { value: "给主会话" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(writes(601)).toContain("给主会话"), { timeout: 4000 });
    const sent = writes(601);
    expect(sent.indexOf("\r")).toBeGreaterThanOrEqual(0);
    expect(sent.indexOf("\r")).toBeLessThan(sent.indexOf("给主会话"));
  });

  it("正文：切不回时不写，横幅说明", async () => {
    mount(602, STUCK);
    await ready(602);
    fireEvent.change(box(), { target: { value: "给主会话" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await notice();
    expect(writes(602)).toEqual([]);
  });

  it("贴图：切不回时不粘贴", async () => {
    mount(603, STUCK);
    await ready(603);
    openDialog.mockResolvedValueOnce(["C:\\repo\\shot.png"]);
    pickAttachments();
    await screen.findByAltText("shot.png");
    fireEvent.change(box(), { target: { value: "看图" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await notice();
    expect(writes(603)).toEqual([]);
  });

  it("/btw：切不回时不发", async () => {
    mount(604, STUCK);
    await ready(604);
    fireEvent.change(box(), { target: { value: "/btw 刚才做了什么？" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await notice();
    expect(writes(604)).toEqual([]);
  });

  it("中断：先切回再发 Esc，Esc 不落到子代理上", async () => {
    mount(605, MAIN_SELECTED, "running");
    await ready(605);
    fireEvent.click(await screen.findByRole("button", { name: zh.chat.interruptNow }));
    await waitFor(() => expect(writes(605)).toContain("\u001b"), { timeout: 4000 });
    // 先回车切回（面板选中 main），再发中断键。
    const sent = writes(605);
    expect(sent.indexOf("\r")).toBeGreaterThanOrEqual(0);
    expect(sent.indexOf("\r")).toBeLessThan(sent.indexOf("\u001b"));
  });

  it("中断：切不回时不发 Esc，横幅说明；再点一次也不发", async () => {
    mount(606, STUCK, "running");
    await ready(606);
    const button = await screen.findByRole("button", { name: zh.chat.interruptNow });
    fireEvent.click(button);
    await notice();
    fireEvent.click(button);
    await new Promise((resolve) => setTimeout(resolve, 600));
    expect(writes(606)).not.toContain("\u001b");
  });

  it("中断并处理：切不回时不发 Esc（看过提示照发的正文也不算数）", async () => {
    mount(608, STUCK, "running");
    await ready(608);
    fireEvent.change(box(), { target: { value: "插一句" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await notice();
    // 看过提示仍点发送：正文照发给它（既有约定），于是有了排队回执和「中断并处理」。
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(writes(608)).toContain("插一句"), { timeout: 4000 });
    fireEvent.click(await screen.findByRole("button", { name: "中断并处理" }));
    await new Promise((resolve) => setTimeout(resolve, 600));
    expect(writes(608)).not.toContain("\u001b");
  });

  it("撤回：切不回时不发 /rewind", async () => {
    const items = [
      { type: "user_text", id: "u1", timestamp: null, text: "先做甲" },
      { type: "assistant_text", id: "a1", timestamp: null, text: "甲做完了" },
    ];
    mount(607, STUCK, "waiting", items);
    await screen.findByText("甲做完了");
    await ready(607);
    fireEvent.click((await screen.findAllByRole("button", { name: zh.chat.undoTip }))[0]);
    await notice();
    expect(writes(607).join("")).not.toContain("/rewind");
  });
});
