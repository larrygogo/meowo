import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { zh } from "../../i18n/zh";
import { ContextMeter, fmtTokens } from "./ContextMeter";

const meter = (extra: Partial<Parameters<typeof ContextMeter>[0]> = {}) => (
  <ContextMeter pct={30} window={200_000} t={zh} {...extra} />
);

afterEach(() => { cleanup(); vi.useRealTimers(); });

describe("ContextMeter 卡片", () => {
  it("token 数按 k/M 缩写", () => {
    expect(fmtTokens(607_700)).toBe("607.7k");
    expect(fmtTokens(1_000_000)).toBe("1M");
    expect(fmtTokens(950)).toBe("950");
  });

  it("点击弹出：只有上下文已用/窗口（账号额度照 Orca 归底部状态条）", () => {
    render(meter());
    fireEvent.click(screen.getByTestId("chat-context"));
    expect(screen.getByText("60k/200k")).toBeTruthy();
    expect(screen.queryByText(zh.chat.chatAccountUsage)).toBeNull();
  });

  it("不上报上下文时说明", () => {
    render(meter({ pct: null, window: null }));
    fireEvent.click(screen.getByTestId("chat-context"));
    expect(screen.getByTestId("chat-context-card").textContent).toContain(zh.chat.contextUnsupported);
  });

  it("悬停 150ms 打开，移开关闭；点开的 Esc 关闭", async () => {
    vi.useFakeTimers();
    render(meter());
    const wrap = screen.getByTestId("chat-context").parentElement!;
    fireEvent.mouseEnter(wrap);
    expect(screen.queryByTestId("chat-context-card")).toBeNull();
    await act(async () => { vi.advanceTimersByTime(160); });
    expect(screen.getByTestId("chat-context-card")).toBeTruthy();
    fireEvent.mouseLeave(wrap);
    await act(async () => { vi.advanceTimersByTime(160); });
    expect(screen.queryByTestId("chat-context-card")).toBeNull();
    vi.useRealTimers();
    fireEvent.click(screen.getByTestId("chat-context"));
    expect(screen.getByTestId("chat-context-card")).toBeTruthy();
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByTestId("chat-context-card")).toBeNull());
  });
});
