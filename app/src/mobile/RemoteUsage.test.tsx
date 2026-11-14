import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderAccountPayload, ProviderUsage } from "../api";

const api = vi.hoisted(() => ({
  getAccounts: vi.fn(),
  refreshUsage: vi.fn(),
}));
vi.mock("../api", async (importOriginal) => ({ ...(await importOriginal<typeof import("../api")>()), ...api }));

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

describe("RemoteUsage", () => {
  beforeEach(() => {
    api.getAccounts.mockResolvedValue([]);
    api.refreshUsage.mockResolvedValue(usage(60));
  });
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("先展示缓存的全部泳道、余额、重置时间和备注，不自动联网刷新", async () => {
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
      payload("gemini", { account: null }),
      payload("kimi", { usage_supported: false }),
      payload("opencode", { relay_enabled: true }),
    ]);
    render(<RemoteUsage />);

    expect((await screen.findByRole("region", { name: "Gemini" })).textContent).toContain("未登录");
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
});
