import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProfileView, ProviderAccountPayload, ProviderUsage } from "../api";

const api = vi.hoisted(() => ({
  getAccounts: vi.fn(),
  refreshUsage: vi.fn(),
  listProfiles: vi.fn(),
  refreshProfileUsage: vi.fn(),
  setActiveProfile: vi.fn(),
  sessionsOffActiveProfileCount: vi.fn(),
  applyActiveProfileToSessions: vi.fn(),
  getSettings: vi.fn(),
}));
const confirm = vi.hoisted(() => ({ appConfirm: vi.fn() }));
vi.mock("../api", async (importOriginal) => ({ ...(await importOriginal<typeof import("../api")>()), ...api }));
vi.mock("../confirm", async (importOriginal) => ({ ...(await importOriginal<typeof import("../confirm")>()), ...confirm }));

import { RemoteUsage } from "./RemoteUsage";

function usage(pct: number): ProviderUsage {
  return { lanes: [{ kind: "five_hour", used_pct: pct, used: null, limit: null, unit: null, resets_at: null }], note: null };
}

function payload(provider: string, patch: Partial<ProviderAccountPayload> = {}): ProviderAccountPayload {
  return {
    provider,
    account: { email: `${provider}@example.com`, display_name: null, organization: null, plan: null, login_label: null },
    usage: null,
    usage_supported: true,
    relay_enabled: false,
    ...patch,
  };
}

function profile(id: string | null, name: string, active: boolean, patch: Partial<ProfileView> = {}): ProfileView {
  return {
    id,
    name,
    active,
    account: { email: `${id ?? "default"}@example.com`, display_name: null, organization: null, plan: null, login_label: null },
    set_aside: [],
    ...patch,
  };
}

describe("RemoteUsage", () => {
  beforeEach(() => {
    api.getAccounts.mockResolvedValue([]);
    api.refreshUsage.mockResolvedValue(usage(60));
    // 默认每家只有默认账号一条：不出列表，与改版前一致。
    api.listProfiles.mockImplementation(async () => [profile(null, "", true)]);
    api.setActiveProfile.mockResolvedValue(undefined);
    api.sessionsOffActiveProfileCount.mockResolvedValue(0);
    api.applyActiveProfileToSessions.mockResolvedValue(0);
    api.getSettings.mockResolvedValue({ profile_switch_running: "ask" });
    confirm.appConfirm.mockResolvedValue(true);
  });
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    vi.useRealTimers();
  });

  it("先展示缓存的全部泳道、余额、重置时间和备注，不自动联网刷新", async () => {
    // 「30 分钟后重置」只在同一天里成立：23:30 之后跑，重置时刻落到次日，文案变成「明天 …」。钉在中午。
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date(2026, 10, 18, 12, 0));
    api.getAccounts.mockResolvedValue([
      payload("claude", {
        usage: {
          lanes: [
            { kind: "five_hour", used_pct: 42, used: null, limit: null, unit: null, resets_at: new Date(Date.now() + 30 * 60_000).toISOString() },
            { kind: "model_weekly", label: "Fable", used_pct: 104, used: null, limit: null, unit: null, resets_at: null },
          ],
          note: "extra_usage_enabled",
        },
      }),
      payload("codex", {
        usage: {
          lanes: [{ kind: "balance", used_pct: null, used: 12, limit: null, unit: "USD", resets_at: null }],
          note: "credits:45.5",
        },
      }),
    ]);
    render(<RemoteUsage />);

    const claude = within(await screen.findByRole("region", { name: "Claude" }));
    expect(claude.getByRole("progressbar", { name: "5 小时配额" }).getAttribute("aria-valuenow")).toBe("42");
    expect(claude.getByRole("progressbar", { name: "Fable · 每周" }).getAttribute("aria-valuenow")).toBe("100");
    expect(claude.getByText(/分钟后重置/)).toBeTruthy();
    expect(claude.getByText("已开启超额用量")).toBeTruthy();
    const codex = within(screen.getByRole("region", { name: "Codex" }));
    expect(codex.getByText("12 USD")).toBeTruthy();
    expect(codex.getByText("额度余额：45.5")).toBeTruthy();
    expect(api.refreshUsage).not.toHaveBeenCalled();
  });

  it("未登录、不支持和中转显示真实状态，不提供无效的刷新入口", async () => {
    api.getAccounts.mockResolvedValue([
      payload("codex", { account: null }),
      payload("kimi", { usage_supported: false }),
      payload("opencode", { relay_enabled: true }),
    ]);
    render(<RemoteUsage />);

    expect((await screen.findByRole("region", { name: "Codex" })).textContent).toContain("未登录");
    expect(screen.getByRole("region", { name: "Kimi" }).textContent).toContain("当前账号不支持用量查询");
    expect(screen.getByRole("region", { name: "Opencode" }).textContent).toContain("正在使用 API 中转，官方配额不适用");
    expect(screen.queryByRole("button", { name: /刷新 .* 用量/ })).toBeNull();
  });

  it("单独刷新成功后更新；失败后保留缓存并提示", async () => {
    api.getAccounts.mockResolvedValue([payload("claude", { usage: usage(25) })]);
    api.refreshUsage.mockResolvedValueOnce(usage(62));
    render(<RemoteUsage />);

    const button = await screen.findByRole("button", { name: "刷新 Claude 用量" });
    fireEvent.click(button);
    await waitFor(() => expect(screen.getByRole("progressbar", { name: "5 小时配额" }).getAttribute("aria-valuenow")).toBe("62"));
    expect(api.refreshUsage).toHaveBeenCalledTimes(1);
    expect(api.refreshUsage).toHaveBeenCalledWith("claude");

    api.refreshUsage.mockRejectedValueOnce("network down");
    fireEvent.click(button);
    expect((await screen.findByRole("alert")).textContent).toContain("刷新失败，显示缓存值");
    expect(screen.getByRole("progressbar", { name: "5 小时配额" }).getAttribute("aria-valuenow")).toBe("62");
  });

  it("账号快照读取失败时给出错误，而不是伪装成空列表", async () => {
    api.getAccounts.mockRejectedValue("offline");
    render(<RemoteUsage />);
    expect((await screen.findByRole("alert")).textContent).toContain("账号信息加载失败");
  });

  describe("多账号", () => {
    const twoProfiles = () => [
      profile(null, "", true, { account: { email: "me@home.com", display_name: null, organization: null, plan: "Max", login_label: null } }),
      profile("work", "工作", false, { account: { email: null, display_name: null, organization: null, plan: null, login_label: "API Key" } }),
    ];

    it("多账号的家列出全部账号（名字、描述、套餐、使用中），单账号家不出列表", async () => {
      api.getAccounts.mockResolvedValue([payload("claude", { usage: usage(30) }), payload("codex")]);
      api.listProfiles.mockImplementation(async (provider: string) =>
        provider === "claude" ? [...twoProfiles(), profile("spare", "备用", false, { account: null })] : [profile(null, "", true)],
      );
      render(<RemoteUsage />);

      const list = within(await screen.findByRole("group", { name: "Claude 账号" }));
      const def = list.getByRole("button", { name: "查看默认账号的用量" });
      expect(def.textContent).toContain("默认账号");
      expect(def.textContent).toContain("me@home.com");
      expect(def.textContent).toContain("Max");
      expect(def.getAttribute("aria-pressed")).toBe("true");
      expect(list.getByText("使用中")).toBeTruthy();
      expect(list.getByRole("button", { name: "查看工作的用量" }).textContent).toContain("API Key");
      expect(list.getByRole("button", { name: "切换到工作" })).toBeTruthy();
      // 未登录的账号手机上登不了：不给切换，说清去电脑上登录
      expect(list.getByRole("button", { name: "查看备用的用量" }).textContent).toContain("未登录，在电脑上登录后可切换");
      expect(list.queryByRole("button", { name: "切换到备用" })).toBeNull();
      // 当前账号不给切换按钮
      expect(list.queryByRole("button", { name: "切换到默认账号" })).toBeNull();

      const single = within(screen.getByRole("region", { name: "Codex" }));
      expect(single.queryByRole("group")).toBeNull();
      expect(single.queryByText("使用中")).toBeNull();
      expect(screen.getByText("登录、退出和管理账号请在电脑上操作")).toBeTruthy();
      // 打开只读本地：不联网查用量
      expect(api.refreshUsage).not.toHaveBeenCalled();
      expect(api.refreshProfileUsage).not.toHaveBeenCalled();
      expect(api.listProfiles).toHaveBeenCalledTimes(2);
    });

    it("点非当前账号查它的用量，点回当前账号显示缓存；失败与不支持各有说法", async () => {
      api.getAccounts.mockResolvedValue([payload("claude", { usage: usage(30) })]);
      api.listProfiles.mockResolvedValue(twoProfiles());
      api.refreshProfileUsage.mockResolvedValueOnce(usage(77));
      render(<RemoteUsage />);

      const work = await screen.findByRole("button", { name: "查看工作的用量" });
      fireEvent.click(work);
      await waitFor(() => expect(screen.getByRole("progressbar", { name: "5 小时配额" }).getAttribute("aria-valuenow")).toBe("77"));
      expect(api.refreshProfileUsage).toHaveBeenCalledWith("claude", "work");
      expect(work.getAttribute("aria-pressed")).toBe("true");
      expect(screen.getByText("工作 · 用量")).toBeTruthy();

      fireEvent.click(screen.getByRole("button", { name: "查看默认账号的用量" }));
      expect(screen.getByRole("progressbar", { name: "5 小时配额" }).getAttribute("aria-valuenow")).toBe("30");
      expect(api.refreshProfileUsage).toHaveBeenCalledTimes(1);
      expect(api.refreshUsage).not.toHaveBeenCalled();

      api.refreshProfileUsage.mockRejectedValueOnce("network down");
      fireEvent.click(work);
      expect((await screen.findByRole("alert")).textContent).toContain("用量获取失败");

      api.refreshProfileUsage.mockRejectedValueOnce("usage/USAGE_UNSUPPORTED");
      fireEvent.click(screen.getByRole("button", { name: "查看默认账号的用量" }));
      fireEvent.click(work);
      expect(await screen.findByText("当前账号不支持用量查询")).toBeTruthy();
      expect(screen.queryByRole("alert")).toBeNull();
    });

    it("切换：setActive → 问后端计数 → 确认 → 重启，再整页重载", async () => {
      api.getAccounts.mockResolvedValue([payload("claude", { usage: usage(30) })]);
      api.listProfiles.mockResolvedValueOnce(twoProfiles()).mockResolvedValue([
        profile(null, "", false),
        profile("work", "工作", true),
      ]);
      const calls: string[] = [];
      api.setActiveProfile.mockImplementation(async () => { calls.push("set"); });
      api.sessionsOffActiveProfileCount.mockImplementation(async () => { calls.push("count"); return 2; });
      confirm.appConfirm.mockImplementation(async () => { calls.push("confirm"); return true; });
      api.applyActiveProfileToSessions.mockImplementation(async () => { calls.push("apply"); return 2; });
      render(<RemoteUsage />);

      const button = await screen.findByRole("button", { name: "切换到工作" });
      fireEvent.click(button);
      // 进行中再点不重复调用
      fireEvent.click(button);
      await waitFor(() => expect(api.getAccounts).toHaveBeenCalledTimes(2));
      expect(calls).toEqual(["set", "count", "confirm", "apply"]);
      expect(api.setActiveProfile).toHaveBeenCalledWith("claude", "work");
      expect(api.sessionsOffActiveProfileCount).toHaveBeenCalledWith("claude");
      expect(api.applyActiveProfileToSessions).toHaveBeenCalledWith("claude");
      expect(String(confirm.appConfirm.mock.calls[0][0])).toContain("2 个会话");
      // 重载后「使用中」到了工作账号上
      const list = within(await screen.findByRole("group", { name: "Claude 账号" }));
      await waitFor(() => expect(list.getByRole("button", { name: "切换到默认账号" })).toBeTruthy());
      expect(list.queryByRole("button", { name: "切换到工作" })).toBeNull();
    });

    it("切换：与桌面同一份设置，直接重启不问、留在原账号不重启", async () => {
      api.getAccounts.mockResolvedValue([payload("claude", { usage: usage(30) })]);
      api.listProfiles.mockResolvedValue(twoProfiles());
      api.sessionsOffActiveProfileCount.mockResolvedValue(2);
      api.getSettings.mockResolvedValue({ profile_switch_running: "restart" });
      const first = render(<RemoteUsage />);
      fireEvent.click(await screen.findByRole("button", { name: "切换到工作" }));
      await waitFor(() => expect(api.applyActiveProfileToSessions).toHaveBeenCalledWith("claude"));
      expect(confirm.appConfirm).not.toHaveBeenCalled();
      first.unmount();

      vi.clearAllMocks();
      api.getAccounts.mockResolvedValue([payload("claude", { usage: usage(30) })]);
      api.listProfiles.mockResolvedValue(twoProfiles());
      api.setActiveProfile.mockResolvedValue(undefined);
      api.sessionsOffActiveProfileCount.mockResolvedValue(2);
      api.getSettings.mockResolvedValue({ profile_switch_running: "keep" });
      render(<RemoteUsage />);
      fireEvent.click(await screen.findByRole("button", { name: "切换到工作" }));
      await waitFor(() => expect(api.setActiveProfile).toHaveBeenCalledWith("claude", "work"));
      await waitFor(() => expect(api.getAccounts).toHaveBeenCalledTimes(2));
      expect(api.sessionsOffActiveProfileCount).not.toHaveBeenCalled();
      expect(confirm.appConfirm).not.toHaveBeenCalled();
      expect(api.applyActiveProfileToSessions).not.toHaveBeenCalled();
    });

    it("切换后丢掉旧账号的用量，自动拉一次新账号的", async () => {
      api.getAccounts
        .mockResolvedValueOnce([payload("claude", { usage: usage(30) })])
        .mockResolvedValue([payload("claude", { usage: null, active_profile_name: "工作" })]);
      api.listProfiles.mockResolvedValue(twoProfiles());
      api.refreshUsage.mockResolvedValue(usage(5));
      render(<RemoteUsage />);

      expect((await screen.findByRole("progressbar", { name: "5 小时配额" })).getAttribute("aria-valuenow")).toBe("30");
      fireEvent.click(screen.getByRole("button", { name: "切换到工作" }));
      await waitFor(() => expect(screen.getByRole("progressbar", { name: "5 小时配额" }).getAttribute("aria-valuenow")).toBe("5"));
      expect(api.refreshUsage).toHaveBeenCalledTimes(1);
      expect(api.refreshUsage).toHaveBeenCalledWith("claude");
    });

    it("新账号的用量没拉到：显示失败说法，不拿旧账号的数字顶替", async () => {
      api.getAccounts
        .mockResolvedValueOnce([payload("claude", { usage: usage(30) })])
        .mockResolvedValue([payload("claude", { usage: null, active_profile_name: "工作" })]);
      api.listProfiles.mockResolvedValue(twoProfiles());
      api.refreshUsage.mockRejectedValue("network down");
      render(<RemoteUsage />);

      await screen.findByRole("progressbar", { name: "5 小时配额" });
      fireEvent.click(screen.getByRole("button", { name: "切换到工作" }));
      expect((await screen.findByRole("alert")).textContent).toContain("用量获取失败");
      expect(screen.queryByRole("progressbar")).toBeNull();
    });

    it("电脑上切了账号（设置轮询带来新的活跃账号）：手机这一家跟着重载，别家不动", async () => {
      let active: string | undefined;
      api.getAccounts.mockImplementation(async () => [
        payload("claude", { usage: active ? null : usage(30), active_profile_name: active ? "工作" : null }),
        payload("codex", { usage: usage(44) }),
      ]);
      api.listProfiles.mockImplementation(async (provider: string) =>
        provider === "claude"
          ? [profile(null, "", !active), profile("work", "工作", !!active)]
          : [profile(null, "", true)],
      );
      api.refreshUsage.mockResolvedValue(usage(7));
      const view = render(<RemoteUsage activeProfiles={{}} />);
      const claude = within(await screen.findByRole("region", { name: "Claude" }));
      await waitFor(() => expect(claude.getByRole("button", { name: "切换到工作" })).toBeTruthy());

      active = "work";
      view.rerender(<RemoteUsage activeProfiles={{ claude: "work" }} />);
      await waitFor(() =>
        expect(within(screen.getByRole("region", { name: "Claude" })).getByRole("button", { name: "切换到默认账号" })).toBeTruthy(),
      );
      await waitFor(() =>
        expect(within(screen.getByRole("region", { name: "Claude" })).getByRole("progressbar", { name: "5 小时配额" }).getAttribute("aria-valuenow")).toBe("7"),
      );
      expect(api.refreshUsage).toHaveBeenCalledTimes(1);
      expect(api.refreshUsage).toHaveBeenCalledWith("claude");
      expect(within(screen.getByRole("region", { name: "Codex" })).getByRole("progressbar", { name: "5 小时配额" }).getAttribute("aria-valuenow")).toBe("44");
    });

    it("自己切的账号，设置轮询回来时不再重载第二次", async () => {
      api.getAccounts.mockResolvedValue([payload("claude", { usage: usage(30) })]);
      api.listProfiles.mockResolvedValue(twoProfiles());
      const view = render(<RemoteUsage activeProfiles={{}} />);
      fireEvent.click(await screen.findByRole("button", { name: "切换到工作" }));
      await waitFor(() => expect(api.getAccounts).toHaveBeenCalledTimes(2));
      view.rerender(<RemoteUsage activeProfiles={{ claude: "work" }} />);
      await new Promise((r) => setTimeout(r, 20));
      expect(api.getAccounts).toHaveBeenCalledTimes(2);
    });

    it("切换途中离开设置页：不在别处弹重启确认", async () => {
      api.getAccounts.mockResolvedValue([payload("claude")]);
      api.listProfiles.mockResolvedValue(twoProfiles());
      let releaseCount!: (n: number) => void;
      api.sessionsOffActiveProfileCount.mockImplementation(() => new Promise<number>((r) => { releaseCount = r; }));
      const view = render(<RemoteUsage />);
      fireEvent.click(await screen.findByRole("button", { name: "切换到工作" }));
      await waitFor(() => expect(api.sessionsOffActiveProfileCount).toHaveBeenCalled());
      view.unmount();
      releaseCount(3);
      await new Promise((r) => setTimeout(r, 20));
      expect(confirm.appConfirm).not.toHaveBeenCalled();
      expect(api.applyActiveProfileToSessions).not.toHaveBeenCalled();
    });

    it("用户不同意重启时不调 apply；没有要搬的会话（含搬不了会话的家）不问", async () => {
      api.getAccounts.mockResolvedValue([payload("claude"), payload("kimi")]);
      api.listProfiles.mockResolvedValue(twoProfiles());
      api.sessionsOffActiveProfileCount.mockImplementation(async (provider: string) => (provider === "claude" ? 1 : 0));
      confirm.appConfirm.mockResolvedValue(false);
      render(<RemoteUsage />);

      const claude = within(await screen.findByRole("group", { name: "Claude 账号" }));
      fireEvent.click(claude.getByRole("button", { name: "切换到工作" }));
      await waitFor(() => expect(api.getAccounts).toHaveBeenCalledTimes(2));
      expect(confirm.appConfirm).toHaveBeenCalledTimes(1);
      expect(api.applyActiveProfileToSessions).not.toHaveBeenCalled();

      const kimi = within(await screen.findByRole("group", { name: "Kimi 账号" }));
      fireEvent.click(kimi.getByRole("button", { name: "切换到工作" }));
      await waitFor(() => expect(api.getAccounts).toHaveBeenCalledTimes(3));
      expect(api.setActiveProfile).toHaveBeenLastCalledWith("kimi", "work");
      expect(confirm.appConfirm).toHaveBeenCalledTimes(1);
      expect(api.applyActiveProfileToSessions).not.toHaveBeenCalled();
    });

    it("切换失败就地报错、不重载；切了但重启失败也报错，账号照样算切了", async () => {
      api.getAccounts.mockResolvedValue([payload("claude")]);
      api.listProfiles.mockResolvedValue(twoProfiles());
      api.setActiveProfile.mockRejectedValueOnce("profile/not_found");
      render(<RemoteUsage />);

      fireEvent.click(await screen.findByRole("button", { name: "切换到工作" }));
      expect(await screen.findByRole("alert")).toBeTruthy();
      expect(api.getAccounts).toHaveBeenCalledTimes(1);
      expect(api.sessionsOffActiveProfileCount).not.toHaveBeenCalled();

      api.sessionsOffActiveProfileCount.mockResolvedValue(1);
      api.applyActiveProfileToSessions.mockRejectedValueOnce("terminal/restart_failed");
      fireEvent.click(screen.getByRole("button", { name: "切换到工作" }));
      await waitFor(() => expect(api.getAccounts).toHaveBeenCalledTimes(2));
      expect(api.applyActiveProfileToSessions).toHaveBeenCalledTimes(1);
      // 重载后（小节重挂载）错误仍在
      expect(await screen.findByRole("alert")).toBeTruthy();
      expect(api.setActiveProfile).toHaveBeenCalledTimes(2);
    });

    it("账号列表读取失败有说法，用量照常显示", async () => {
      api.getAccounts.mockResolvedValue([payload("claude", { usage: usage(30) })]);
      api.listProfiles.mockRejectedValue("io");
      render(<RemoteUsage />);
      expect((await screen.findByRole("alert")).textContent).toContain("账号列表读取失败");
      expect(screen.getByRole("progressbar", { name: "5 小时配额" })).toBeTruthy();
    });
  });
});
