import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup, act } from "@testing-library/react";

// vi.mock 会被提升到文件顶部，工厂函数里引用的外部变量必须走 vi.hoisted
// （否则 TDZ：ReferenceError: Cannot access 'api' before initialization）。
const api = vi.hoisted(() => ({
  newSession: vi.fn(),
  recentCwds: vi.fn(),
  checkProviderHooks: vi.fn(),
  availableTerminals: vi.fn(),
  getSettings: vi.fn(),
  listAgents: vi.fn(),
  getAccounts: vi.fn(),
  loginAgent: vi.fn(),
  cancelLogin: vi.fn(),
}));
vi.mock("../api", async (orig) => ({ ...(await orig<typeof import("../api")>()), ...api }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
const closeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ close: closeMock }) }));
// 收集 login-done 回调，测试里手动广播（模拟 Tauri emit）。其余事件（ns-prefill）照常返回 unlisten。
const ev = vi.hoisted(() => ({ loginCbs: [] as Array<(e: unknown) => void> }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e: unknown) => void) => {
    if (event === "login-done") ev.loginCbs.push(cb);
    return Promise.resolve(() => {});
  },
}));
const fireLogin = (provider: string, ok: boolean) =>
  act(() => ev.loginCbs.forEach((cb) => cb({ payload: { provider, ok } })));

import { NewSessionPanel } from "./NewSessionPanel";
import { descriptors } from "../test/agents";
import { zh } from "../i18n/zh";

/**
 * 当前是否处于「等待登录」态。
 *
 * 判据是按钮文案而非 `disabled`：等待中按钮不再禁用，而是变成「取消等待」——终端可能已被
 * 关掉（手动关、崩溃、agent 自己退出），而后端只轮询账号文件，要 5 分钟才超时。
 */
const waiting = () => screen.getByTestId("ns-login").textContent === zh.newSession.cancelLogin;

beforeEach(() => {
  Object.values(api).forEach((m) => m.mockReset());
  closeMock.mockReset();
  ev.loginCbs.length = 0;
  api.recentCwds.mockResolvedValue([]);
  api.checkProviderHooks.mockResolvedValue("installed");
  api.availableTerminals.mockResolvedValue(["wt"]);
  api.getSettings.mockResolvedValue({ default_agent: "claude", resume_terminal: "wt" });
  api.listAgents.mockResolvedValue(descriptors(["claude", "codex", "kimi"]));
  // 默认三家都已登录 → 不显示登录提示（各测试按需覆盖）。
  api.getAccounts.mockResolvedValue([
    { provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true },
    { provider: "codex", account: { login_label: "API Key" }, usage: null, usage_supported: false },
    { provider: "kimi", account: { email: "k@b.c" }, usage: null, usage_supported: true },
  ]);
});
afterEach(() => cleanup());

describe("NewSessionPanel (独立窗口)", () => {
  it("目录为空时启动禁用", async () => {
    render(<NewSessionPanel />);
    const launch = await screen.findByTestId("ns-launch");
    expect((launch as HTMLButtonElement).disabled).toBe(true);
  });

  it("填目录后启动调 newSession → 关窗", async () => {
    api.newSession.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.change(await screen.findByTestId("ns-dir"), { target: { value: "C:/proj" } });
    fireEvent.click(screen.getByTestId("ns-launch"));
    await waitFor(() => expect(api.newSession).toHaveBeenCalledWith("C:/proj", "claude"));
    await waitFor(() => expect(closeMock).toHaveBeenCalled());
  });

  it("同一事件批次重复点击只启动一个终端", async () => {
    let finish!: () => void;
    api.newSession.mockImplementation(() => new Promise<void>((resolve) => { finish = resolve; }));
    render(<NewSessionPanel />);
    fireEvent.change(await screen.findByTestId("ns-dir"), { target: { value: "C:/proj" } });
    const launch = screen.getByTestId("ns-launch");

    act(() => {
      launch.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      launch.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(api.newSession).toHaveBeenCalledTimes(1);
    finish();
    await waitFor(() => expect(closeMock).toHaveBeenCalled());
  });

  it("hooks 未装显示警告", async () => {
    api.checkProviderHooks.mockResolvedValue("missing");
    render(<NewSessionPanel />);
    expect(await screen.findByTestId("ns-hooks-warn")).toBeTruthy();
  });

  it("启动失败显示错误，不关窗", async () => {
    api.newSession.mockRejectedValue("启动终端失败");
    render(<NewSessionPanel />);
    fireEvent.change(await screen.findByTestId("ns-dir"), { target: { value: "C:/proj" } });
    fireEvent.click(screen.getByTestId("ns-launch"));
    expect((await screen.findByTestId("ns-error")).textContent).toContain("启动终端失败");
    expect(closeMock).not.toHaveBeenCalled();
  });

  it("agent 选择只列已装的", async () => {
    api.listAgents.mockResolvedValue(descriptors(["claude", "codex"]));
    render(<NewSessionPanel />);
    await screen.findByTestId("ns-launch");
    expect(screen.queryByTestId("ns-agent-claude")).toBeTruthy();
    expect(screen.queryByTestId("ns-agent-codex")).toBeTruthy();
    expect(screen.queryByTestId("ns-agent-kimi")).toBeNull();
  });

  it("一个都没装时提示 + 启动禁用", async () => {
    api.listAgents.mockResolvedValue(descriptors([]));
    render(<NewSessionPanel />);
    expect(await screen.findByTestId("ns-no-agents")).toBeTruthy();
    expect((screen.getByTestId("ns-launch") as HTMLButtonElement).disabled).toBe(true);
  });

  it("输入斜杠与最近项反斜杠方向不同时仍能高亮匹配", async () => {
    api.recentCwds.mockResolvedValue(["C:\\Users\\larry\\proj"]);
    const { container } = render(<NewSessionPanel />);
    fireEvent.change(await screen.findByTestId("ns-dir"), { target: { value: "C:/Users/larry/proj" } });
    await waitFor(() =>
      expect(container.querySelector(".ns-recent-item.is-on")?.textContent).toContain("proj")
    );
  });

  it("同一目录因斜杠方向不同重复时只保留一条", async () => {
    api.recentCwds.mockResolvedValue([
      "C:/Users/larry/proj",
      "C:\\Users\\larry\\proj",
      "C:\\Users\\larry\\other",
    ]);
    const { container } = render(<NewSessionPanel />);
    await waitFor(() =>
      expect(container.querySelectorAll(".ns-recent-item").length).toBe(2)
    );
  });
});

describe("NewSessionPanel 登录", () => {
  /** 让当前选中的 claude 处于未登录（account 为 null）。 */
  const claudeSignedOut = () =>
    api.getAccounts.mockResolvedValue([
      { provider: "claude", account: null, usage: null, usage_supported: true },
      { provider: "codex", account: { login_label: "API Key" }, usage: null, usage_supported: false },
      { provider: "kimi", account: { email: "k@b.c" }, usage: null, usage_supported: true },
    ]);

  it("未登录时提示并给出登录按钮", async () => {
    claudeSignedOut();
    render(<NewSessionPanel />);
    expect(await screen.findByTestId("ns-login-warn")).toBeTruthy();
    expect(screen.getByTestId("ns-login")).toBeTruthy();
  });

  it("已登录时不提示", async () => {
    render(<NewSessionPanel />); // beforeEach 里三家都已登录
    await screen.findByTestId("ns-agent-claude");
    expect(screen.queryByTestId("ns-login-warn")).toBeNull();
  });

  it("拿不到账号时不提示（宁可不打扰，也不误报未登录）", async () => {
    api.getAccounts.mockRejectedValue(new Error("boom"));
    render(<NewSessionPanel />);
    await screen.findByTestId("ns-agent-claude");
    expect(screen.queryByTestId("ns-login-warn")).toBeNull();
  });

  /**
   * 「无账号能力」≠「未登录」。
   *
   * 回归：`getAccounts()` 只返回**声明了账号能力**的 agent（后端遍历 all_with_account）。曾把
   * 「查不到行」当成「未登录」，于是给没有账号能力的 agent 也亮出登录按钮——点下去后端
   * `login_argv()` 是 None，只能报「拉起登录失败」。给出入口却走不通，比没有入口更糟。
   *
   * 两者必须分开：真有账号能力却没登录的（kimi），照旧给入口。当前五家都有账号能力，故这里
   * 手造一个没有的。
   */
  it("无账号能力的 agent 不给登录入口；真未登录的照给", async () => {
    api.listAgents.mockResolvedValue([
      { id: "claude", display_name: "Claude Code", installed: true, supports_proxy: true, supports_account: true, supports_profiles: true },
      { id: "kimi", display_name: "Kimi Code", installed: true, supports_proxy: true, supports_account: true, supports_profiles: true },
      { id: "noacct", display_name: "No Account", installed: true, supports_proxy: false, supports_account: false, supports_profiles: false },
    ]);
    api.getAccounts.mockResolvedValue([
      { provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true },
      // 有账号能力，但确实没登录 → 该给入口。
      { provider: "kimi", account: null, usage: null, usage_supported: true },
      // noacct 不在返回里——它没声明账号能力，后端根本不会列它。
    ]);
    render(<NewSessionPanel />);

    // kimi：真未登录 → 亮出入口。
    fireEvent.click(await screen.findByTestId("ns-agent-kimi"));
    await waitFor(() => expect(screen.queryByTestId("ns-login")).toBeTruthy());

    // noacct：无账号概念 → 不该亮（曾经会亮，且点了必失败）。
    fireEvent.click(screen.getByTestId("ns-agent-noacct"));
    await waitFor(() => expect(screen.queryByTestId("ns-login-warn")).toBeNull());
    expect(screen.queryByTestId("ns-login")).toBeNull();
  });

  it("点登录调 loginAgent 并进入等待态", async () => {
    claudeSignedOut();
    api.loginAgent.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.click(await screen.findByTestId("ns-login"));
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalledWith("claude"));
    // 等 login-done 才落回。按钮此时变成「取消等待」，而不是死掉的禁用按钮——终端可能已被关掉，
    // 而后端只轮询账号文件，要 5 分钟才超时。
    await waitFor(() => expect(waiting()).toBe(true));
  });

  it("login-done 成功 → 提示消失", async () => {
    claudeSignedOut();
    api.loginAgent.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.click(await screen.findByTestId("ns-login"));
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalled());
    fireLogin("claude", true);
    await waitFor(() => expect(screen.queryByTestId("ns-login-warn")).toBeNull());
  });

  /// 起因：点完登录后如果终端被关掉（用户手动关、崩溃、agent 自己退出），后端只轮询账号文件，
  /// 要 5 分钟才超时。这五分钟里按钮一直是「等待登录…」且不可点——用户既不能重来也不知道发生了什么。
  it("等待中点按钮 → 调 cancel_login，落回可点并提示已取消（而非「未检测到登录完成」）", async () => {
    claudeSignedOut();
    api.loginAgent.mockResolvedValue(undefined);
    api.cancelLogin.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.click(await screen.findByTestId("ns-login"));
    await waitFor(() => expect(waiting()).toBe(true));

    fireEvent.click(screen.getByTestId("ns-login"));
    await waitFor(() => expect(api.cancelLogin).toHaveBeenCalledWith("claude"));

    // 收尾由后端 emit login-done（它会再查一次账号；这里模拟「确实没登上」）。
    fireLogin("claude", false);
    await waitFor(() => expect(waiting()).toBe(false));
    // 取消 ≠ 超时：文案必须区分，否则用户以为是没检测到。
    expect(screen.getByTestId("ns-error").textContent).toBe(zh.newSession.loginCancelled);
  });

  /// 取消时后端会再查一次账号——用户可能已经在终端里登完了，只是嫌等得慢。此时应转「已登录」。
  it("取消时若其实已登录成功 → 未登录提示消失，不显示取消提示", async () => {
    claudeSignedOut();
    api.loginAgent.mockResolvedValue(undefined);
    api.cancelLogin.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.click(await screen.findByTestId("ns-login"));
    await waitFor(() => expect(waiting()).toBe(true));
    fireEvent.click(screen.getByTestId("ns-login"));
    await waitFor(() => expect(api.cancelLogin).toHaveBeenCalled());

    fireLogin("claude", true); // 后端复查发现真登上了
    await waitFor(() => expect(screen.queryByTestId("ns-login-warn")).toBeNull());
    expect(screen.queryByTestId("ns-error")).toBeNull();
  });

  it("login-done 超时 → 落回可点 + 显示提示，提示仍在", async () => {
    claudeSignedOut();
    api.loginAgent.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.click(await screen.findByTestId("ns-login"));
    await waitFor(() => expect(waiting()).toBe(true));
    fireLogin("claude", false);
    await waitFor(() => expect(waiting()).toBe(false));
    // 超时不等于登录失败，未登录提示仍在，错误行给出本地化说明。
    expect(screen.getByTestId("ns-login-warn")).toBeTruthy();
    expect(screen.getByTestId("ns-error").textContent?.trim().length).toBeGreaterThan(0);
  });

  it("别的 provider 的 login-done 不影响当前选中项", async () => {
    claudeSignedOut();
    api.loginAgent.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.click(await screen.findByTestId("ns-login"));
    await waitFor(() => expect(waiting()).toBe(true));
    fireLogin("kimi", true); // 与当前选中的 claude 无关
    expect(waiting()).toBe(true);
    expect(screen.getByTestId("ns-login-warn")).toBeTruthy();
  });

  /** 两家都未登录，用于「登录中切走」的回归。 */
  const bothSignedOut = () =>
    api.getAccounts.mockResolvedValue([
      { provider: "claude", account: null, usage: null, usage_supported: true },
      { provider: "codex", account: { login_label: "API Key" }, usage: null, usage_supported: false },
      { provider: "kimi", account: null, usage: null, usage_supported: true },
    ]);

  it("登录中切到别的 agent：新 agent 的登录按钮可点，且能真的发起登录", async () => {
    // 回归：等待态曾是全局 boolean，切走后 claude 的 login-done 被按当前选中项过滤掉、
    // 等待态永远落不回 → kimi 的按钮虽显示可点，doLogin 却被 `if (loginBusy) return` 静默挡住。
    bothSignedOut();
    api.loginAgent.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.click(await screen.findByTestId("ns-login")); // 发起 claude 登录
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalledWith("claude"));
    await waitFor(() => expect(waiting()).toBe(true));

    fireEvent.click(screen.getByTestId("ns-agent-kimi")); // claude 还在登录中就切走
    // kimi 未登录且不在等待态 → 按钮可点
    await waitFor(() => expect(waiting()).toBe(false));
    fireEvent.click(screen.getByTestId("ns-login"));
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalledWith("kimi")); // 并发登录不被挡
    await waitFor(() => expect(waiting()).toBe(true));
  });

  it("登录中切走后，原 agent 的 login-done 仍能清掉它的等待态", async () => {
    bothSignedOut();
    api.loginAgent.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.click(await screen.findByTestId("ns-login")); // claude 登录中
    await waitFor(() => expect(waiting()).toBe(true));

    fireEvent.click(screen.getByTestId("ns-agent-kimi")); // 切走
    fireLogin("claude", false); // claude 登录超时（此时选中的是 kimi）
    fireEvent.click(screen.getByTestId("ns-agent-claude")); // 切回来

    // 等待态已被清掉 → 可以重试登录（旧实现在此永久禁用）
    await waitFor(() => expect(waiting()).toBe(false));
  });

  it("切走期间到达的成功事件，切回后应显示为已登录", async () => {
    bothSignedOut();
    api.loginAgent.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.click(await screen.findByTestId("ns-login"));
    fireEvent.click(await screen.findByTestId("ns-agent-kimi")); // 切走
    fireLogin("claude", true); // 登录成功是客观事实，与当前选中谁无关
    fireEvent.click(screen.getByTestId("ns-agent-claude")); // 切回来
    await waitFor(() => expect(screen.queryByTestId("ns-login-warn")).toBeNull());
  });
});
