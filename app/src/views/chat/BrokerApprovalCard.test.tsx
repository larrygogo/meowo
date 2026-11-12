import { act, cleanup, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import type { PendingApproval } from "../../api";
import { approvalChoiceForKey, BrokerApprovalCard, digitKeysOwnedElsewhere, useBrokerResolver } from "./BrokerApprovalCard";

afterEach(() => {
  cleanup();
  invoke.mockReset();
});

describe("approvalChoiceForKey", () => {
  it("1 = 允许一次，2.. 依次对应持久放行；超出的数字与非数字不映射", () => {
    expect(approvalChoiceForKey("1", 0)).toBe("allow_once");
    expect(approvalChoiceForKey("2", 2)).toBe("suggestion:0");
    expect(approvalChoiceForKey("3", 2)).toBe("suggestion:1");
    expect(approvalChoiceForKey("4", 2)).toBeNull();
    expect(approvalChoiceForKey("0", 2)).toBeNull();
    // Enter 刻意不映射：输入框外随手一个回车不能放行命令。
    expect(approvalChoiceForKey("Enter", 2)).toBeNull();
  });
});

describe("useBrokerResolver", () => {
  it("在途时第二次结算直接忽略，只发一次", async () => {
    let release!: () => void;
    invoke.mockImplementation(() => new Promise<void>((resolve) => { release = resolve; }));
    const { result } = renderHook(() => useBrokerResolver(7));
    let first!: Promise<unknown>;
    act(() => { first = result.current.resolve("r1", "allow_once"); });
    let second: unknown;
    await act(async () => { second = await result.current.resolve("r1", "deny"); });
    expect(second).toBeNull();
    await act(async () => { release(); await first; });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("resolve_pending_approval", { sessionId: 7, requestId: "r1", choice: "allow_once" });
  });

  it("失败原因按请求绑定：换了请求不显示旧错误", async () => {
    invoke.mockRejectedValueOnce(new Error("boom"));
    const { result } = renderHook(() => useBrokerResolver(7));
    let outcome: unknown;
    await act(async () => { outcome = await result.current.resolve("r1", "allow_once"); });
    expect(outcome).toMatchObject({ ok: false });
    expect(result.current.errorFor("r1")).toBeTruthy();
    expect(result.current.errorFor("r2")).toBeNull();
  });
});

describe("BrokerApprovalCard", () => {
  const approval = (requestId: string): PendingApproval => ({
    requestId, toolName: "Bash", input: "ls", description: null, permissionSuggestions: [],
  } as unknown as PendingApproval);
  const props = { countdown: null, timedOut: false, resolving: false, error: null, escArmed: false, dirs: [], returnFocusTo: { current: null } };

  it("Bash 参数只摆命令本身，复制也复制命令", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
    const input = JSON.stringify({ command: "cargo test", description: "运行测试" });
    render(<BrokerApprovalCard {...props} approval={{ ...approval("r1"), input }} onDecide={() => {}} />);
    expect(screen.getByText("命令")).toBeTruthy();
    expect(screen.getByText("cargo test")).toBeTruthy();
    expect(screen.queryByText(input)).toBeNull();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "复制命令" })); });
    expect(writeText).toHaveBeenCalledWith("cargo test");
  });

  it("「说明并拒绝」把理由带进拒绝选择", () => {
    const onDecide = vi.fn();
    render(<BrokerApprovalCard {...props} approval={approval("r1")} onDecide={onDecide} />);
    fireEvent.click(screen.getByRole("button", { name: "说明并拒绝" }));
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "先别动生产库" } });
    fireEvent.submit(screen.getByRole("textbox").closest("form")!);
    expect(onDecide).toHaveBeenCalledWith("deny:先别动生产库");
  });

  /// 按 requestId 挂载：换请求即新卡，上一张卡写了一半的理由不能串过来。
  it("换请求后理由表单归零", () => {
    const { rerender } = render(<BrokerApprovalCard key="r1" {...props} approval={approval("r1")} onDecide={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "说明并拒绝" }));
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "半句" } });
    rerender(<BrokerApprovalCard key="r2" {...props} approval={approval("r2")} onDecide={() => {}} />);
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  /// 走查 C-3（p1-broker-fail：结算失败后 activeElement=BODY）：结算在途用 aria-disabled，焦点留在刚按的
  /// 按钮上；在途的点击不发出去。兄弟场景：持久放行、「说明并拒绝」、拒绝、理由提交钮同一口径。
  it("结算在途按钮不失焦、点了不发；失败后可原地重按", () => {
    const onDecide = vi.fn();
    const suggestions = [{ type: "addRules", destination: "session", rules: [{ toolName: "Bash", ruleContent: "ls" }] }];
    const card = (resolving: boolean, error: string | null) => (
      <BrokerApprovalCard {...props} resolving={resolving} error={error} approval={{ ...approval("r1"), permissionSuggestions: suggestions } as PendingApproval} onDecide={onDecide} />
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
    render(<BrokerApprovalCard {...props} approval={approval("r1")} onDecide={onDecide} />);
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

  /// 走查 C-5（p2-digit-in-ta：默认焦点在输入框，按「1」只把数字打进输入框）：数字徽标只在按下去真生效时
  /// 显示，判据与窗口级数字键监听同一份（digitKeysOwnedElsewhere）。Esc 徽标照常常驻（空输入框里也生效）。
  it("焦点在输入框或按钮上时不显示数字徽标，焦点离开后显示", async () => {
    const composer = document.createElement("textarea");
    document.body.appendChild(composer);
    composer.focus();
    render(<BrokerApprovalCard {...props} approval={approval("r1")} onDecide={() => {}} />);
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

  it("digitKeysOwnedElsewhere：输入控件与按钮链接归它们自己，body 与普通元素归审批卡", () => {
    const make = (tag: string) => document.createElement(tag);
    expect(digitKeysOwnedElsewhere(make("textarea"))).toBe(true);
    expect(digitKeysOwnedElsewhere(make("input"))).toBe(true);
    expect(digitKeysOwnedElsewhere(make("button"))).toBe(true);
    expect(digitKeysOwnedElsewhere(make("a"))).toBe(true);
    expect(digitKeysOwnedElsewhere(document.body)).toBe(false);
    expect(digitKeysOwnedElsewhere(make("div"))).toBe(false);
    expect(digitKeysOwnedElsewhere(null)).toBe(false);
  });
});
