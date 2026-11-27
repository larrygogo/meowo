import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderUsage } from "../../api";
import { listProfiles, refreshProfileUsage } from "../../api";
import { zh } from "../../i18n/zh";
import { ChatStatusBar } from "./ChatStatusBar";
import { fmtResetShort, resetChatUsageCache, resetUsageDisplay } from "./chatAccountUsage";

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
    resets_at: new Date(Date.now() + (i === 0 ? 219 : 3 * 1440 + 13 * 60) * 60_000 + 20_000).toISOString(),
  })),
});

const bar = (extra: Partial<Parameters<typeof ChatStatusBar>[0]> = {}) => (
  <ChatStatusBar provider="claude" profile="p1" agentName="Claude Code" supportsAccount {...extra} />
);

beforeEach(() => {
  resetChatUsageCache();
  resetUsageDisplay();
  vi.mocked(refreshProfileUsage).mockReset();
  vi.mocked(listProfiles).mockResolvedValue([]);
});
afterEach(() => cleanup());

describe("ChatStatusBar 底部状态条", () => {
  it("短倒计时：分钟 / 小时+分 / 天+小时", () => {
    const now = Date.parse("2026-11-28T00:00:00Z");
    expect(fmtResetShort("2026-11-28T00:12:00Z", now)).toBe("12m");
    expect(fmtResetShort("2026-11-28T03:39:00Z", now)).toBe("3h 39m");
    expect(fmtResetShort("2026-12-01T13:00:00Z", now)).toBe("3d 13h");
  });

  it("不用悬停：挂上就读这个对话所用账号的额度并常驻显示 5h / 7 天", async () => {
    vi.mocked(refreshProfileUsage).mockResolvedValue(usage([9, 79]));
    render(bar());
    expect(refreshProfileUsage).toHaveBeenCalledWith("claude", "p1");
    const chip = await screen.findByTestId("chat-statusbar-usage");
    await waitFor(() => expect(chip.textContent).toContain("9%"));
    expect(chip.textContent).toContain("3h 39m");
    expect(chip.textContent).toContain("79%");
    expect(chip.textContent).toContain("3d 13h");
    expect(chip.textContent).toContain(zh.chat.usageUsed);
  });

  it("点开详情：每个窗口一条进度条，≥90% 标红、≥80% 标黄；切到剩余后数字变、色阶不变", async () => {
    vi.mocked(refreshProfileUsage).mockResolvedValue(usage([92, 84]));
    render(bar());
    const chip = await screen.findByTestId("chat-statusbar-usage");
    await waitFor(() => expect(chip.textContent).toContain("92%"));
    fireEvent.click(chip);
    const bars = screen.getAllByRole("progressbar");
    expect(bars[0].className).toContain("is-full");
    expect(bars[1].className).toContain("is-warn");
    fireEvent.click(screen.getByTestId("chat-statusbar-toggle"));
    expect(chip.textContent).toContain(zh.chat.usageRemaining);
    expect(chip.textContent).toContain("8%");
    expect(screen.getAllByRole("progressbar")[0].className).toContain("is-full");
    expect(screen.getAllByRole("progressbar")[0].getAttribute("aria-valuenow")).toBe("8");
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByTestId("chat-statusbar-card")).toBeNull());
  });

  it("凭据过期且没有旧数字：⚠ + 短文案，详情说清原因、不给重试", async () => {
    vi.mocked(refreshProfileUsage).mockRejectedValue("USAGE_EXPIRED");
    render(bar());
    const chip = await screen.findByTestId("chat-statusbar-usage");
    await waitFor(() => expect(chip.textContent).toContain(zh.chat.usageShortExpired));
    fireEvent.click(chip);
    expect(screen.getByRole("status").textContent).toContain(zh.account.usageExpired("Claude Code"));
    expect(screen.queryByTestId("chat-statusbar-retry")).toBeNull();
  });

  it("刷新钮强制再取；5 分钟内重挂不重复请求", async () => {
    vi.mocked(refreshProfileUsage).mockResolvedValue(usage([10]));
    const { unmount } = render(bar());
    const chip = await screen.findByTestId("chat-statusbar-usage");
    await waitFor(() => expect(chip.textContent).toContain("10%"));
    unmount();
    render(bar());
    await screen.findByTestId("chat-statusbar-usage");
    expect(refreshProfileUsage).toHaveBeenCalledTimes(1);
    await act(async () => { fireEvent.click(screen.getByTestId("chat-statusbar-refresh")); });
    await waitFor(() => expect(refreshProfileUsage).toHaveBeenCalledTimes(2));
  });

  it("不支持账号额度的 agent：只显示 --，不发请求", () => {
    render(bar({ supportsAccount: false, provider: "opencode", agentName: "OpenCode" }));
    expect(screen.getByTestId("chat-statusbar").textContent).toContain("--");
    expect(screen.queryByTestId("chat-statusbar-usage")).toBeNull();
    expect(refreshProfileUsage).not.toHaveBeenCalled();
  });
});
