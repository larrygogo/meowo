import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor, act } from "@testing-library/react";

// vi.mock 会被提升到文件顶部，工厂函数里引用的外部变量必须走 vi.hoisted
// （否则 TDZ：ReferenceError: Cannot access 'api' before initialization，与 NewSessionPanel.test.tsx 同坑）。
const api = vi.hoisted(() => ({ getAccounts: vi.fn(), listAgents: vi.fn(), installAgent: vi.fn(), loginAgent: vi.fn(), cancelLogin: vi.fn(), checkProviderHooks: vi.fn(), refreshUsage: vi.fn(), getSettings: vi.fn(), setSettings: vi.fn(), agentPathGap: vi.fn(), addAgentToUserPath: vi.fn() }));
vi.mock("../api", async (o) => ({ ...(await o<typeof import("../api")>()), ...api }));

// 收集所有 ProviderCard 注册的 install-done / login-done 回调，测试里手动广播
// （模拟 Tauri emit 到全部监听者）。进度不透传英文、前端不订阅 install-progress，故只收集 done。
const ev = vi.hoisted(() => ({
  doneCbs: [] as Array<(e: unknown) => void>,
  loginCbs: [] as Array<(e: unknown) => void>,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e: unknown) => void) => {
    if (event === "install-done") ev.doneCbs.push(cb);
    if (event === "login-done") ev.loginCbs.push(cb);
    return Promise.resolve(() => {});
  },
}));
const fireDone = (provider: string, ok: boolean, logPath: string | null = null) =>
  act(() => ev.doneCbs.forEach((cb) => cb({ payload: { provider, ok, code: ok ? 0 : 1, logPath } })));
const fireLogin = (provider: string, ok: boolean) =>
  act(() => ev.loginCbs.forEach((cb) => cb({ payload: { provider, ok } })));

import { AccountSection } from "./About";
import { descriptors } from "../test/agents";
import { zh } from "../i18n/zh";

beforeEach(() => {
  Object.values(api).forEach((m) => m.mockReset());
  api.getAccounts.mockResolvedValue([{ provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true }]);
  api.listAgents.mockResolvedValue(descriptors(["claude", "codex"]));
  api.refreshUsage.mockResolvedValue({ lanes: [], note: null });
  api.getSettings.mockResolvedValue({ sticker_quota_providers: [] });
  // 默认：bin 目录都在 PATH 上（无提示条），个别用例再覆盖。
  api.agentPathGap.mockResolvedValue(null);
  api.addAgentToUserPath.mockResolvedValue(undefined);
  // 默认 hooks 已接入（无「未接入」提示条），验接线的用例再覆盖。
  api.checkProviderHooks.mockResolvedValue("installed");
  ev.doneCbs.length = 0;
  ev.loginCbs.length = 0;
});
afterEach(() => cleanup());

describe("AccountSection agent 卡", () => {
  it("三个 agent 都渲染，未装的标未安装 + 安装按钮", async () => {
    render(<AccountSection />);
    await waitFor(() => expect(screen.getByTestId("agent-card-kimi")).toBeTruthy());
    expect(screen.getByTestId("agent-card-claude")).toBeTruthy();
    expect(screen.getByTestId("agent-card-codex")).toBeTruthy();
    // kimi 未装：availableAgents() resolve 后才出现安装按钮（首帧检测中不渲染，findByTestId 等待）
    expect(await screen.findByTestId("agent-install-kimi")).toBeTruthy();
    // 已装的（claude/codex）无安装按钮
    expect(screen.queryByTestId("agent-install-claude")).toBeNull();
  });

  it("点安装调 installAgent", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(api.installAgent).toHaveBeenCalledWith("kimi"));
  });

  it("点安装进入安装中：转圈 + 本地化「安装中…」（不透传脚本英文）", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    // 安装中指示出现，且有非空本地化文案（不硬编码 i18n 串，避免 locale 依赖）
    const installing = await screen.findByTestId("agent-installing-kimi");
    expect(installing.textContent?.trim().length).toBeGreaterThan(0);
    // 安装按钮已被安装中指示替换
    expect(screen.queryByTestId("agent-install-kimi")).toBeNull();
  });

  it("install-done 成功后重查检测、卡片转已装", async () => {
    api.installAgent.mockResolvedValue(undefined);
    // 初次未装 kimi；装完重查返回含 kimi
    api.listAgents.mockResolvedValueOnce(descriptors(["claude", "codex"])).mockResolvedValue(descriptors(["claude", "codex", "kimi"]));
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(screen.getByTestId("agent-installing-kimi")).toBeTruthy());
    fireDone("kimi", true);
    await waitFor(() => expect(screen.queryByTestId("agent-install-kimi")).toBeNull());
    expect(screen.queryByTestId("agent-installing-kimi")).toBeNull();
  });

  it("install-done 失败：退出安装中、显示重试按钮", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(screen.getByTestId("agent-installing-kimi")).toBeTruthy());
    fireDone("kimi", false);
    await waitFor(() => expect(screen.queryByTestId("agent-installing-kimi")).toBeNull());
    // 仍未装 → 按钮回来（文案为重试），testid 不变
    expect(screen.getByTestId("agent-install-kimi")).toBeTruthy();
  });

  // 已登录的卡片只显示邮箱。此前拼 显示名 · 邮箱 · 组织，个人账号的组织名恰是
  // 「<邮箱>'s Organization」，于是同一个邮箱在一行里出现两次，又长又重复。
  it("已登录只显示邮箱，不带显示名与组织", async () => {
    api.getAccounts.mockResolvedValue([
      { provider: "claude", account: { email: "a@b.c", display_name: "Larry", organization: "a@b.c's Organization" }, usage: null, usage_supported: true },
    ]);
    render(<AccountSection />);
    const desc = await screen.findByTestId("agent-desc-claude");
    expect(desc.textContent).toBe("a@b.c");
  });

  // 没有邮箱的登录方式（如 codex 的 API key）不能让描述行变空白。
  it("无邮箱时回退到显示名/登录标签", async () => {
    api.getAccounts.mockResolvedValue([
      { provider: "claude", account: { login_label: "API key" }, usage: null, usage_supported: true },
    ]);
    render(<AccountSection />);
    const desc = await screen.findByTestId("agent-desc-claude");
    expect(desc.textContent).toBe("API key");
  });

  // 回归：此前安装输出被丢进 Stdio::null()，失败时用户拿不到任何可排查的东西。
  it("install-done 失败：给出安装日志路径（有 logPath 时）", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    fireDone("kimi", false, "C:\\Users\\x\\.meowo\\install-kimi.log");
    const log = await screen.findByTestId("agent-install-log-kimi");
    expect(log.textContent).toContain("install-kimi.log");
  });

  // 回归：claude 的安装器不写 PATH 也照样 exit 0——「装好了」不等于「终端里敲得出来」。
  // 提示条须在挂载时就查（多数受害者是早就装好、从没进过 PATH 的用户），不能只在装完时查。
  it("已装但 bin 目录不在 PATH：显示提示条，可一键写入", async () => {
    api.agentPathGap.mockResolvedValue("C:\\Users\\x\\.local\\bin");
    render(<AccountSection />);
    const gap = await screen.findByTestId("agent-path-gap-claude");
    expect(gap.textContent).toContain(".local\\bin");

    fireEvent.click(screen.getByTestId("agent-add-path-claude"));
    await waitFor(() => expect(api.addAgentToUserPath).toHaveBeenCalledWith("claude"));
    // 写入成功 → 提示条消失，转为「请重开终端」提示
    await waitFor(() => expect(screen.queryByTestId("agent-path-gap-claude")).toBeNull());
    expect(screen.getByTestId("agent-path-msg-claude")).toBeTruthy();
  });

  it("写入 PATH 失败：保留提示条并给出失败说明", async () => {
    api.agentPathGap.mockResolvedValue("C:\\Users\\x\\.local\\bin");
    api.addAgentToUserPath.mockRejectedValue(new Error("denied"));
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-add-path-claude"));
    await waitFor(() => expect(screen.getByTestId("agent-path-msg-claude")).toBeTruthy());
    // 没写成功 → 提示条不该消失，否则用户以为已修好
    expect(screen.getByTestId("agent-path-gap-claude")).toBeTruthy();
  });

  /// 后端在**跑脚本之前**就失败（引导脚本被 Cloudflare 人机校验拦截）：错误串是我们自己的中文
  /// 诊断，必须原样显示。此时还没有日志文件——旧代码 `.catch(() => setInstallState("error"))`
  /// 把它整个丢掉，用户只看到一句通用的「安装失败」，一点线索都没有。
  it("installAgent 直接 reject：显示后端诊断，而不是通用文案", async () => {
    const diag = "https://claude.ai/install.ps1 返回了 Cloudflare 人机校验页，而不是安装脚本。";
    api.installAgent.mockRejectedValue(diag);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    const errLine = await screen.findByTestId("agent-install-error-kimi");
    expect(errLine.textContent).toContain("Cloudflare");
    // 没有日志文件（脚本压根没跑），故不显示日志路径行。
    expect(screen.queryByTestId("agent-install-log-kimi")).toBeNull();
  });

  /// 两种失败不能串台：脚本真的跑了再失败时，该给通用文案 + 日志路径，而不是上一次的诊断。
  it("先被 CF 拦、重试后脚本跑起来又失败：诊断被清掉，改显示日志路径", async () => {
    api.installAgent.mockRejectedValueOnce("被 Cloudflare 人机校验拦截").mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(screen.getByTestId("agent-install-error-kimi").textContent).toContain("Cloudflare"));

    fireEvent.click(screen.getByTestId("agent-install-kimi")); // 重试
    fireDone("kimi", false, "C:\\Users\\me\\.meowo\\install-kimi.log");
    await waitFor(() => expect(screen.getByTestId("agent-install-log-kimi")).toBeTruthy());
    expect(screen.getByTestId("agent-install-error-kimi").textContent).not.toContain("Cloudflare");
  });

  it("install-done 失败：显示本地化失败说明 + 重试按钮", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(screen.getByTestId("agent-installing-kimi")).toBeTruthy());
    // 失败——错误行始终显示本地化 installFailed 文案
    fireDone("kimi", false);
    await waitFor(() => expect(screen.queryByTestId("agent-installing-kimi")).toBeNull());
    // 失败说明行可见且非空（不硬编码 i18n 文案，避免 locale 依赖）
    const errLine = screen.getByTestId("agent-install-error-kimi");
    expect(errLine.textContent?.trim().length).toBeGreaterThan(0);
    // 重试按钮仍在（testid 与安装共用）
    expect(screen.getByTestId("agent-install-kimi")).toBeTruthy();
  });
});

describe("AccountSection 登录", () => {
  it("已装未登录才显示登录按钮；已登录/未装都不显示", async () => {
    render(<AccountSection />);
    // codex 已装（availableAgents）但 getAccounts 没返回它 → account 为 null → 未登录。
    expect(await screen.findByTestId("agent-login-codex")).toBeTruthy();
    // claude 已装且有账号 → 已登录，无登录按钮。
    expect(screen.queryByTestId("agent-login-claude")).toBeNull();
    // kimi 未装 → 该先安装，不给登录按钮。
    expect(screen.queryByTestId("agent-login-kimi")).toBeNull();
  });

  it("点登录调 loginAgent 并进入等待态", async () => {
    api.loginAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    const btn = await screen.findByTestId("agent-login-codex");
    fireEvent.click(btn);
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalledWith("codex"));
    // spawn 成功后不落回 idle——等 login-done。按钮此时变成「取消等待」，而不是一个死掉的禁用按钮：
    // 终端可能已被关掉，而后端要 5 分钟才超时。
    await waitFor(() => expect(screen.getByTestId("agent-login-codex").textContent).toBe(zh.account.cancelLogin));
    expect((screen.getByTestId("agent-login-codex") as HTMLButtonElement).disabled).toBe(false);
  });

  it("login-done 成功 → 重查账号（卡片可转已登录）", async () => {
    api.loginAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-login-codex"));
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalled());
    const before = api.getAccounts.mock.calls.length;
    // 登录成功后 codex 也有账号了
    api.getAccounts.mockResolvedValue([
      { provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true },
      { provider: "codex", account: { login_label: "API Key" }, usage: null, usage_supported: false },
    ]);
    fireLogin("codex", true);
    await waitFor(() => expect(api.getAccounts.mock.calls.length).toBeGreaterThan(before));
    await waitFor(() => expect(screen.queryByTestId("agent-login-codex")).toBeNull());
  });

  it("login-done 超时 → 落回可点 + 显示本地化提示（超时不是登录失败）", async () => {
    api.loginAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-login-codex"));
    await waitFor(() => expect(screen.getByTestId("agent-login-codex").textContent).toBe(zh.account.cancelLogin));
    fireLogin("codex", false);
    await waitFor(() => expect(screen.getByTestId("agent-login-codex").textContent).toBe(zh.account.login));
    expect(screen.getByTestId("agent-login-error-codex").textContent).toBe(zh.account.loginTimeout);
  });

  /// 起因：点完登录后如果终端被关掉（用户手动关、崩溃、agent 自己退出），后端只轮询账号文件，
  /// 要 5 分钟才超时。这五分钟里按钮一直是「等待登录…」且不可点——用户既不能重来也不知道发生了什么。
  it("等待中点按钮 → 调 cancel_login，落回可点并提示已取消（而非「未检测到登录完成」）", async () => {
    api.loginAgent.mockResolvedValue(undefined);
    api.cancelLogin.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-login-codex"));
    await waitFor(() => expect(screen.getByTestId("agent-login-codex").textContent).toBe(zh.account.cancelLogin));

    fireEvent.click(screen.getByTestId("agent-login-codex"));
    await waitFor(() => expect(api.cancelLogin).toHaveBeenCalledWith("codex"));

    // 收尾由后端 emit login-done（它会再查一次账号；这里模拟「确实没登上」）。
    fireLogin("codex", false);
    await waitFor(() => expect(screen.getByTestId("agent-login-codex").textContent).toBe(zh.account.login));
    // 取消 ≠ 超时：文案必须区分，否则用户以为是没检测到。
    expect(screen.getByTestId("agent-login-error-codex").textContent).toBe(zh.account.loginCancelled);
  });

  /// 取消时后端会再查一次账号——用户可能已经在终端里登完了，只是嫌等得慢。此时应转「已登录」。
  it("取消时若其实已登录成功 → 转已登录，不显示取消提示", async () => {
    api.loginAgent.mockResolvedValue(undefined);
    api.cancelLogin.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-login-codex"));
    await waitFor(() => expect(screen.getByTestId("agent-login-codex").textContent).toBe(zh.account.cancelLogin));
    fireEvent.click(screen.getByTestId("agent-login-codex"));
    await waitFor(() => expect(api.cancelLogin).toHaveBeenCalled());

    api.getAccounts.mockResolvedValue([
      { provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true },
      { provider: "codex", account: { login_label: "API Key" }, usage: null, usage_supported: false },
    ]);
    fireLogin("codex", true); // 后端复查发现真登上了
    await waitFor(() => expect(screen.queryByTestId("agent-login-codex")).toBeNull());
    expect(screen.queryByTestId("agent-login-error-codex")).toBeNull();
  });

  /// 装完 / 登录后，后端会顺手接线（best-effort），前端据此重查一次 hooks 状态——接上了就让
  /// 「未接入」提示条自动消失，不必让用户再去点「修复连接」。
  it("登录成功后重查 hooks：接上了则「未接入」提示条消失", async () => {
    // codex 已装，但 hooks 初始未接入；登录后端接线 → codex 的第二次查询返回 installed。
    // 按 provider 计数（mockResolvedValueOnce 是全局队列，claude 的挂载查询会抢走它）。
    let codexChecks = 0;
    api.checkProviderHooks.mockImplementation((p: string) => {
      if (p !== "codex") return Promise.resolve("installed");
      codexChecks += 1;
      return Promise.resolve(codexChecks === 1 ? "missing" : "installed");
    });
    api.getAccounts.mockResolvedValue([
      { provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true },
      { provider: "codex", account: null, usage: null, usage_supported: false }, // 未登录
    ]);
    api.loginAgent.mockResolvedValue(undefined);
    render(<AccountSection />);

    // 未接入 → 提示条 + 修复按钮在。
    expect(await screen.findByTestId("agent-repair-hooks-codex")).toBeTruthy();

    fireEvent.click(screen.getByTestId("agent-login-codex"));
    fireLogin("codex", true);
    // 登录成功 → 重查 hooks 得 installed → 提示条消失。
    await waitFor(() => expect(screen.queryByTestId("agent-repair-hooks-codex")).toBeNull());
  });

  it("拉起登录失败 → 落回可点 + 显示提示", async () => {
    api.loginAgent.mockRejectedValue(new Error("启动终端失败"));
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-login-codex"));
    await waitFor(() => expect(screen.getByTestId("agent-login-error-codex")).toBeTruthy());
    expect((screen.getByTestId("agent-login-codex") as HTMLButtonElement).disabled).toBe(false);
  });

  it("装完自动引导：install-done 成功后，该 agent 的登录按钮被标为主要动作", async () => {
    api.installAgent.mockResolvedValue(undefined);
    // 初次未装 kimi；装完重查返回含 kimi（此时 kimi 无账号 → 未登录）
    api.listAgents.mockResolvedValueOnce(descriptors(["claude", "codex"])).mockResolvedValue(descriptors(["claude", "codex", "kimi"]));
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    fireDone("kimi", true);
    const loginBtn = await screen.findByTestId("agent-login-kimi");
    // 「装完 → 登录」串成一条链路：按钮升为 primary，而非埋在一堆次要按钮里。
    expect(loginBtn.className).toContain("provider-card-action-primary");
    // 未经安装流程的 codex 则是普通次要按钮。
    expect(screen.getByTestId("agent-login-codex").className).not.toContain("provider-card-action-primary");
  });
});
