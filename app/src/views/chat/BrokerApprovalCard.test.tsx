import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

// 卡片本体的测试在 ApprovalRequestCard.test.tsx（broker 与屏幕两条通道同一张卡）。
import { approvalChoiceForKey, digitKeysOwnedElsewhere, useBrokerResolver } from "./BrokerApprovalCard";

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

describe("digitKeysOwnedElsewhere", () => {
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
