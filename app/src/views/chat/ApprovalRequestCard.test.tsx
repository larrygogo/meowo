import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { RuntimeRequest } from "../../api";
import { zh } from "../../i18n/zh";
import { markRemoteUi } from "../../remoteMode";
import { terminalAttention } from "../../terminalAttention";
import { grammar } from "../../test/agents";
import {
  CLAUDE_APPROVAL_SCREEN,
  CLAUDE_BROKER_APPROVAL,
  CODEX_BROKER_APPROVAL,
  GEMINI_APPROVAL_SCREEN,
  KIMI_APPROVAL_SCREEN,
  OPENCODE_PERMISSION_SCREEN,
} from "../../test/requestSamples";
import { ApprovalRequestCard } from "./ApprovalRequestCard";
import { approvalCardModel } from "./requestCard";
import { asTerminalRuntimeRequest } from "./runtimeRequest";

afterEach(() => {
  cleanup();
  // 远程标志挂在 globalThis，不复位会把后面的桌面用例污染成远程态。
  (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = undefined;
  document.body.classList.remove("remote-ui");
});

function screenRequest(provider: string, screen: string, writable = true): RuntimeRequest {
  const attention = terminalAttention(screen, [], false, false, grammar(provider));
  if (!attention) throw new Error(`${provider} 的样本没认出来`);
  return asTerminalRuntimeRequest({ sessionId: 7, provider, attention, writable });
}

/// 五家各一份样本：claude、codex 走 broker，kimi、gemini、opencode 走屏幕；claude 另有一份屏幕样本
/// （broker 接不上时的兜底通道）。
const FIVE: [string, RuntimeRequest][] = [
  ["claude（broker）", CLAUDE_BROKER_APPROVAL],
  ["codex（broker）", CODEX_BROKER_APPROVAL],
  ["kimi（屏幕）", screenRequest("kimi", KIMI_APPROVAL_SCREEN)],
  ["gemini（屏幕）", screenRequest("gemini", GEMINI_APPROVAL_SCREEN)],
  ["opencode（屏幕）", screenRequest("opencode", OPENCODE_PERMISSION_SCREEN)],
  ["claude（屏幕兜底）", screenRequest("claude", CLAUDE_APPROVAL_SCREEN)],
];

const base = { countdown: null, resolving: false, error: null, escArmed: false, dirs: [], returnFocusTo: { current: null } };

function Card({ request, timedOut = false, ...rest }: { request: RuntimeRequest; timedOut?: boolean } & Partial<Parameters<typeof ApprovalRequestCard>[0]>) {
  const model = approvalCardModel(request, zh, timedOut);
  return <ApprovalRequestCard key={model.requestId} {...base} model={model} onDecide={() => {}} onCollapse={() => {}} {...rest} />;
}

/// 卡片底部按钮的文字，按读屏与 Tab 的顺序（DOM 序）。去掉键盘徽标。
function actionNames(): string[] {
  return Array.from(document.querySelectorAll<HTMLButtonElement>(".chat-approval-actions button")).map((button) => {
    const clone = button.cloneNode(true) as HTMLElement;
    clone.querySelectorAll("kbd").forEach((kbd) => kbd.remove());
    return clone.textContent?.trim() ?? "";
  });
}

describe("审批卡：五家同一张卡", () => {
  it.each(FIVE)("%s：标题、徽章、「拒绝」「允许一次」的名称与先后相同", (_name, request) => {
    render(<Card request={request} />);
    const card = screen.getByRole("alertdialog");
    expect(card.getAttribute("aria-label")).toBe("Agent 请求权限");
    expect(card.querySelector(".chat-approval-badge")?.textContent).toBe("待批准");
    const names = actionNames();
    // 本次决定的两颗钮恒在最后，拒绝在前、允许一次在后。
    expect(names.slice(-2)).toEqual(["拒绝", "允许一次"]);
    // 按钮名不再是「确认 / 取消」。
    expect(names).not.toContain("确认");
    expect(names).not.toContain("取消 / 返回");
  });

  it("五家的完整按钮集合与顺序：多出的按钮只来自能力，不来自通道", () => {
    const sets = FIVE.map(([name, request]) => {
      render(<Card request={request} />);
      const names = actionNames();
      cleanup();
      return [name, names];
    });
    expect(sets).toEqual([
      // broker：理由能送回 Agent，所以有「说明并拒绝」；收起等于让 Agent 干等，所以没有「仅收起」。
      ["claude（broker）", ["允许并记住（此项目、本机） · cargo test", "说明并拒绝", "拒绝", "允许一次"]],
      ["codex（broker）", ["说明并拒绝", "拒绝", "允许一次"]],
      // 屏幕：识别可能认错，所以有「仅收起」；持久放行按终端里实际有的选项出。
      ["kimi（屏幕）", ["仅收起", "本会话内允许", "拒绝", "允许一次"]],
      ["gemini（屏幕）", ["仅收起", "本会话内允许", "拒绝", "允许一次"]],
      ["opencode（屏幕）", ["仅收起", "本会话内允许", "拒绝", "允许一次"]],
      ["claude（屏幕兜底）", ["仅收起", "允许并记住 · curl *", "允许并切到自动模式", "拒绝", "允许一次"]],
    ]);
  });

  it.each(FIVE)("%s：键盘徽标相同，拒绝是 Esc、允许一次是 1、持久放行从 2 起", async (_name, request) => {
    render(<Card request={request} />);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
    expect(screen.getByRole("button", { name: "拒绝" }).querySelector("kbd")?.textContent).toBe("Esc");
    expect(screen.getByRole("button", { name: "允许一次" }).querySelector("kbd")?.textContent).toBe("1");
    const persistent = Array.from(document.querySelectorAll(".chat-approval-actions button.is-persistent"));
    expect(persistent.map((button) => button.querySelector("kbd")?.textContent)).toEqual(persistent.map((_, index) => String(index + 2)));
  });

  it.each(FIVE)("%s：点按钮发出的决定是同一套词", (_name, request) => {
    const onDecide = vi.fn();
    render(<Card request={request} onDecide={onDecide} />);
    fireEvent.click(screen.getByRole("button", { name: "允许一次" }));
    fireEvent.click(screen.getByRole("button", { name: "拒绝" }));
    const persistent = document.querySelector<HTMLButtonElement>(".chat-approval-actions button.is-persistent");
    if (persistent) fireEvent.click(persistent);
    expect(onDecide.mock.calls.map(([choice]) => choice)).toEqual(persistent ? ["allow_once", "deny", "suggestion:0"] : ["allow_once", "deny"]);
  });

  it.each(FIVE)("%s（手机页）：同一张卡，同样的按钮，不挂键盘徽标", async (_name, request) => {
    render(<Card request={request} />);
    const desktop = actionNames();
    cleanup();
    markRemoteUi();
    render(<Card request={request} />);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
    expect(actionNames()).toEqual(desktop);
    expect(document.querySelector(".chat-approval kbd")).toBeNull();
  });

  it("屏幕通道的卡可以仅收起；broker 的卡没有这颗钮", () => {
    const onCollapse = vi.fn();
    const onDecide = vi.fn();
    render(<Card request={screenRequest("kimi", KIMI_APPROVAL_SCREEN)} onCollapse={onCollapse} onDecide={onDecide} />);
    fireEvent.click(screen.getByRole("button", { name: "仅收起" }));
    expect(onCollapse).toHaveBeenCalledTimes(1);
    expect(onDecide).not.toHaveBeenCalled();
    cleanup();
    render(<Card request={CLAUDE_BROKER_APPROVAL} onCollapse={onCollapse} />);
    expect(screen.queryByRole("button", { name: "仅收起" })).toBeNull();
  });

  it("详情标签：认出是命令写「命令」，否则写「完整参数」，两条通道同一条规则", () => {
    render(<Card request={CLAUDE_BROKER_APPROVAL} />);
    expect(screen.getByText("命令")).toBeTruthy();
    cleanup();
    render(<Card request={screenRequest("gemini", GEMINI_APPROVAL_SCREEN)} />);
    expect(screen.getByText("命令")).toBeTruthy();
    expect(screen.queryByText("完整参数")).toBeNull();
    cleanup();
    render(<Card request={screenRequest("opencode", OPENCODE_PERMISSION_SCREEN)} />);
    expect(screen.getByText("完整参数")).toBeTruthy();
    cleanup();
    render(<Card request={{ ...CODEX_BROKER_APPROVAL, toolName: "apply_patch", input: JSON.stringify({ path: "a.ts", patch: "…" }) }} />);
    expect(screen.getByText("完整参数")).toBeTruthy();
  });

  it("破坏性命令：两条通道的「允许一次」都换成警示色", () => {
    render(<Card request={{ ...CODEX_BROKER_APPROVAL, input: JSON.stringify({ command: "rm -rf build" }) }} />);
    expect(screen.getByRole("button", { name: /允许一次/ }).className).toContain("is-danger");
    cleanup();
    render(<Card request={screenRequest("kimi", KIMI_APPROVAL_SCREEN.replace("echo meowo-approval-probe-1234", "rm -rf build"))} />);
    expect(screen.getByRole("button", { name: /允许一次/ }).className).toContain("is-danger");
  });
});

describe("审批卡状态：过期、已取消、外部终端", () => {
  it.each([
    ["过期（broker 倒计时归零）", { request: CLAUDE_BROKER_APPROVAL, timedOut: true }, "已过期"],
    ["过期（后端标记）", { request: { ...CODEX_BROKER_APPROVAL, status: "expired" as const } }, "已过期"],
    ["已取消（broker）", { request: { ...CLAUDE_BROKER_APPROVAL, status: "cancelled" as const } }, "已取消"],
    ["已取消（屏幕）", { request: { ...screenRequest("kimi", KIMI_APPROVAL_SCREEN), status: "cancelled" as const } }, "已取消"],
  ])("%s：徽章换成一个词，按钮点了不发", (_name, props, badge) => {
    const onDecide = vi.fn();
    render(<Card {...props} onDecide={onDecide} countdown="0:00" />);
    expect(document.querySelector(".chat-approval-badge")?.textContent).toBe(badge);
    for (const button of Array.from(document.querySelectorAll<HTMLButtonElement>(".chat-approval-actions button:not(.chat-attention-dismiss)"))) {
      // aria-disabled 而不是 disabled：正聚焦的按钮不被踢掉焦点。
      expect(button.disabled).toBe(false);
      expect(button.getAttribute("aria-disabled")).toBe("true");
      fireEvent.click(button);
    }
    expect(onDecide).not.toHaveBeenCalled();
    // 不可作答时不挂键盘徽标：按了不生效的键不该写在按钮上。
    expect(document.querySelector(".chat-approval kbd")).toBeNull();
  });

  it("待处理时徽章带剩余时间（broker 才有时限）", () => {
    render(<Card request={CLAUDE_BROKER_APPROVAL} countdown="4:32" />);
    expect(document.querySelector(".chat-approval-badge")?.textContent).toBe("待批准 · 4:32");
  });

  it("外部终端里跑的会话：屏幕卡照常显示内容，按钮不可用并说明原因", () => {
    const onDecide = vi.fn();
    render(<Card request={screenRequest("gemini", GEMINI_APPROVAL_SCREEN, false)} onDecide={onDecide} />);
    expect(screen.getByText(zh.chat.questionExternalHint)).toBeTruthy();
    expect(screen.getByText("echo meowo-probe > probe-out.txt")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "允许一次" }));
    expect(onDecide).not.toHaveBeenCalled();
  });
});

/// 以下几条原在 BrokerApprovalCard.test.tsx，卡片合一后随卡片搬来，断言不变。
describe("审批卡：说明并拒绝、在途、焦点（broker 通道的能力）", () => {
  const approval = (requestId: string): RuntimeRequest => ({
    requestId, sessionId: 7, provider: "claude", kind: "approval", status: "pending",
    toolName: "Bash", input: "ls", description: null, permissionSuggestions: [],
    responseCapability: { type: "broker_live" },
  });

  it("Bash 参数只摆命令本身，复制也复制命令", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
    const input = JSON.stringify({ command: "cargo test", description: "运行测试" });
    render(<Card request={{ ...approval("r1"), input }} />);
    expect(screen.getByText("命令")).toBeTruthy();
    expect(screen.getByText("cargo test")).toBeTruthy();
    expect(screen.queryByText(input)).toBeNull();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "复制命令" })); });
    expect(writeText).toHaveBeenCalledWith("cargo test");
  });

  it("「说明并拒绝」把理由带进拒绝选择", () => {
    const onDecide = vi.fn();
    render(<Card request={approval("r1")} onDecide={onDecide} />);
    fireEvent.click(screen.getByRole("button", { name: "说明并拒绝" }));
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "先别动生产库" } });
    fireEvent.submit(screen.getByRole("textbox").closest("form")!);
    expect(onDecide).toHaveBeenCalledWith("deny:先别动生产库");
  });

  /// 按 requestId 挂载：换请求即新卡，上一张卡写了一半的理由不能串过来。
  it("换请求后理由表单归零", () => {
    const { rerender } = render(<Card request={approval("r1")} />);
    fireEvent.click(screen.getByRole("button", { name: "说明并拒绝" }));
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "半句" } });
    rerender(<Card request={approval("r2")} />);
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  /// 走查 C-3（p1-broker-fail：结算失败后 activeElement=BODY）：结算在途用 aria-disabled，焦点留在刚按的
  /// 按钮上；在途的点击不发出去。兄弟场景：持久放行、「说明并拒绝」、拒绝、理由提交钮同一口径。
  it("结算在途按钮不失焦、点了不发；失败后可原地重按", () => {
    const onDecide = vi.fn();
    const suggestions = [{ type: "addRules", destination: "session", rules: [{ toolName: "Bash", ruleContent: "ls" }] }];
    const card = (resolving: boolean, error: string | null) => (
      <Card request={{ ...approval("r1"), permissionSuggestions: suggestions }} resolving={resolving} error={error} onDecide={onDecide} />
    );
    const { rerender } = render(card(false, null));
    const allow = screen.getByRole("button", { name: "允许一次" }) as HTMLButtonElement;
    allow.focus();
    fireEvent.click(allow);
    expect(onDecide).toHaveBeenCalledWith("allow_once");
    rerender(card(true, null));
    for (const button of screen.getAllByRole("button").filter((b) => b.closest(".chat-approval-actions"))) {
      expect((button as HTMLButtonElement).disabled).toBe(false);
      expect(button.getAttribute("aria-disabled")).toBe("true");
    }
    expect(document.activeElement).toBe(allow);
    fireEvent.click(allow);
    fireEvent.click(screen.getByRole("button", { name: "拒绝" }));
    expect(onDecide).toHaveBeenCalledTimes(1);
    rerender(card(false, "已过期"));
    expect(screen.getByRole("alert").textContent).toBe("已过期");
    expect(document.activeElement).toBe(allow);
    fireEvent.click(allow);
    expect(onDecide).toHaveBeenCalledTimes(2);
  });

  /// 走查 C-7（p3-reason-empty 截获 deny: 空理由）：理由空着回车不算数，提交钮不可用。
  it("「说明并拒绝」理由为空不提交，只有空白也不算", () => {
    const onDecide = vi.fn();
    render(<Card request={approval("r1")} onDecide={onDecide} />);
    fireEvent.click(screen.getByRole("button", { name: "说明并拒绝" }));
    const form = screen.getByRole("textbox").closest("form")!;
    expect((screen.getByRole("button", { name: "提交并拒绝" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.submit(form);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "   " } });
    fireEvent.submit(form);
    expect(onDecide).not.toHaveBeenCalled();
    fireEvent.change(screen.getByRole("textbox"), { target: { value: " 先别跑 " } });
    expect((screen.getByRole("button", { name: "提交并拒绝" }) as HTMLButtonElement).disabled).toBe(false);
    fireEvent.submit(form);
    expect(onDecide).toHaveBeenCalledWith("deny:先别跑");
  });

  it("屏幕通道没有「说明并拒绝」：理由送不回 Agent 就不出这颗钮", () => {
    render(<Card request={screenRequest("kimi", KIMI_APPROVAL_SCREEN)} />);
    expect(screen.queryByRole("button", { name: "说明并拒绝" })).toBeNull();
  });

  /// 走查 C-5（p2-digit-in-ta：默认焦点在输入框，按「1」只把数字打进输入框）：数字徽标只在按下去真生效时
  /// 显示，判据与窗口级数字键监听同一份（digitKeysOwnedElsewhere）。Esc 徽标照常常驻（空输入框里也生效）。
  it.each([
    ["broker", approval("r1")],
    ["屏幕", screenRequest("opencode", OPENCODE_PERMISSION_SCREEN)],
  ])("%s：焦点在输入框或按钮上时不显示数字徽标，焦点离开后显示", async (_name, request) => {
    const composer = document.createElement("textarea");
    document.body.appendChild(composer);
    composer.focus();
    render(<Card request={request} />);
    const allow = screen.getByRole("button", { name: "允许一次" });
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
    expect(allow.querySelector("kbd")).toBeNull();
    expect(screen.getByRole("button", { name: "拒绝" }).querySelector("kbd")?.textContent).toBe("Esc");
    await act(async () => { composer.blur(); await new Promise((resolve) => setTimeout(resolve, 0)); });
    expect(allow.querySelector("kbd")?.textContent).toBe("1");
    await act(async () => { allow.focus(); await new Promise((resolve) => setTimeout(resolve, 0)); });
    expect(allow.querySelector("kbd")).toBeNull();
    composer.remove();
  });
});
