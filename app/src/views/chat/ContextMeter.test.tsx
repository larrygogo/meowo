import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderUsage } from "../../api";
import { listProfiles, refreshProfileUsage } from "../../api";
import { zh } from "../../i18n/zh";
import { ContextMeter, fmtTokens, resetChatUsageCache } from "./ContextMeter";

vi.mock("../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api")>()),
  refreshProfileUsage: vi.fn(),
  listProfiles: vi.fn(async () => []),
}));

const usage = (pcts: number[]): ProviderUsage => ({
  note: null,
  lanes: pcts.map((p, i) => ({
    kind: i === 0 ? "five_hour" : "seven_day",
    used_pct: p, used: null, limit: null, unit: null,
    resets_at: new Date(Date.now() + 90 * 60_000).toISOString(),
  })),
});

const meter = (extra: Partial<Parameters<typeof ContextMeter>[0]> = {}) => (
  <ContextMeter pct={30} window={200_000} t={zh} provider="claude" profile="p1" agentName="Claude Code" showUsage {...extra} />
);

beforeEach(() => {
  resetChatUsageCache();
  vi.mocked(refreshProfileUsage).mockReset();
  vi.mocked(listProfiles).mockResolvedValue([]);
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe("ContextMeter 卡片", () => {
  it("token 数按 k/M 缩写", () => {
    expect(fmtTokens(607_700)).toBe("607.7k");
    expect(fmtTokens(1_000_000)).toBe("1M");
    expect(fmtTokens(950)).toBe("950");
  });

  it("点击弹出：上下文已用/窗口 + 这个对话所用账号的额度（≥90% 标红）", async () => {
    vi.mocked(refreshProfileUsage).mockResolvedValue(usage([92, 40]));
    render(meter());
    fireEvent.click(screen.getByTestId("chat-context"));
    expect(screen.getByText("60k/200k")).toBeTruthy();
    expect(refreshProfileUsage).toHaveBeenCalledWith("claude", "p1");
    await screen.findByText("92%");
    const bars = screen.getAllByRole("progressbar");
    expect(bars[0].className).toContain("is-full");
    expect(bars[1].className).not.toContain("is-warn");
    expect(screen.getByText(zh.chat.chatAccountUsage)).toBeTruthy();
  });

  it("5 分钟内重开不重复请求", async () => {
    vi.mocked(refreshProfileUsage).mockResolvedValue(usage([10]));
    render(meter());
    const btn = screen.getByTestId("chat-context");
    fireEvent.click(btn);
    await screen.findByText("10%");
    fireEvent.click(btn);
    fireEvent.click(btn);
    await screen.findByText("10%");
    expect(refreshProfileUsage).toHaveBeenCalledTimes(1);
  });

  it("限流失败：说清原因并给重试，重试会强制再取", async () => {
    vi.mocked(refreshProfileUsage).mockRejectedValueOnce("USAGE_RATE_LIMITED:120").mockResolvedValueOnce(usage([55]));
    render(meter());
    fireEvent.click(screen.getByTestId("chat-context"));
    const retry = await screen.findByTestId("chat-context-retry");
    expect(screen.getByRole("status").textContent).toContain(zh.account.usageRateLimited(2));
    await act(async () => { fireEvent.click(retry); });
    await screen.findByText("55%");
    expect(refreshProfileUsage).toHaveBeenCalledTimes(2);
  });

  it("凭据过期：提示等 CLI 运行时更新，不给重试", async () => {
    vi.mocked(refreshProfileUsage).mockRejectedValue("USAGE_EXPIRED");
    render(meter());
    fireEvent.click(screen.getByTestId("chat-context"));
    await screen.findByRole("status");
    expect(screen.getByRole("status").textContent).toContain(zh.account.usageExpired("Claude Code"));
    expect(screen.queryByTestId("chat-context-retry")).toBeNull();
  });

  it("不支持账号的 agent 只显示上下文段；不上报上下文时说明", () => {
    render(meter({ showUsage: false, pct: null, window: null }));
    fireEvent.click(screen.getByTestId("chat-context"));
    expect(screen.getByTestId("chat-context-card").textContent).toContain(zh.chat.contextUnsupported);
    expect(screen.queryByTestId("chat-context-usage")).toBeNull();
    expect(refreshProfileUsage).not.toHaveBeenCalled();
  });

  it("悬停 150ms 打开，移开关闭；点开的 Esc 关闭", async () => {
    vi.useFakeTimers();
    vi.mocked(refreshProfileUsage).mockResolvedValue(usage([10]));
    render(meter({ showUsage: false }));
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
