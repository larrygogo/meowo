import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invokeMock(...a) }));

import { loginAgent, installAgent, isLoggedIn, type ProviderAccountPayload } from "./api";

beforeEach(() => invokeMock.mockReset());

describe("login api", () => {
  // 命令名拼错时组件测试仍会通过（它们 mock 掉了整个 api 模块），只有这里能拦住。
  it("loginAgent 调用 login_agent 并透传 provider/terminal", () => {
    invokeMock.mockResolvedValue(undefined);
    loginAgent("claude", "wt");
    expect(invokeMock).toHaveBeenCalledWith("login_agent", {
      provider: "claude",
      terminal: "wt",
      profile: null, // 省略 = 当前活跃账号
    });
  });

  /**
   * 多账号：登录**必须**能指定登进哪个账号——凭据会写进那个账号自己的目录。
   * 漏传的话，新账号的登录就把默认账号的凭据覆盖了：用户以为加了个账号，其实是把原来那个换掉了。
   */
  it("loginAgent 透传 profile（决定凭据写进哪个账号的目录）", () => {
    invokeMock.mockResolvedValue(undefined);
    loginAgent("claude", "wt", "work");
    expect(invokeMock).toHaveBeenCalledWith("login_agent", {
      provider: "claude",
      terminal: "wt",
      profile: "work",
    });
  });

  it("loginAgent 省略 terminal 时传 undefined（后端回退设置里的默认终端）", () => {
    invokeMock.mockResolvedValue(undefined);
    loginAgent("kimi");
    expect(invokeMock).toHaveBeenCalledWith("login_agent", {
      provider: "kimi",
      terminal: undefined,
      profile: null,
    });
  });

  it("installAgent 调用 install_agent", () => {
    invokeMock.mockResolvedValue(undefined);
    installAgent("codex");
    expect(invokeMock).toHaveBeenCalledWith("install_agent", { provider: "codex" });
  });
});

describe("isLoggedIn", () => {
  const payload = (account: ProviderAccountPayload["account"]): ProviderAccountPayload => ({
    provider: "claude",
    account,
    usage: null,
    usage_supported: true,
  });

  it("能解析出账号即已登录（三家判据各异，已在后端 account() 内收敛）", () => {
    expect(isLoggedIn(payload({ email: "a@b.c" } as ProviderAccountPayload["account"]))).toBe(true);
    // codex 的 API Key 登录没有 email，只有 login_label——同样算已登录。
    expect(isLoggedIn(payload({ login_label: "API Key" } as ProviderAccountPayload["account"]))).toBe(true);
  });

  it("account 为 null 或 payload 缺失 → 未登录", () => {
    expect(isLoggedIn(payload(null))).toBe(false);
    expect(isLoggedIn(undefined)).toBe(false);
  });
});
