// Claude 启动阶段的全屏确认页在对话页出选择卡（验收 27）；进程换代后上一个进程留下的提示条清掉。
// 屏幕样本是真机抓的（test/fixtures/startup-frames.json，隔离配置目录，见文件里的 note）。
import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { adapter, eventListeners, invoke, terminalProps } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { en } from "../i18n/en";
import { zh } from "../i18n/zh";
import { markRemoteUi } from "../remoteMode";
import { terminalAttention, type TerminalAttention } from "../terminalAttention";
import { grammar } from "../test/agents";
import fixture from "../test/fixtures/startup-frames.json";
import { CLAUDE_BROKER_APPROVAL, KIMI_APPROVAL_SCREEN } from "../test/requestSamples";
import { TerminalPromptCard } from "./chat/TerminalRequestCards";

const FRAMES = fixture.frames as Record<string, { provider: string; rule: string | null; screen: string[] }>;
const recognize = (name: string, startupPrompts = true): TerminalAttention | null =>
  terminalAttention(FRAMES[name].screen.join("\n"), [], false, false, grammar("claude"), startupPrompts);

function setup(sessionId: number, provider: string, approval: unknown = null) {
  const base = adapter.setup(sessionId, provider, "running");
  invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
    if (command === "get_chat_history") return Promise.resolve({ ...adapter.history(sessionId, provider, "running"), ptyManaged: true, pendingReview: approval ? "approval" : null });
    if (command === "pending_interaction") return Promise.resolve({ approval: approval ? { ...(approval as object), sessionId } : null, question: null });
    return base(command, args);
  });
}
async function show(attention: TerminalAttention | null) {
  await waitFor(() => expect(terminalProps.current?.onAttention).toBeTypeOf("function"));
  act(() => (terminalProps.current!.onAttention as (value: unknown) => void)(attention));
}
const writes = (id: number) => invoke.mock.calls.flatMap(([command, args]) => (command === "write_managed_terminal" && args?.sessionId === id ? [args.data] : []));
const restoreDesktop = () => {
  (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = undefined;
  document.body.classList.remove("remote-ui");
};

describe("Bypass Permissions 确认页：认出来，出选择卡，不代按", () => {
  it("真机屏幕被认成「等你选择」，两个选项照原文，No, exit 的角色是 exit", () => {
    const attention = recognize("bypass-permissions");
    expect(attention?.id).toBe(FRAMES["bypass-permissions"].rule);
    expect(attention?.kind).toBe("choice");
    expect(attention?.notice).toBe("bypass_permissions");
    expect(attention?.options?.map((option) => [option.label, option.role])).toEqual([["No, exit", "exit"], ["Yes, I accept", "other"]]);
    // 会话跑起来之后（稳态扫描不认通用启动提示）照样认：它是插件规则，不是启动阶段才有的通用提示。
    expect(recognize("bypass-permissions", false)?.id).toBe(FRAMES["bypass-permissions"].rule);
  });

  it("按键从焦点算：焦点在 No 时接受要先下移再回车；焦点已在 Yes 时直接回车", () => {
    const keys = (name: string) => Object.fromEntries((recognize(name)?.options ?? []).map((option) => [option.label, option.input]));
    // 真机：页面刚出现，焦点在 No, exit。
    expect(keys("bypass-permissions")).toEqual({ "No, exit": "\r", "Yes, I accept": "\x1b[B\r" });
    // 真机：按过一次下键之后，焦点在 Yes, I accept。
    expect(keys("bypass-permissions-moved")).toEqual({ "No, exit": "\x1b[A\r", "Yes, I accept": "\r" });
  });

  it("正文里提到这句警告（不是整行）不出卡", () => {
    expect(terminalAttention("● 启动时会显示 WARNING: Claude Code running in Bypass Permissions mode 这一页", [], false, false, grammar("claude"), false)).toBeNull();
  });

  it.each([["桌面", false], ["手机页", true]] as const)("卡片（%s）：一句人话说明、原文可见、退出项写明后果且不标推荐", (_name, remote) => {
    if (remote) markRemoteUi();
    try {
      render(<TerminalPromptCard attention={recognize("bypass-permissions")!} live returnFocusTo={{ current: null }} onChoose={() => {}} onKey={() => {}} onCollapse={() => {}} />);
      const card = screen.getByRole("alertdialog");
      expect(card.getAttribute("aria-label")).toBe("终端正在等待你的选择");
      expect(card.querySelector(".chat-approval-badge")?.textContent).toBe("待选择");
      expect(screen.getByText(zh.chat.promptNotice.bypass_permissions)).toBeTruthy();
      // 原文在卡内：警告与那句责任声明都看得到。
      const original = card.querySelector("pre")?.textContent ?? "";
      expect(original).toContain("WARNING: Claude Code running in Bypass Permissions mode");
      expect(original).toContain("you accept all responsibility");
      const [exit, accept] = Array.from(card.querySelectorAll<HTMLButtonElement>(".chat-approval-options button"));
      expect(exit.textContent).toBe(`No, exit${zh.chat.optionExitsSession}`);
      expect(accept.textContent).toBe("Yes, I accept");
      // 默认焦点在退出项上，但卡片不把「退出」标成推荐；也不替用户把「接受」标成推荐。
      expect(exit.className).toBe("");
      expect(accept.className).toBe("");
      // 没有分不清选的是哪一项的「确认 / 取消」。
      expect(screen.queryByRole("button", { name: "确认" })).toBeNull();
    } finally {
      restoreDesktop();
    }
  });

  it("文案不超过 20 字、不用破折号和分号，中英文都有", () => {
    for (const text of [...Object.values(zh.chat.promptNotice), zh.chat.optionExitsSession]) {
      expect(text.length).toBeLessThanOrEqual(20);
      expect(text).not.toMatch(/[—;；]|--/);
    }
    for (const key of Object.keys(zh.chat.promptNotice) as (keyof typeof zh.chat.promptNotice)[]) expect(en.chat.promptNotice[key]).toBeTruthy();
    expect(en.chat.optionExitsSession).toBeTruthy();
  });

  it.each([["桌面", 1101, false], ["手机页", 1102, true]] as const)("对话窗（%s）：出卡，Meowo 不代按；发送被拦并说明；点接受才写下移加回车", async (_name, id, remote) => {
    if (remote) markRemoteUi();
    try {
      setup(id, "claude");
      render(<ChatWindow />);
      await show(recognize("bypass-permissions"));
      const accept = await screen.findByRole("button", { name: "Yes, I accept" });
      // 认出这一页不等于替用户接受：没有任何按键写进终端。
      await act(async () => { await new Promise((resolve) => setTimeout(resolve, 60)); });
      expect(writes(id)).toEqual([]);
      // 输入框照常可写，发送先拦住，三处是同一句说明（卡片一路定的统一规则）。
      const input = adapter.box();
      expect(input.disabled).toBe(false);
      expect(input.placeholder).toBe(zh.chat.cardLockPrompt);
      fireEvent.change(input, { target: { value: "继续" } });
      fireEvent.keyDown(input, { key: "Enter" });
      await screen.findByText(zh.chat.cardLockPrompt);
      expect(writes(id)).toEqual([]);
      fireEvent.click(accept);
      await waitFor(() => expect(writes(id)).toEqual(["\x1b[B\r"]));
    } finally {
      restoreDesktop();
    }
  });

  it("对话窗：焦点已在 Yes 上（用户在终端里挪过）点接受只写回车；点退出写的是退出项的键", async () => {
    setup(1103, "claude");
    render(<ChatWindow />);
    await show(recognize("bypass-permissions-moved"));
    fireEvent.click(await screen.findByRole("button", { name: "Yes, I accept" }));
    await waitFor(() => expect(writes(1103)).toEqual(["\r"]));
  });

  it("对话窗：点「No, exit」写退出项的键（焦点就在它上面，回车）", async () => {
    setup(1104, "claude");
    render(<ChatWindow />);
    await show(recognize("bypass-permissions"));
    fireEvent.click(await screen.findByRole("button", { name: /^No, exit/ }));
    await waitFor(() => expect(writes(1104)).toEqual(["\r"]));
  });
});

describe("同类启动确认页", () => {
  it("自定义 API 密钥确认页：真机屏幕认成选择卡，两项照原文，按键从焦点算", () => {
    const attention = recognize("custom-api-key");
    expect(attention?.id).toBe(FRAMES["custom-api-key"].rule);
    expect(attention?.kind).toBe("choice");
    expect(attention?.notice).toBe("custom_api_key");
    // 焦点在 No (recommended)。
    expect(attention?.options?.map((option) => [option.label, option.input, option.role])).toEqual([
      ["Yes", "\x1b[A\r", "other"],
      ["No (recommended)", "\r", "other"],
    ]);
  });

  it("首次运行的主题选择页：现在不认（清单见 progress-fix-cards.md），不会被误认成别的卡", () => {
    expect(recognize("first-run-theme")).toBeNull();
  });
});

/// 用户实拍 2026-11-15：切账号把会话重启到新账号后，输入框下方还挂着上一个进程的
/// 「该审批已在别处处理或已过期」。原因：只有终端视图听 pty-restarted，对话页不知道进程换了代。
describe("进程换代后，上一个进程留下的提示条清掉", () => {
  /// 挂一张 broker 审批，再让后端报它没了（旧进程被杀）：提示条出现。
  async function leaveEndedNotice(id: number, provider: string) {
    let pending: unknown = CLAUDE_BROKER_APPROVAL;
    const base = adapter.setup(id, provider, "running");
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve({ ...adapter.history(id, provider, "running"), ptyManaged: true, pendingReview: pending ? "approval" : null });
      if (command === "pending_interaction") return Promise.resolve({ approval: pending ? { ...(pending as object), sessionId: id, provider } : null, question: null });
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByRole("button", { name: "允许一次" });
    pending = null;
    await screen.findByText(zh.chat.approvalClearedNotice, undefined, { timeout: 4000 });
  }
  const restart = (sessionId: number) => act(() => { eventListeners.get("pty-restarted")?.({ payload: { sessionId } }); });

  it.each([
    ["claude 切账号重启", 1111, "claude"],
    ["kimi 重启续接切模型", 1112, "kimi"],
    ["gemini 重启续接切模型", 1113, "gemini"],
  ] as const)("%s：pty-restarted 之后提示条消失", async (_name, id, provider) => {
    await leaveEndedNotice(id, provider);
    restart(id);
    await waitFor(() => expect(screen.queryByText(zh.chat.approvalClearedNotice)).toBeNull());
  });

  it("别的会话重启不影响这个会话的提示条", async () => {
    await leaveEndedNotice(1114, "claude");
    restart(9999);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 60)); });
    expect(screen.getByText(zh.chat.approvalClearedNotice)).toBeTruthy();
  });

  it("新卡到来时同样清掉（手机页收不到宿主事件，靠这一条）", async () => {
    markRemoteUi();
    try {
      await leaveEndedNotice(1115, "claude");
      await show(recognize("bypass-permissions"));
      await screen.findByRole("button", { name: "Yes, I accept" });
      expect(screen.queryByText(zh.chat.approvalClearedNotice)).toBeNull();
    } finally {
      restoreDesktop();
    }
  });

  it("只清这几句说明：进程重启不把别的错误（发送被拦的说明）一起清掉", async () => {
    setup(1116, "kimi");
    render(<ChatWindow />);
    await show(terminalAttention(KIMI_APPROVAL_SCREEN, [], false, false, grammar("kimi")));
    await screen.findByRole("button", { name: "允许一次" });
    const input = adapter.box();
    fireEvent.change(input, { target: { value: "x" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await screen.findByText(zh.chat.cardLockApproval);
    restart(1116);
    // 旧进程屏幕上认出的卡随进程一起作废。
    await waitFor(() => expect(screen.queryByRole("button", { name: "允许一次" })).toBeNull());
  });

  it("旧进程屏幕上的卡被收起后留下的收起条，进程重启后也作废", async () => {
    setup(1117, "kimi");
    render(<ChatWindow />);
    await show(terminalAttention(KIMI_APPROVAL_SCREEN, [], false, false, grammar("kimi")));
    fireEvent.click(await screen.findByRole("button", { name: "仅收起" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "允许一次" })).toBeNull());
    const before = adapter.box().placeholder;
    expect(before).toBe(zh.chat.cardLockApproval);
    restart(1117);
    await waitFor(() => expect(adapter.box().placeholder).not.toBe(zh.chat.cardLockApproval));
  });
});
