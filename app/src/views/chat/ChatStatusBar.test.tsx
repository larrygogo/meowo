import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderUsage } from "../../api";
import { getSettings, listProfiles, refreshProfileUsage, type Settings } from "../../api";
import { zh } from "../../i18n/zh";
import { ChatStatusBar } from "./ChatStatusBar";
import { fmtResetShort, resetChatUsageCache, resetUsageDisplay } from "./chatAccountUsage";

vi.mock("../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api")>()),
  refreshProfileUsage: vi.fn(),
  listProfiles: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ usage_polling_disabled: [] })),
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

/** 点状态条打开用量面板，再点 Claude 行打开账号面板（Orca 的子菜单）。 */
const openPanel = (chip: HTMLElement) => {
  fireEvent.click(chip);
  fireEvent.click(screen.getByTestId("chat-statusbar-provider-row"));
};

beforeEach(() => {
  resetChatUsageCache();
  resetUsageDisplay();
  vi.mocked(refreshProfileUsage).mockReset();
  vi.mocked(listProfiles).mockResolvedValue([]);
  vi.mocked(getSettings).mockResolvedValue({ usage_polling_disabled: [] } as unknown as Settings);
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
    await waitFor(() => expect(refreshProfileUsage).toHaveBeenCalledWith("claude", "p1"));
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
    openPanel(chip);
    const fills = () => [...screen.getByTestId("chat-statusbar-panel").querySelectorAll<HTMLElement>(".ou-window-bar .ou-bar-fill")];
    expect(fills()[0].className).toContain("is-urgent");
    expect(fills()[1].className).toContain("is-warning");
    fireEvent.click(screen.getAllByTestId("chat-statusbar-toggle")[0]);
    expect(chip.textContent).toContain(zh.chat.usageRemaining);
    expect(chip.textContent).toContain("8%");
    // 色阶始终按已用算（Orca：Tone always follows consumption），条长按显示的数字。
    expect(fills()[0].className).toContain("is-urgent");
    expect(fills()[0].style.width).toBe("8%");
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByTestId("chat-statusbar-card")).toBeNull());
  });

  it("凭据过期且没有旧数字：⚠ + 短文案，详情说清原因、不给重试", async () => {
    vi.mocked(refreshProfileUsage).mockRejectedValue("USAGE_EXPIRED");
    render(bar());
    const chip = await screen.findByTestId("chat-statusbar-usage");
    await waitFor(() => expect(chip.textContent).toContain(zh.chat.usageShortExpired));
    openPanel(chip);
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

  it("设置里关了用量轮询：挂上不取，打开用量面板才取", async () => {
    vi.mocked(getSettings).mockResolvedValue({ usage_polling_disabled: ["claude"] } as unknown as Settings);
    vi.mocked(refreshProfileUsage).mockResolvedValue(usage([12]));
    render(bar());
    const chip = await screen.findByTestId("chat-statusbar-usage");
    await act(async () => { await Promise.resolve(); });
    expect(refreshProfileUsage).not.toHaveBeenCalled();
    fireEvent.click(chip);
    await waitFor(() => expect(refreshProfileUsage).toHaveBeenCalledWith("claude", "p1"));
    await waitFor(() => expect(chip.textContent).toContain("12%"));
  });

  it("不支持账号额度的 agent：只显示 --，不发请求", () => {
    render(bar({ supportsAccount: false, provider: "opencode", agentName: "OpenCode" }));
    expect(screen.getByTestId("chat-statusbar").textContent).toContain("--");
    expect(screen.queryByTestId("chat-statusbar-usage")).toBeNull();
    expect(refreshProfileUsage).not.toHaveBeenCalled();
  });
});

describe("ChatStatusBar 多账号面板（照 Orca 的 Claude 账号面板）", () => {
  const acct = (email: string, plan: string) => ({ email, display_name: null, login_label: null, plan, org: null });
  const rows = () => [
    { id: null, name: "", active: true, account: acct("me@example.com", "Max 5x"), set_aside: [] },
    { id: "p1", name: "工作", active: false, account: acct("work@example.com", "Max 20x"), set_aside: [] },
    { id: "p2", name: "", active: false, account: acct("side@example.com", "Pro"), set_aside: [] },
    { id: "p3", name: "", active: false, account: null, set_aside: [] },
  ] as unknown as Awaited<ReturnType<typeof listProfiles>>;

  it("列出系统默认和所有账号；其余账号一个接一个读，未登录的不读；此对话的账号打勾", async () => {
    vi.mocked(listProfiles).mockResolvedValue(rows());
    const pending: Record<string, (u: ProviderUsage) => void> = {};
    vi.mocked(refreshProfileUsage).mockImplementation((_p, profile) => new Promise((res) => { pending[String(profile)] = res; }));
    render(bar({ profile: "p1" }));
    await waitFor(() => expect(pending.p1).toBeTruthy());
    await act(async () => { pending.p1(usage([93, 64])); });
    openPanel(screen.getByTestId("chat-statusbar-usage"));
    await screen.findByTestId("chat-statusbar-roster");
    // 系统默认先问；它回来之前不问下一个（不会几个账号同时打官方接口）。
    await waitFor(() => expect(pending.null).toBeTruthy());
    expect(pending.p2).toBeUndefined();
    await act(async () => { pending.null(usage([16, 81])); });
    await waitFor(() => expect(pending.p2).toBeTruthy());
    await act(async () => { pending.p2(usage([5, 10])); });
    const calls = vi.mocked(refreshProfileUsage).mock.calls.map((c) => c[1]);
    expect(calls).toEqual(["p1", null, "p2"]);
    const rowsEl = screen.getAllByTestId("chat-statusbar-acct");
    expect(rowsEl).toHaveLength(4);
    expect(rowsEl[1].getAttribute("aria-current")).toBe("true");
    expect(rowsEl[1].querySelector(".ou-acct-check svg")).not.toBeNull();
    expect(rowsEl[2].textContent).toContain(zh.chat.usagePersonal);
    expect(rowsEl[0].textContent).toContain("81%");
    expect(rowsEl[3].textContent).toContain(zh.chat.usageSignIn);
  });

  it("60 秒内重开面板不再来一轮；点刷新不受限", async () => {
    vi.mocked(listProfiles).mockResolvedValue(rows().slice(0, 2));
    vi.mocked(refreshProfileUsage).mockResolvedValue(usage([10]));
    render(bar({ profile: "p1" }));
    const chip = await screen.findByTestId("chat-statusbar-usage");
    openPanel(chip);
    await waitFor(() => expect(refreshProfileUsage).toHaveBeenCalledTimes(2));
    fireEvent.click(chip);
    openPanel(chip);
    await screen.findByTestId("chat-statusbar-roster");
    expect(refreshProfileUsage).toHaveBeenCalledTimes(2);
    await act(async () => { fireEvent.click(screen.getByTestId("chat-statusbar-refresh-all")); });
    await waitFor(() => expect(refreshProfileUsage).toHaveBeenCalledTimes(4));
  });

  it("精简：状态条只留最紧的百分比，不带倒计时", async () => {
    vi.mocked(refreshProfileUsage).mockResolvedValue(usage([9, 79]));
    render(bar());
    const chip = await screen.findByTestId("chat-statusbar-usage");
    await waitFor(() => expect(chip.textContent).toContain("3h 39m"));
    fireEvent.click(chip);
    fireEvent.click(screen.getByTestId("chat-statusbar-mode-compact"));
    expect(chip.textContent).toContain("79%");
    expect(chip.textContent).not.toContain("9%·");
    expect(chip.textContent).not.toContain("3h 39m");
  });
});
