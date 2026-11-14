// 审批卡与提问卡在对话窗里的整体行为：五家同一张卡、同一套键、同一条输入框规则。
// 卡片本体与视图模型的单测在 chat/ApprovalRequestCard.test.tsx、chat/requestCard.test.ts。
import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { adapter, invoke, terminalProps } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";
import { markRemoteUi } from "../remoteMode";
import { terminalAttention } from "../terminalAttention";
import { grammar } from "../test/agents";
import {
  CLAUDE_APPROVAL_SCREEN,
  CLAUDE_BROKER_APPROVAL,
  CODEX_BROKER_APPROVAL,
  GEMINI_APPROVAL_SCREEN,
  KIMI_APPROVAL_SCREEN,
  OPENCODE_ALWAYS_CONFIRM_SCREEN,
  OPENCODE_PERMISSION_SCREEN,
} from "../test/requestSamples";

/// 起一个会话；broker 通道的把样本请求挂到 pending_interaction 上。
function setup(sessionId: number, provider: string, approval: unknown = null) {
  const base = adapter.setup(sessionId, provider, "running");
  invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
    if (command === "get_chat_history") return Promise.resolve({ ...adapter.history(sessionId, provider, "running"), ptyManaged: true, pendingReview: approval ? "approval" : null });
    if (command === "pending_interaction") return Promise.resolve({ approval: approval ? { ...(approval as object), sessionId } : null, question: null });
    return base(command, args);
  });
}

/// 屏幕通道：用真实解析器认样本，再经 ManagedTerminal 的回调交给对话窗（与生产路径同一条链）。
async function showScreen(provider: string, screenText: string) {
  await waitFor(() => expect(terminalProps.current?.onAttention).toBeTypeOf("function"));
  const attention = terminalAttention(screenText, [], false, false, grammar(provider));
  expect(attention).toBeTruthy();
  act(() => (terminalProps.current!.onAttention as (value: unknown) => void)(attention));
}

const CARDS: [string, number, (id: number) => Promise<void>][] = [
  ["claude（broker）", 901, async (id) => { setup(id, "claude", CLAUDE_BROKER_APPROVAL); render(<ChatWindow />); }],
  ["codex（broker）", 902, async (id) => { setup(id, "codex", CODEX_BROKER_APPROVAL); render(<ChatWindow />); }],
  ["kimi（屏幕）", 903, async (id) => { setup(id, "kimi"); render(<ChatWindow />); await showScreen("kimi", KIMI_APPROVAL_SCREEN); }],
  ["gemini（屏幕）", 904, async (id) => { setup(id, "gemini"); render(<ChatWindow />); await showScreen("gemini", GEMINI_APPROVAL_SCREEN); }],
  ["opencode（屏幕）", 905, async (id) => { setup(id, "opencode"); render(<ChatWindow />); await showScreen("opencode", OPENCODE_PERMISSION_SCREEN); }],
  ["claude（屏幕兜底）", 906, async (id) => { setup(id, "claude"); render(<ChatWindow />); await showScreen("claude", CLAUDE_APPROVAL_SCREEN); }],
];

/// 每家「允许一次」「拒绝」实际送达的东西：broker 是应答词，屏幕是按键。
const DELIVERY: Record<string, { allow: unknown; deny: unknown }> = {
  "claude（broker）": { allow: ["respond_runtime_request", "allow_once"], deny: ["respond_runtime_request", "deny"] },
  "codex（broker）": { allow: ["respond_runtime_request", "allow_once"], deny: ["respond_runtime_request", "deny"] },
  "kimi（屏幕）": { allow: ["write_managed_terminal", "1"], deny: ["write_managed_terminal", "3"] },
  "gemini（屏幕）": { allow: ["write_managed_terminal", "\r"], deny: ["write_managed_terminal", "\x1b[B\x1b[B\r"] },
  "opencode（屏幕）": { allow: ["write_managed_terminal", "\r"], deny: ["write_managed_terminal", "\x1b"] },
  "claude（屏幕兜底）": { allow: ["write_managed_terminal", "\r"], deny: ["write_managed_terminal", "\x1b[B\x1b[B\x1b[B\r"] },
};

function delivered(sessionId: number): unknown[] {
  return invoke.mock.calls.flatMap(([command, args]) => {
    if (command === "respond_runtime_request" && args?.sessionId === sessionId) return [[command, args.choice]];
    if (command === "write_managed_terminal" && args?.sessionId === sessionId) return [[command, args.data]];
    return [];
  });
}

describe("审批卡在对话窗里：五家同一套", () => {
  it.each(CARDS)("%s：同一张卡，「拒绝」「允许一次」名称与先后相同", async (_name, id, open) => {
    await open(id);
    const allow = await screen.findByRole("button", { name: "允许一次" });
    const card = allow.closest(".chat-approval")!;
    expect(card.getAttribute("aria-label")).toBe("Agent 请求权限");
    expect(card.querySelector(".chat-approval-badge")?.textContent).toMatch(/^待批准/);
    const names = Array.from(card.querySelectorAll(".chat-approval-actions > button")).map((button) => button.childNodes[0]?.textContent?.trim());
    expect(names.slice(-2)).toEqual(["拒绝", "允许一次"]);
    // 整个对话窗里只有这一张审批卡（没有第二套实现同时渲染）。
    expect(document.querySelectorAll(".chat-approval-overlay .chat-approval")).toHaveLength(1);
  });

  it.each(CARDS)("%s：数字键 1 = 允许一次（焦点在输入框时数字属于输入，不放行）", async (name, id, open) => {
    await open(id);
    await screen.findByRole("button", { name: "允许一次" });
    const input = adapter.box();
    input.focus();
    fireEvent.keyDown(input, { key: "1" });
    expect(delivered(id)).toEqual([]);
    input.blur();
    fireEvent.keyDown(window, { key: "1" });
    await waitFor(() => expect(delivered(id)).toEqual([DELIVERY[name].allow]));
  });

  it.each(CARDS)("%s：Esc 两段式拒绝，第一下只点亮，第二下才拒", async (name, id, open) => {
    await open(id);
    const deny = await screen.findByRole("button", { name: "拒绝" });
    adapter.box().blur();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(deny.className).toContain("is-esc-armed");
    expect(deny.textContent).toContain("再按 Esc 确认拒绝");
    expect(delivered(id)).toEqual([]);
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(delivered(id)).toEqual([DELIVERY[name].deny]));
  });

  it.each(CARDS)("%s：空输入框里的 Esc 同样是两段式拒绝", async (name, id, open) => {
    await open(id);
    await screen.findByRole("button", { name: "拒绝" });
    const input = adapter.box();
    input.focus();
    fireEvent.keyDown(input, { key: "Escape" });
    expect(delivered(id)).toEqual([]);
    fireEvent.keyDown(input, { key: "Escape" });
    await waitFor(() => expect(delivered(id)).toEqual([DELIVERY[name].deny]));
  });

  it.each(CARDS)("%s：数字键 2 = 第一个持久放行；没有持久放行的家按了不生效", async (name, id, open) => {
    await open(id);
    await screen.findByRole("button", { name: "允许一次" });
    adapter.box().blur();
    fireEvent.keyDown(window, { key: "2" });
    const expected: Record<string, unknown[]> = {
      "claude（broker）": [["respond_runtime_request", "suggestion:0"]],
      "codex（broker）": [],
      "kimi（屏幕）": [["write_managed_terminal", "2"]],
      "gemini（屏幕）": [["write_managed_terminal", "\x1b[B\r"]],
      "opencode（屏幕）": [["write_managed_terminal", "\x1b[C\r"]],
      "claude（屏幕兜底）": [["write_managed_terminal", "\x1b[B\r"]],
    };
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 30)); });
    expect(delivered(id)).toEqual(expected[name]);
  });
});

describe("有卡片待处理时的输入框：五家同一条规则", () => {
  /// 走查 输入 B2：此前屏幕通道整块 inert（不能打字），broker 通道不锁、发送后才报错。
  it.each(CARDS)("%s：输入框照常可写，发送先被拦住并说明，不往终端写一个字", async (_name, id, open) => {
    await open(id);
    await screen.findByRole("button", { name: "允许一次" });
    const input = adapter.box();
    // 输入框不禁用、不 inert，占位先把原因说了。
    expect(input.disabled).toBe(false);
    expect(input.closest(".chat-compose")?.hasAttribute("inert")).toBe(false);
    expect(input.placeholder).toBe(zh.chat.cardLockApproval);
    fireEvent.change(input, { target: { value: "先别管审批，接着做" } });
    expect(input.value).toBe("先别管审批，接着做");
    // 发送键事先置灰并带同一句提示。
    const send = screen.getByRole("button", { name: "发送" });
    expect(send.getAttribute("aria-disabled")).toBe("true");
    expect(send.getAttribute("data-tip")).toBe(zh.chat.cardLockApproval);
    // 按回车：不发，草稿原样留着，提示条上是同一句。
    fireEvent.keyDown(input, { key: "Enter" });
    await screen.findByText(zh.chat.cardLockApproval);
    expect(adapter.writes(id)).toEqual([]);
    expect(input.value).toBe("先别管审批，接着做");
  });

  it("没有卡片时输入框照旧：占位不是锁定说明，发送键不因卡片置灰", async () => {
    setup(910, "kimi");
    render(<ChatWindow />);
    await screen.findByText("适配-910");
    const input = adapter.box();
    expect(input.placeholder).not.toBe(zh.chat.cardLockApproval);
    fireEvent.change(input, { target: { value: "你好" } });
    expect(screen.getByRole("button", { name: "发送" }).getAttribute("aria-disabled")).toBeNull();
  });

  it("屏幕卡被仅收起后：输入框仍可写，发送仍被拦（终端里的框还在等按键）", async () => {
    await CARDS[2][2](911);
    fireEvent.click(await screen.findByRole("button", { name: "仅收起" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "允许一次" })).toBeNull());
    const input = adapter.box();
    expect(input.disabled).toBe(false);
    expect(input.placeholder).toBe(zh.chat.cardLockApproval);
    fireEvent.change(input, { target: { value: "不能替我回答审批" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await screen.findByText(zh.chat.cardLockApproval);
    expect(adapter.writes(911)).toEqual([]);
  });

  it("手机页同一条规则：屏幕卡在场时输入框可写，发送被拦并说明", async () => {
    markRemoteUi();
    try {
      await CARDS[4][2](912);
      await screen.findByRole("button", { name: "允许一次" });
      const input = adapter.box();
      expect(input.disabled).toBe(false);
      expect(input.placeholder).toBe(zh.chat.cardLockApproval);
      fireEvent.change(input, { target: { value: "手机上也不能误发" } });
      fireEvent.keyDown(input, { key: "Enter" });
      await screen.findByText(zh.chat.cardLockApproval);
      expect(adapter.writes(912)).toEqual([]);
    } finally {
      (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = undefined;
      document.body.classList.remove("remote-ui");
    }
  });
});

describe("OpenCode 的持久放行：点一下，确认页由 Meowo 认出后代按", () => {
  it("点「本会话内允许」写「→ 回车」；认出 Always allow 确认页后再写回车，不出第二张卡", async () => {
    await CARDS[4][2](920);
    fireEvent.click(await screen.findByRole("button", { name: "本会话内允许" }));
    await waitFor(() => expect(adapter.writes(920)).toEqual(["\x1b[C\r"]));
    await showScreen("opencode", OPENCODE_ALWAYS_CONFIRM_SCREEN);
    await waitFor(() => expect(adapter.writes(920)).toEqual(["\x1b[C\r", "\r"]));
    // 确认页没有变成卡片。
    expect(screen.queryByRole("button", { name: "Confirm" })).toBeNull();
    expect(document.querySelectorAll(".chat-approval-overlay .chat-approval")).toHaveLength(0);
  });

  it("不是在 Meowo 里点出来的确认页（用户自己在终端里选的）：照常出卡，不替他按", async () => {
    setup(921, "opencode");
    render(<ChatWindow />);
    await showScreen("opencode", OPENCODE_ALWAYS_CONFIRM_SCREEN);
    const confirm = await screen.findByRole("button", { name: "Confirm" });
    const card = confirm.closest(".chat-approval")!;
    expect(card.getAttribute("aria-label")).toBe("终端正在等待你的选择");
    expect(card.querySelector(".chat-approval-badge")?.textContent).toBe("待选择");
    expect(card.querySelector("pre")?.textContent).toContain("until OpenCode is restarted");
    expect(adapter.writes(921)).toEqual([]);
    fireEvent.click(confirm);
    await waitFor(() => expect(adapter.writes(921)).toEqual(["\r"]));
  });

  it("点的是「允许一次」：之后出现的确认页不代按", async () => {
    await CARDS[4][2](922);
    fireEvent.click(await screen.findByRole("button", { name: "允许一次" }));
    await waitFor(() => expect(adapter.writes(922)).toEqual(["\r"]));
    await showScreen("opencode", OPENCODE_ALWAYS_CONFIRM_SCREEN);
    await screen.findByRole("button", { name: "Confirm" });
    expect(adapter.writes(922)).toEqual(["\r"]);
  });
});

describe("Kimi 提问表单（屏幕通道）", () => {
  const RULE = " " + "─".repeat(60);
  const form = (...body: string[]) => [RULE, "  question", ...body, RULE, " Ask When Needed  K3 thinking: high"].join("\n");
  const MULTI = form(
    "  (✓) 颜色   水果    Submit",
    "  ? 喜欢哪些水果？",
    "   [ ] 苹果",
    "   [✓] 香蕉",
    "   [ ] 橙子",
    "   [ ] Other",
    "   ↑↓ select  1-4 / ↵ toggle  ←/→/tab switch  esc cancel",
  );
  const SINGLE = form(
    "   Q1    Submit",
    "  ? 喜欢哪种颜色？",
    "   → [1] 红色",
    "     [2] 绿色",
    "     [3] Other",
    "   ↑↓ select  1-3 / ↵ choose  ←/→/tab switch  esc cancel",
  );

  /// 走查 输入 A3、A6：此前落在通用提示卡上，徽章写「待批准」，多选每点一项卡片就收起。
  it("多选：是提问卡（徽章「待回答」），连续勾选卡片不收起，勾选状态就地翻转", async () => {
    setup(930, "kimi");
    render(<ChatWindow />);
    await showScreen("kimi", MULTI);
    const apple = await screen.findByRole("button", { name: "苹果" });
    const card = apple.closest(".chat-approval")!;
    expect(card.getAttribute("aria-label")).toBe("Agent 正在等待你的回答");
    expect(card.querySelector(".chat-approval-badge")?.textContent).toBe("待回答");
    fireEvent.click(apple);
    await waitFor(() => expect(adapter.writes(930)).toEqual(["1"]));
    // 卡片还在，同一个节点；「苹果」已就地标成勾选。
    expect(screen.getByRole("button", { name: /苹果/ }).closest(".chat-approval")).toBe(card);
    await waitFor(() => expect(screen.getByRole("button", { name: /苹果/ }).className).toContain("is-selected"));
    fireEvent.click(screen.getByRole("button", { name: /橙子/ }));
    await waitFor(() => expect(adapter.writes(930)).toEqual(["1", "3"]));
    expect(document.querySelectorAll(".chat-approval-overlay .chat-approval")).toHaveLength(1);
    // 去下一页：Tab。
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    await waitFor(() => expect(adapter.writes(930)).toEqual(["1", "3", "\t"]));
  });

  it("单选：点一项就是作答，卡片随即收起", async () => {
    setup(931, "kimi");
    render(<ChatWindow />);
    await showScreen("kimi", SINGLE);
    fireEvent.click(await screen.findByRole("button", { name: "绿色" }));
    await waitFor(() => expect(adapter.writes(931)).toEqual(["2"]));
    await waitFor(() => expect(screen.queryByRole("button", { name: "绿色" })).toBeNull());
  });
});
