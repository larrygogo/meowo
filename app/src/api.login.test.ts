import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invokeMock(...a) }));

import { getRelaySecrets, loginAgent, logoutAgent, installAgent, isLoggedIn, listRelayModels, type ProviderAccountPayload } from "./api";

beforeEach(() => invokeMock.mockReset());

describe("login api", () => {
  // 命令名拼错时组件测试仍会通过（它们 mock 掉了整个 api 模块），只有这里能拦住。
  it("loginAgent 调用 login_agent 并透传 provider/terminal", () => {
    invokeMock.mockResolvedValue(undefined);
    loginAgent("claude", "wt");
    expect(invokeMock).toHaveBeenCalledWith("login_agent", { provider: "claude", terminal: "wt" });
  });

  it("loginAgent 省略 terminal 时传 undefined（后端回退设置里的默认终端）", () => {
    invokeMock.mockResolvedValue(undefined);
    loginAgent("kimi");
    expect(invokeMock).toHaveBeenCalledWith("login_agent", { provider: "kimi", terminal: undefined });
  });

  it("installAgent 调用 install_agent", () => {
    invokeMock.mockResolvedValue(undefined);
    installAgent("codex");
    expect(invokeMock).toHaveBeenCalledWith("install_agent", { provider: "codex" });
  });

  it("logoutAgent 调用 logout_agent", () => {
    invokeMock.mockResolvedValue(undefined);
    logoutAgent("codex");
    expect(invokeMock).toHaveBeenCalledWith("logout_agent", { provider: "codex" });
  });
});

describe("relay api", () => {
  it("getRelaySecrets 读取本机保存的密钥", () => {
    invokeMock.mockResolvedValue({ codex: "sk-local" });
    getRelaySecrets();
    expect(invokeMock).toHaveBeenCalledWith("get_relay_secrets");
  });

  it("listRelayModels 只传配置元数据，密钥由后端读取", () => {
    invokeMock.mockResolvedValue(["relay-model"]);
    listRelayModels("claude", "https://relay.example/v1", "", "api_key");
    expect(invokeMock).toHaveBeenCalledWith("list_relay_models", {
      agent: "claude",
      baseUrl: "https://relay.example/v1",
      protocol: "",
      auth: "api_key",
    });
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
