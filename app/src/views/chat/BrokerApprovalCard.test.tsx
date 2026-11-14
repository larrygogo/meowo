import { act, cleanup, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import type { RuntimeRequest } from "../../api";
import { approvalChoiceForKey, BrokerApprovalCard, useBrokerResolver } from "./BrokerApprovalCard";

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
    act(() => { first = result.current.respond("r1", "allow_once"); });
    let second: unknown;
    await act(async () => { second = await result.current.respond("r1", "deny"); });
    expect(second).toBeNull();
    await act(async () => { release(); await first; });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("respond_runtime_request", { sessionId: 7, requestId: "r1", choice: "allow_once" });
  });

  it("失败原因按请求绑定：换了请求不显示旧错误", async () => {
    invoke.mockRejectedValueOnce(new Error("boom"));
    const { result } = renderHook(() => useBrokerResolver(7));
    let outcome: unknown;
    await act(async () => { outcome = await result.current.respond("r1", "allow_once"); });
    expect(outcome).toMatchObject({ ok: false });
    expect(result.current.errorFor("r1")).toBeTruthy();
    expect(result.current.errorFor("r2")).toBeNull();
    expect(invoke).not.toHaveBeenCalledWith("resolve_pending_approval", expect.anything());
  });

  it("普通审批仅在统一命令缺席时回退旧接口", async () => {
    invoke.mockImplementation((command: string) => command === "respond_runtime_request"
      ? Promise.reject(new Error("Command respond_runtime_request not found"))
      : Promise.resolve());
    const { result } = renderHook(() => useBrokerResolver(7));
    let outcome: unknown;
    await act(async () => { outcome = await result.current.respond("r1", "deny:先别执行"); });
    expect(outcome).toMatchObject({ ok: true });
    expect(invoke).toHaveBeenCalledWith("respond_runtime_request", { sessionId: 7, requestId: "r1", choice: "deny:先别执行" });
    expect(invoke).toHaveBeenCalledWith("resolve_pending_approval", { sessionId: 7, requestId: "r1", choice: "deny:先别执行" });
  });
});

describe("BrokerApprovalCard", () => {
  const approval = (requestId: string): RuntimeRequest => ({
    requestId, sessionId: 7, provider: "claude", kind: "approval", status: "pending",
    toolName: "Bash", input: "ls", description: null, permissionSuggestions: [],
    responseCapability: { type: "broker_live" },
  });
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
});
