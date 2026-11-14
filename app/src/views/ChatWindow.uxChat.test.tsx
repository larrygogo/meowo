// 第三轮体验修复（对话窗这一组）的回归：菜单选完焦点回触发钮（走查 C-1）、斜杠菜单点选焦点留在
// 输入框（C-2）、错误条给出路（C-4）、空框 Esc 两段式中断（C-9）、移除附件焦点去下一个（C-10）、
// 输入框被锁时附件 / 模型 / 模式按钮一并锁（backlog 仍存在第 3 条，验收 21）。
// 走查原始日志：scratchpad/ux2/walk/p1/log3.txt（p3-model-kbd-pick、p3-mode-kbd-pick、p3-slash-click、
// p3-att-remove 选完 / 点完 activeElement 均为 BODY）。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { chatUiReady, invoke, openDialog, pickAttachments, respondWithHistory } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";

const idle = (sessionId: number, over: Record<string, unknown> = {}) => ({
  sessionId, title: `对话窗-${sessionId}`, status: "idle", provider: "claude", cwd: "C:/repo",
  supported: true, offset: 1, reset: false, pendingReview: null, ptyManaged: true,
  items: [{ type: "assistant_text", id: "a1", timestamp: null, text: "好的" }],
  ...over,
});
const box = () => screen.getByRole("combobox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement;
const writes = (sessionId: number) => invoke.mock.calls
  .filter(([command, args]) => command === "write_managed_terminal" && (args as { sessionId: number }).sessionId === sessionId)
  .map(([, args]) => (args as { data: string }).data);

/**
 * 把切模型的确认轮询快进完（做空第一轮 #12，2026-11-12）：useModelControls 等回话是 12×200ms 的真实
 * setTimeout，原先用 findByText(…, { timeout: 4000 }) 干等，全量 vitest 并发吃满 CPU 时偶发超时。
 * 只假 setTimeout 系列（rAF、Promise 不动），shouldAdvanceTime 让 waitFor 之类照常随真实时间走。
 */
function fakeTimers() {
  vi.useFakeTimers({ shouldAdvanceTime: true, toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
}
async function fastForward(ms: number) {
  await act(async () => { await vi.advanceTimersByTimeAsync(ms); });
}
afterEach(() => { vi.useRealTimers(); });

describe("对话窗：焦点", () => {
  it("权限模式菜单选完一项，焦点回到模式钮（C-1）", async () => {
    window.history.replaceState({}, "", "/?sessionId=801");
    respondWithHistory(idle(801));
    render(<ChatWindow />);
    await chatUiReady();
    const mode = await screen.findByRole("button", { name: /^切换模式: 权限模式/ });
    mode.focus();
    fireEvent.click(mode);
    const item = screen.getByRole("menuitem", { name: /计划/ });
    item.focus();
    fireEvent.click(item);
    expect(screen.queryByRole("menuitem", { name: /计划/ })).toBeNull();
    expect(document.activeElement).toBe(mode);
    // 发送在途（循环按 Shift+Tab 到位）期间模式钮不被 disabled 踢走焦点。
    await waitFor(() => expect(writes(801)).toContain("\u001b[Z"));
    expect((mode as HTMLButtonElement).disabled).toBe(false);
    expect(document.activeElement).toBe(mode);
  });

  it("模型菜单选完焦点回模型钮；没等到确认时错误条给「重试」「去终端页」「收起」（C-1、C-4）", async () => {
    fakeTimers();
    window.history.replaceState({}, "", "/?sessionId=802");
    respondWithHistory(idle(802));
    render(<ChatWindow />);
    await chatUiReady();
    const model = await screen.findByRole("button", { name: "切换模型" });
    model.focus();
    fireEvent.click(model);
    fireEvent.click(screen.getByRole("menuitem", { name: "Sonnet" }));
    expect(document.activeElement).toBe(model);
    await waitFor(() => expect(writes(802).some((data) => data.includes("/model sonnet"))).toBe(true));
    // 假终端从不回「Set model to」：12×200ms 后报未确认（快进，不干等真实时间）。
    await fastForward(3_000);
    expect(screen.getByText(zh.chat.modelSwitchUnconfirmed)).toBeTruthy();
    const bar = document.querySelector(".chat-send-error")!;
    const labels = [...bar.querySelectorAll("button")].map((button) => button.textContent);
    expect(labels).toEqual([zh.chat.errorRetry, zh.chat.goTerminal, zh.chat.slashMenuDismiss]);
    expect(bar.textContent).not.toMatch(/请到终端/);
    // 收起：错误条消失、焦点交给输入框（按钮随错误条卸载，不能掉到 body）。
    fireEvent.click(screen.getByRole("button", { name: zh.chat.slashMenuDismiss }));
    expect(document.querySelector(".chat-send-error")).toBeNull();
    expect(document.activeElement).toBe(box());
  });

  it("错误条的「重试」就地再切一次，换了错误后旧的出路作废（C-4）", async () => {
    fakeTimers();
    window.history.replaceState({}, "", "/?sessionId=803");
    respondWithHistory(idle(803));
    render(<ChatWindow />);
    await chatUiReady();
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Sonnet" }));
    await fastForward(3_000);
    expect(screen.getByText(zh.chat.modelSwitchUnconfirmed)).toBeTruthy();
    const before = writes(803).filter((data) => data.includes("/model sonnet")).length;
    fireEvent.click(screen.getByRole("button", { name: zh.chat.errorRetry }));
    await waitFor(() => expect(writes(803).filter((data) => data.includes("/model sonnet")).length).toBe(before + 1));
    expect(document.activeElement).toBe(box());
    // 打字清掉错误：出路跟着那条错误一起走。
    await fastForward(3_000);
    expect(screen.getByText(zh.chat.modelSwitchUnconfirmed)).toBeTruthy();
    fireEvent.change(box(), { target: { value: "继续" } });
    expect(screen.queryByRole("button", { name: zh.chat.errorRetry })).toBeNull();
  });

  it("斜杠菜单用鼠标点选：焦点留在输入框，光标停在命令后（C-2）", async () => {
    window.history.replaceState({}, "", "/?sessionId=804");
    respondWithHistory(idle(804));
    render(<ChatWindow />);
    await chatUiReady();
    const input = box();
    input.focus();
    fireEvent.change(input, { target: { value: "/mo" } });
    const option = screen.getByRole("option", { name: /^\/model/ });
    // mousedown 不抢焦点（preventDefault），click 后下一帧补一次聚焦。
    expect(fireEvent.mouseDown(option)).toBe(false);
    fireEvent.click(option);
    await act(async () => { await new Promise((resolve) => requestAnimationFrame(() => resolve(null))); });
    expect(input.value).toBe("/model ");
    expect(document.activeElement).toBe(input);
    expect(input.selectionStart).toBe("/model ".length);
  });

  it("移除附件：焦点去下一个附件的「×」，最后一个移除后回输入框（C-10）", async () => {
    window.history.replaceState({}, "", "/?sessionId=805");
    respondWithHistory(idle(805));
    openDialog.mockResolvedValueOnce(["C:\\repo\\one.txt", "C:\\repo\\two.txt"]);
    render(<ChatWindow />);
    await chatUiReady();
    pickAttachments();
    const first = await screen.findByRole("button", { name: `${zh.chat.removeAttachment} one.txt` });
    first.focus();
    fireEvent.click(first);
    await act(async () => { await new Promise((resolve) => requestAnimationFrame(() => resolve(null))); });
    const second = screen.getByRole("button", { name: `${zh.chat.removeAttachment} two.txt` });
    expect(document.activeElement).toBe(second);
    fireEvent.click(second);
    await act(async () => { await new Promise((resolve) => requestAnimationFrame(() => resolve(null))); });
    expect(document.querySelector(".chat-attachments")).toBeNull();
    expect(document.activeElement).toBe(box());
  });
});

describe("对话窗：键盘", () => {
  it("回合在跑、输入框为空：Esc 第一下只提示，第二下才中断（C-9）", async () => {
    window.history.replaceState({}, "", "/?sessionId=806");
    respondWithHistory(idle(806, { status: "running" }));
    render(<ChatWindow />);
    await chatUiReady();
    const input = box();
    await waitFor(() => expect(input.placeholder).not.toBe(zh.chat.inputPlaceholder));
    fireEvent.keyDown(input, { key: "Escape" });
    expect(input.placeholder).toBe(zh.chat.escInterruptArmed);
    expect(writes(806)).not.toContain("\x1b");
    fireEvent.keyDown(input, { key: "Escape" });
    await waitFor(() => expect(writes(806)).toContain("\x1b"));
    expect(input.placeholder).not.toBe(zh.chat.escInterruptArmed);
  });

  it("开口打字就撤销武装的 Esc：有草稿时 Esc 不中断", async () => {
    window.history.replaceState({}, "", "/?sessionId=807");
    respondWithHistory(idle(807, { status: "running" }));
    render(<ChatWindow />);
    await chatUiReady();
    const input = box();
    fireEvent.keyDown(input, { key: "Escape" });
    expect(input.placeholder).toBe(zh.chat.escInterruptArmed);
    fireEvent.change(input, { target: { value: "等等" } });
    fireEvent.keyDown(input, { key: "Escape" });
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.keyDown(input, { key: "Escape" });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(writes(807)).not.toContain("\x1b");
  });

  it("空闲时 Esc 什么都不做", async () => {
    window.history.replaceState({}, "", "/?sessionId=808");
    respondWithHistory(idle(808));
    render(<ChatWindow />);
    await chatUiReady();
    const input = box();
    fireEvent.keyDown(input, { key: "Escape" });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(input.placeholder).not.toBe(zh.chat.escInterruptArmed);
    expect(writes(808)).not.toContain("\x1b");
  });
});

describe("输入框被锁时按钮一并锁（验收 21）", () => {
  it("会话已结束（门卡态）：附件、模型、模式都不可点", async () => {
    window.history.replaceState({}, "", "/?sessionId=809");
    respondWithHistory(idle(809, { status: "ended", ptyManaged: false, agentModes: [{ dimension: "permission", value: "default" }] }));
    render(<ChatWindow />);
    await chatUiReady();
    await waitFor(() => expect(box().disabled).toBe(true));
    const add = document.querySelector(".chat-compose .chat-attach-button") as HTMLButtonElement;
    expect(add.disabled).toBe(true);
    expect((screen.getByRole("button", { name: "切换模型" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: /^切换模式: 权限模式/ }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("运行中：模型选择排到本轮之后，说明等待条件且不写终端（C-8）", async () => {
    window.history.replaceState({}, "", "/?sessionId=810");
    respondWithHistory(idle(810, { status: "running" }));
    render(<ChatWindow />);
    await chatUiReady();
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Sonnet" }));
    expect(await screen.findByText(zh.chat.settingsDeferred("Sonnet"))).toBeTruthy();
    expect(writes(810).some((data) => data.includes("/model"))).toBe(false);
  });
});
