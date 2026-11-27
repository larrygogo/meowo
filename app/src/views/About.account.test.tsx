import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor, act } from "@testing-library/react";

// vi.mock 会被提升到文件顶部，工厂函数里引用的外部变量必须走 vi.hoisted
// （否则 TDZ：ReferenceError: Cannot access 'api' before initialization，与 NewSessionPanel.test.tsx 同坑）。
const api = vi.hoisted(() => ({
  getAccounts: vi.fn(),
  listAgents: vi.fn(),
  installAgent: vi.fn(),
  cancelInstall: vi.fn(),
  loginAgent: vi.fn(),
  cancelLogin: vi.fn(),
  logoutAgent: vi.fn(),
  apiKeyLogin: vi.fn(),
  checkProviderHooks: vi.fn(),
  refreshUsage: vi.fn(),
  refreshProfileUsage: vi.fn(),
  getSettings: vi.fn(),
  setSettings: vi.fn(),
  getRelaySecretStatus: vi.fn(),
  getRelaySecrets: vi.fn(),
  listRelayModels: vi.fn(),
  setRelaySecret: vi.fn(),
  agentPathGap: vi.fn(),
  addAgentToUserPath: vi.fn(),
  openInstallLog: vi.fn(),
  checkAgentUpdates: vi.fn(),
  installedAgentVersions: vi.fn(),
  getLiveSessionsPage: vi.fn(),
  // 多账号
  listProfiles: vi.fn(),
  createProfile: vi.fn(),
  setActiveProfile: vi.fn(),
  applyActiveProfileToSessions: vi.fn(),
  sessionsOffActiveProfileCount: vi.fn(),
  renameProfile: vi.fn(),
  deleteProfile: vi.fn(),
  mergeProfileIntoDefault: vi.fn(),
  // 本机可用终端：登录等待文案要拿它核对「存值的那个终端还在不在」（7S-1 尾）。
  availableTerminals: vi.fn(),
  // 发图工具没接上的提示里「在文件夹中显示」。
  revealTerminalPath: vi.fn(),
}));
vi.mock("../api", async (o) => ({ ...(await o<typeof import("../api")>()), ...api }));
// 确认框统一走 appConfirm（应用主题原生小窗）；沿用 dialog.confirm 变量名以保住既有断言。
const dialog = vi.hoisted(() => ({ confirm: vi.fn() }));
vi.mock("../confirm", () => ({ appConfirm: dialog.confirm }));

// 收集所有 ProviderCard 注册的 install-done / login-done 回调，测试里手动广播
// （模拟 Tauri emit 到全部监听者）。进度不透传英文、前端不订阅 install-progress，故只收集 done。
const ev = vi.hoisted(() => ({
  doneCbs: [] as Array<(e: unknown) => void>,
  loginCbs: [] as Array<(e: unknown) => void>,
  profileCbs: [] as Array<(e: unknown) => void>,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e: unknown) => void) => {
    if (event === "install-done") ev.doneCbs.push(cb);
    if (event === "login-done") ev.loginCbs.push(cb);
    if (event === "active-profile-changed") ev.profileCbs.push(cb);
    return Promise.resolve(() => {});
  },
}));
const fireDone = (provider: string, ok: boolean, logPath: string | null = null) =>
  act(() => ev.doneCbs.forEach((cb) => cb({ payload: { provider, ok, code: ok ? 0 : 1, logPath, cancelled: false } })));
/** 用户取消：后端 cancel_install 代发的 cancelled install-done（S-8）。 */
const fireCancelled = (provider: string) =>
  act(() => ev.doneCbs.forEach((cb) => cb({ payload: { provider, ok: false, code: null, logPath: null, cancelled: true } })));
const fireLogin = (provider: string, outcome: "success" | "cancelled" | "timeout") => {
  const call = [...api.loginAgent.mock.calls].reverse().find(([p]) => p === provider);
  const operationId = call?.[3] ?? `unrelated-${provider}`;
  act(() => ev.loginCbs.forEach((cb) => cb({ payload: { provider, operationId, outcome } })));
};

/**
 * 顶部模型下拉切到某个 agent（按展示名）——列表现在一次只渲染选中的那一张卡（agent 一多，全部
 * 竖排要滚半天）。默认选中首个已安装的（beforeEach 里是 claude），要断言别家就先切过去。
 */
/// 按**主标签**精确找选项。未装的 agent 在标签后跟一个副文案 span（「未安装」，见 7G-7），
/// 它和标签之间没有空白，可访问名拼出来是「Kimi Code未安装」——纯文本匹配要么落空，
/// 要么退成前缀匹配、把日后新增的 `Kimi Code Pro` 一起放过（复核指出）。
/// 直接认 DOM：.dd-label 的首个文本节点就是 label 本身，副文案是它的子 span。
function findOption(name: string): HTMLElement {
  const hit = screen.getAllByRole("option").find(
    (el) => el.querySelector(".dd-label")?.firstChild?.textContent?.trim() === name,
  );
  if (!hit) throw new Error(`没有主标签为「${name}」的选项`);
  return hit;
}

async function selectAgent(name: string | RegExp) {
  await waitFor(() => expect(document.querySelector(".account-agent-switch .dd-btn")).toBeTruthy());
  fireEvent.click(document.querySelector(".account-agent-switch .dd-btn") as HTMLElement);
  if (typeof name !== "string") {
    fireEvent.click(await screen.findByRole("option", { name }));
    return;
  }
  await waitFor(() => expect(findOption(name)).toBeTruthy());
  fireEvent.click(findOption(name));
}

import { AccountSection, sameAccountName } from "./settings/AccountSection";
import { modelMenuPlacement } from "./settings/RelayAccess";
import { descriptors } from "../test/agents";
import { zh } from "../i18n/zh";

const usageAt = (pct: number) => ({
  lanes: [{ kind: "five_hour" as const, used_pct: pct, used: null, limit: null, unit: null, resets_at: null }],
  note: null,
});

beforeEach(() => {
  // 顶部模型切换记进 localStorage：不清的话，上个用例选过的 agent 会成为下个用例的默认选中卡。
  localStorage.clear();
  Object.values(api).forEach((m) => m.mockReset());
  api.getAccounts.mockResolvedValue([{ provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true }]);
  api.listAgents.mockResolvedValue(descriptors(["claude", "codex"]));
  api.refreshUsage.mockResolvedValue({ lanes: [], note: null });
  api.refreshProfileUsage.mockResolvedValue({ lanes: [], note: null });
  api.getSettings.mockResolvedValue({ sticker_quota_providers: [] });
  api.availableTerminals.mockResolvedValue([]);
  api.setSettings.mockResolvedValue(undefined);
  api.getRelaySecretStatus.mockResolvedValue({ claude: false, codex: false, kimi: false });
  api.getRelaySecrets.mockResolvedValue({});
  api.listRelayModels.mockResolvedValue([]);
  api.setRelaySecret.mockResolvedValue(undefined);
  api.logoutAgent.mockResolvedValue(undefined);
  dialog.confirm.mockReset();
  dialog.confirm.mockResolvedValue(true);
  // 默认：bin 目录都在 PATH 上（无提示条），个别用例再覆盖。
  api.agentPathGap.mockResolvedValue(null);
  api.addAgentToUserPath.mockResolvedValue(undefined);
  // 默认 hooks 已接入（无「未接入」提示条），验接线的用例再覆盖。
  api.checkProviderHooks.mockResolvedValue("installed");
  // 默认：版本探测返回空（无版本行/更新入口），验更新的用例再覆盖。
  api.checkAgentUpdates.mockResolvedValue([]);
  api.installedAgentVersions.mockResolvedValue([]);
  // 默认：无活跃会话（点「更新」不弹确认），有会话的用例再覆盖。
  api.getLiveSessionsPage.mockResolvedValue({ items: [], next_cursor: null });
  // 默认：切账号后没有会话要重启（个别用例再覆盖）。
  api.sessionsOffActiveProfileCount.mockResolvedValue(0);
  api.applyActiveProfileToSessions.mockResolvedValue(0);
  // 默认：只有一个默认账号（没建过自定义账号），登录态跟 getAccounts 当前给的一致（同真实后端：
  // 默认账号的登录态就是 agent 自己目录的登录态）。直接调 mock 的实现，不记一次 getAccounts 调用。
  api.listProfiles.mockImplementation(async (provider: string) => {
    const accounts = ((await api.getAccounts.getMockImplementation()?.()) ?? []) as Array<{ provider: string; account: unknown }>;
    const account = accounts.find((a) => a.provider === provider)?.account ?? null;
    return [{ id: null, name: "", active: true, account, set_aside: [] }];
  });
  ev.doneCbs.length = 0;
  ev.loginCbs.length = 0;
  ev.profileCbs.length = 0;
});
afterEach(() => cleanup());

describe("AccountSection agent 卡", () => {
  it("插件未声明中转能力时不渲染中转入口", async () => {
    api.listAgents.mockResolvedValue(
      descriptors(["claude", "codex"]).map((agent) =>
        agent.id === "claude" ? { ...agent, relay: null } : agent,
      ),
    );
    render(<AccountSection />);
    // claude（默认选中）relay 置空 → 不给中转入口。
    await screen.findByTestId("agent-card-claude");
    expect(screen.queryByTestId("agent-access-claude")).toBeNull();
    // codex 声明了中转 → 切过去应有入口。
    await selectAgent("Codex");
    expect(await screen.findByTestId("agent-access-codex")).toBeTruthy();
  });

  // API Key 登录入口（supports_api_key_login）原由 gemini 声明，随 provider 移除后现役几家都不声明，
  // 对应的两条用例一并删除。

  it("模型菜单在下方空间不足时自动向上展开", () => {
    expect(modelMenuPlacement({ top: 430, bottom: 461 }, 500, 240)).toEqual({
      opensUp: true,
      top: 185,
      maxHeight: 240,
    });
    expect(modelMenuPlacement({ top: 80, bottom: 111 }, 500, 240)).toEqual({
      opensUp: false,
      top: 116,
      maxHeight: 240,
    });
  });

  it("下拉列出全部 agent，一次只渲染选中的那张卡；未装的标未安装 + 安装按钮", async () => {
    render(<AccountSection />);
    // 默认渲染首个已安装的（claude），别家的卡此刻不在 DOM 里——这正是「不用滚」的由来。
    await screen.findByTestId("agent-card-claude");
    expect(screen.queryByTestId("agent-card-codex")).toBeNull();
    expect(screen.queryByTestId("agent-card-kimi")).toBeNull();

    // 下拉里五家都在（含未装的 kimi）。
    fireEvent.click(document.querySelector(".account-agent-switch .dd-btn") as HTMLElement);
    for (const name of ["Claude Code", "Codex", "Kimi Code", "Antigravity CLI", "OpenCode"]) {
      await waitFor(() => expect(findOption(name)).toBeTruthy());
    }

    // 切到 kimi：它未装 → 卡片带安装按钮；此时 claude 卡已从 DOM 撤下。
    fireEvent.click(findOption("Kimi Code"));
    expect(await screen.findByTestId("agent-install-kimi")).toBeTruthy();
    expect(screen.queryByTestId("agent-card-claude")).toBeNull();

    // 切回 claude：已装 → 无安装按钮。
    await selectAgent("Claude Code");
    expect(await screen.findByTestId("agent-card-claude")).toBeTruthy();
    expect(screen.queryByTestId("agent-install-claude")).toBeNull();
  });

  it("点安装调 installAgent", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("Kimi Code");
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(api.installAgent).toHaveBeenCalledWith("kimi"));
  });

  it("点安装进入安装中：转圈 + 本地化「安装中…」（不透传脚本英文）", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("Kimi Code");
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
    await selectAgent("Kimi Code");
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(screen.getByTestId("agent-installing-kimi")).toBeTruthy());
    fireDone("kimi", true);
    await waitFor(() => expect(screen.queryByTestId("agent-install-kimi")).toBeNull());
    expect(screen.queryByTestId("agent-installing-kimi")).toBeNull();
  });

  it("install-done 失败：退出安装中、显示重试按钮", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("Kimi Code");
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(screen.getByTestId("agent-installing-kimi")).toBeTruthy());
    fireDone("kimi", false);
    await waitFor(() => expect(screen.queryByTestId("agent-installing-kimi")).toBeNull());
    // 仍未装 → 按钮回来（文案为重试），testid 不变
    expect(screen.getByTestId("agent-install-kimi")).toBeTruthy();
  });

  // 取消安装（S-8）：安装曾是不可中断的黑盒。点取消 → 后端杀进程/丢弃结果并代发
  // cancelled 的 install-done → 卡片回到「安装」按钮，不按失败展示。
  it("安装中点「取消安装」：调 cancelInstall，cancelled 事件后回到安装按钮", async () => {
    api.installAgent.mockResolvedValue(undefined);
    api.cancelInstall.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("Kimi Code");
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(screen.getByTestId("agent-installing-kimi")).toBeTruthy());
    fireEvent.click(screen.getByTestId("agent-install-cancel-kimi"));
    await waitFor(() => expect(api.cancelInstall).toHaveBeenCalledWith("kimi"));
    // 后端代发的 cancelled install-done：退出安装中、回到安装按钮（不是失败重试态）
    fireCancelled("kimi");
    await waitFor(() => expect(screen.queryByTestId("agent-installing-kimi")).toBeNull());
    expect(screen.getByTestId("agent-install-kimi")).toBeTruthy();
    expect(screen.queryByTestId("agent-install-error-kimi")).toBeNull();
  });

  // 安装状态住在页面级（useInstallOperations）：安装动辄一两分钟，用户会切下拉去看别的
  // agent。状态曾住在 keyed 卡片里，切换即卸载 + install-done 监听注销——回来显示「安装」
  // 按钮诱导二次安装，后台那次若失败原因也永久丢失（登录侧早为同一坑做了页面级提升）。
  it("安装中切去别的 agent 再切回：仍在安装中，不显示安装按钮", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("Kimi Code");
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(screen.getByTestId("agent-installing-kimi")).toBeTruthy());
    await selectAgent("Claude Code");
    await screen.findByTestId("agent-card-claude");
    // 下拉里安装中的 agent 标签带「· 安装中…」后缀，用前缀匹配
    await selectAgent(/Kimi Code/);
    // 回来仍是安装中（不是「安装」按钮）——二次点击 = 重复安装
    expect(await screen.findByTestId("agent-installing-kimi")).toBeTruthy();
    expect(screen.queryByTestId("agent-install-kimi")).toBeNull();
    expect(api.installAgent).toHaveBeenCalledTimes(1);
  });

  it("卡片卸载期间 install-done 失败也不丢：切回后显示重试与失败信息", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("Kimi Code");
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(screen.getByTestId("agent-installing-kimi")).toBeTruthy());
    // 切走后（kimi 卡已卸载）后台安装失败
    await selectAgent("Claude Code");
    await screen.findByTestId("agent-card-claude");
    fireDone("kimi", false, "C:/logs/kimi-install.log");
    // 切回：失败态还在——重试按钮 + 可点的日志按钮（完整路径在 tooltip，S-8）
    await selectAgent("Kimi Code");
    expect(await screen.findByTestId("agent-install-kimi")).toBeTruthy();
    expect(screen.getByTestId("agent-open-log-kimi").getAttribute("data-tip")).toBe("C:/logs/kimi-install.log");
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

  it("中转模式不展示残留的官方账号配额或登录按钮", async () => {
    api.getAccounts.mockResolvedValue([
      {
        provider: "claude",
        account: { email: "old-official@example.com" },
        usage: { lanes: [{ kind: "five_hour", used_pct: 50 }], note: null },
        usage_supported: false,
        relay_enabled: true,
      },
    ]);
    render(<AccountSection />);
    expect((await screen.findByTestId("agent-desc-claude")).textContent).toBe(zh.account.relayActive);
    expect(screen.queryByTestId("agent-login-claude")).toBeNull();
    expect(screen.queryByTestId("agent-logout-claude")).toBeNull();
    expect(screen.queryByText(zh.account.quota)).toBeNull();
    // 账号列表管的是官方登录身份，中转与之互斥——一并隐藏，免得误导。
    expect(screen.queryByTestId("profiles-claude")).toBeNull();
  });

  /** 照 Orca 只有一处入口：有账号列表的 agent，卡片头不再放登录 / 退出，登录在账号卡上。 */
  it("有账号列表的 agent，卡片头不放登录/退出按钮", async () => {
    render(<AccountSection />);
    await screen.findByTestId("profiles-claude");
    expect(screen.queryByTestId("agent-logout-claude")).toBeNull();
    expect(screen.queryByTestId("agent-login-claude")).toBeNull();
  });

  /**
   * 活跃账号持续可见：切到过自定义账号，provider 卡片标题行就一直挂着「账号：<名>」徽章——
   * 这是「切过一次就忘了，之后会话全写进隔离账号」的防线。默认账号（active_profile_name 缺席）
   * 什么都不显示：没建过账号的用户零感知。
   */
  it("非默认账号活跃时卡片显示账号徽章，默认账号不显示", async () => {
    render(<AccountSection />);
    await screen.findByTestId("agent-card-claude");
    // 默认 mock 没有 active_profile_name（默认账号）→ 无徽章。
    expect(screen.queryByTestId("agent-profile-claude")).toBeNull();

    api.getAccounts.mockResolvedValue([
      { provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true, active_profile_name: "工作" },
    ]);
    cleanup();
    render(<AccountSection />);
    const badge = await screen.findByTestId("agent-profile-claude");
    expect(badge.textContent).toBe(zh.account.activeProfileBadge("工作"));

    // 账号名与邮箱相同（大小写不计）：徽章与下一行重复，隐藏。
    api.getAccounts.mockResolvedValue([
      { provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true, active_profile_name: "A@B.c" },
    ]);
    cleanup();
    render(<AccountSection />);
    await screen.findByTestId("agent-desc-claude");
    expect(screen.queryByTestId("agent-profile-claude")).toBeNull();

    // Gmail 不认点和 +后缀：账号名 ar.b+x@gmail.com 与登录邮箱 arb@gmail.com 是同一个收件箱（实拍），也隐藏。
    api.getAccounts.mockResolvedValue([
      { provider: "claude", account: { email: "arb@gmail.com" }, usage: null, usage_supported: true, active_profile_name: "Ar.B+x@gmail.com" },
    ]);
    cleanup();
    render(<AccountSection />);
    await screen.findByTestId("agent-desc-claude");
    expect(screen.queryByTestId("agent-profile-claude")).toBeNull();
  });

  it("只有 Gmail 按去点比较；别的域名点不同就是不同邮箱", () => {
    expect(sameAccountName("a.b@gmail.com", "ab@googlemail.com")).toBe(true);
    expect(sameAccountName("a.b@example.com", "ab@example.com")).toBe(false);
    expect(sameAccountName("工作", "ab@gmail.com")).toBe(false);
  });

  // leader 2026-11-19：「切换了 tab 就切换了（启动方式）这样不好」。标签只管看，按钮才管用。
  it("点标签或方向键只切换查看，不改启动方式；点「改用 API 中转」才启用", async () => {
    api.getSettings.mockResolvedValue({
      sticker_quota_providers: [],
      relay: {
        per_agent: {
          claude: {
            enabled: false,
            base_url: "https://relay.example/v1",
            model: "claude-relay",
            protocol: "",
            auth: "bearer",
          },
        },
      },
    });
    api.getRelaySecretStatus.mockResolvedValue({ claude: true, codex: false, kimi: false });
    render(<AccountSection />);

    const card = await screen.findByTestId("agent-card-claude");
    const relayChoice = Array.from(card.querySelectorAll('[role="radio"]')).find(
      (el) => el.textContent === zh.relay.title,
    ) as HTMLElement;
    expect(relayChoice).toBeTruthy();
    await waitFor(() => expect(api.getRelaySecretStatus).toHaveBeenCalled());
    // 生效的一边（官方）挂着「正在使用」的点。
    const live = () => screen.getByTestId("agent-access-live-claude").closest('[role="radio"]')?.textContent;
    expect(live()).toBe(zh.relay.official);
    fireEvent.click(relayChoice);
    expect(card.querySelector('[role="radio"][aria-checked="true"]')?.textContent).toBe(zh.relay.title);
    expect(await screen.findByDisplayValue("https://relay.example/v1")).toBeTruthy();
    fireEvent.keyDown(relayChoice, { key: "ArrowLeft" });
    expect(card.querySelector('[role="radio"][aria-checked="true"]')?.textContent).toBe(zh.relay.official);
    fireEvent.keyDown(card.querySelector('[role="radio"][aria-checked="true"]')!, { key: "ArrowRight" });
    expect(api.setSettings).not.toHaveBeenCalled();
    expect(live()).toBe(zh.relay.official);

    const use = await screen.findByTestId("agent-access-use-relay-claude");
    await waitFor(() => expect((use as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(use);
    await waitFor(() => expect(api.setSettings).toHaveBeenCalled());
    const saved = api.setSettings.mock.calls.at(-1)?.[0];
    expect(saved.relay.per_agent.claude.enabled).toBe(true);
  });

  it("正在用中转时点「官方账号」标签不停用中转，点「改用官方账号」才停用", async () => {
    api.getSettings.mockResolvedValue({
      sticker_quota_providers: [],
      relay: { per_agent: { claude: {
        enabled: true,
        base_url: "https://relay.example/v1",
        model: "claude-relay",
        protocol: "",
        auth: "bearer",
      } } },
    });
    api.getRelaySecretStatus.mockResolvedValue({ claude: true, codex: false, kimi: false });
    render(<AccountSection />);
    const card = await screen.findByTestId("agent-card-claude");
    // settings 异步加载后默认停在生效的一边（中转），能直接看到配置。
    expect(await screen.findByDisplayValue("claude-relay")).toBeTruthy();
    expect(card.querySelector('[role="radio"][aria-checked="true"]')?.textContent).toBe(zh.relay.title);
    const officialTab = Array.from(card.querySelectorAll('[role="radio"]')).find(
      (el) => el.textContent === zh.relay.official,
    ) as HTMLElement;
    fireEvent.click(officialTab);
    expect(screen.getByText(zh.relay.nowRelay)).toBeTruthy();
    expect(api.setSettings).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("agent-access-use-official-claude"));
    await waitFor(() =>
      expect(api.setSettings.mock.calls.at(-1)?.[0].relay.per_agent.claude.enabled).toBe(false),
    );
    // 中转配置原样保留。
    expect(api.setSettings.mock.calls.at(-1)?.[0].relay.per_agent.claude.base_url).toBe("https://relay.example/v1");
  });

  it("中转配置不全时「改用 API 中转」不可点并列出缺项；填齐也不会自动启用", async () => {
    api.getSettings.mockResolvedValue({
      sticker_quota_providers: [],
      relay: { per_agent: { claude: {
        enabled: false,
        base_url: "https://relay.example/v1",
        model: "",
        protocol: "",
        auth: "bearer",
      } } },
    });
    api.getRelaySecretStatus.mockResolvedValue({ claude: true, codex: false, kimi: false });
    render(<AccountSection />);
    const card = await screen.findByTestId("agent-card-claude");
    await waitFor(() => expect(api.getRelaySecretStatus).toHaveBeenCalled());
    fireEvent.click(Array.from(card.querySelectorAll('[role="radio"]')).find(
      (el) => el.textContent === zh.relay.title,
    ) as HTMLElement);
    const use = await screen.findByTestId("agent-access-use-relay-claude") as HTMLButtonElement;
    expect(use.disabled).toBe(true);
    expect(screen.getByText(zh.relay.missingFields([zh.relay.model]))).toBeTruthy();

    const model = screen.getByPlaceholderText(zh.relay.modelPlaceholder);
    fireEvent.change(model, { target: { value: "claude-relay" } });
    fireEvent.keyDown(model, { key: "Enter" });
    await waitFor(() => expect(api.setSettings).toHaveBeenCalled());
    const saved = api.setSettings.mock.calls.at(-1)?.[0].relay.per_agent.claude;
    expect(saved.model).toBe("claude-relay");
    expect(saved.enabled).toBe(false);
  });

  it("中转密钥仍走独立命令，不进入 Settings", async () => {
    render(<AccountSection />);
    const card = await screen.findByTestId("agent-card-claude");
    const relayChoice = Array.from(card.querySelectorAll('[role="radio"]')).find(
      (el) => el.textContent === zh.relay.title,
    ) as HTMLElement;
    fireEvent.click(relayChoice);
    const secret = await screen.findByPlaceholderText(zh.relay.secretPlaceholder);
    fireEvent.change(secret, { target: { value: "sk-never-in-settings" } });
    fireEvent.blur(secret);

    await waitFor(() =>
      expect(api.setRelaySecret).toHaveBeenCalledWith("claude", "sk-never-in-settings"),
    );
    expect(JSON.stringify(api.setSettings.mock.calls)).not.toContain("sk-never-in-settings");
  });

  it("已保存的中转密钥明文回填（便于核对），清空后删除", async () => {
    api.getSettings.mockResolvedValue({
      sticker_quota_providers: [],
      relay: { per_agent: { claude: {
        enabled: true,
        base_url: "https://relay.example/v1",
        model: "claude-relay",
        protocol: "",
        auth: "bearer",
      } } },
    });
    api.getRelaySecretStatus.mockResolvedValue({ claude: true, codex: false, kimi: false });
    api.getRelaySecrets.mockResolvedValue({ claude: "sk-visible-local" });
    render(<AccountSection />);
    const secret = await screen.findByDisplayValue("sk-visible-local") as HTMLInputElement;
    // 令牌明文展示：粘贴错一个字符就是 401，用户需要肉眼核对，且仅本机可见。
    expect(secret.type).toBe("text");
    fireEvent.change(secret, { target: { value: "" } });
    fireEvent.blur(secret);
    await waitFor(() => expect(api.setRelaySecret).toHaveBeenCalledWith("claude", ""));
    await waitFor(() =>
      expect(api.setSettings.mock.calls.at(-1)?.[0].relay.per_agent.claude.enabled).toBe(false),
    );
    expect(await screen.findByText(zh.relay.secretClearedOff)).toBeTruthy();
  });

  it("旧中转规则协议为空时保存插件默认协议", async () => {
    api.listAgents.mockResolvedValue(descriptors(["claude", "codex", "kimi"]));
    api.getSettings.mockResolvedValue({
      sticker_quota_providers: [],
      relay: { per_agent: { kimi: {
        enabled: false,
        base_url: "https://relay.example/v1",
        model: "kimi-for-coding",
        protocol: "",
        auth: "bearer",
      } } },
    });
    api.getRelaySecretStatus.mockResolvedValue({ claude: false, codex: false, kimi: true });
    render(<AccountSection />);
    await selectAgent("Kimi Code");
    const card = await screen.findByTestId("agent-card-kimi");
    const relayChoice = Array.from(card.querySelectorAll('[role="radio"]')).find(
      (el) => el.textContent === zh.relay.title,
    ) as HTMLElement;
    fireEvent.click(relayChoice);
    const use = await screen.findByTestId("agent-access-use-relay-kimi") as HTMLButtonElement;
    await waitFor(() => expect(use.disabled).toBe(false));
    fireEvent.click(use);

    await waitFor(() => expect(api.setSettings).toHaveBeenCalled());
    const saved = api.setSettings.mock.calls.at(-1)?.[0].relay.per_agent.kimi;
    expect(saved.enabled).toBe(true);
    expect(saved.protocol).toBe("kimi");
  });

  it("模型选择器从中转获取选项，同时仍可输入自定义模型", async () => {
    api.getSettings.mockResolvedValue({
      sticker_quota_providers: [],
      relay: { per_agent: { claude: {
        enabled: true,
        base_url: "https://relay.example/v1",
        model: "old-model",
        protocol: "",
        auth: "bearer",
      } } },
    });
    api.getRelaySecretStatus.mockResolvedValue({ claude: true, codex: false, kimi: false });
    api.listRelayModels.mockResolvedValue(["relay-model-a", "relay-model-b"]);
    render(<AccountSection />);

    const input = await screen.findByDisplayValue("old-model");
    await waitFor(() => expect(api.getRelaySecretStatus).toHaveBeenCalled());
    fireEvent.focus(input);
    await waitFor(() => expect(api.listRelayModels).toHaveBeenCalledWith(
      "claude", "https://relay.example/v1", "", "bearer",
    ));
    fireEvent.change(input, { target: { value: "relay-model" } });
    fireEvent.click(await screen.findByText("relay-model-b"));
    await waitFor(() => expect(api.setSettings).toHaveBeenCalled());
    expect(api.setSettings.mock.calls.at(-1)?.[0].relay.per_agent.claude.model).toBe("relay-model-b");

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "vendor-private-model" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(api.setSettings.mock.calls.at(-1)?.[0].relay.per_agent.claude.model).toBe("vendor-private-model"));
  });

  // 回归：此前安装输出被丢进 Stdio::null()，失败时用户拿不到任何可排查的东西。
  it("install-done 失败：给出安装日志路径（有 logPath 时）", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("Kimi Code");
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    fireDone("kimi", false, "C:\\Users\\x\\.meowo\\install-kimi.log");
    const log = await screen.findByTestId("agent-install-log-kimi");
    // 路径不裸文本展示（没法点）：按钮打开日志，完整路径在按钮 tooltip（S-8）。
    expect(log.querySelector("[data-testid='agent-open-log-kimi']")?.getAttribute("data-tip")).toContain("install-kimi.log");
  });

  // S-8：日志按钮真的会把日志打开（路径由后端按 provider 重算，前端不传路径）。
  it("点「打开安装日志」调用 openInstallLog", async () => {
    api.installAgent.mockResolvedValue(undefined);
    api.openInstallLog.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("Kimi Code");
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    fireDone("kimi", false, "C:\\Users\\x\\.meowo\\install-kimi.log");
    fireEvent.click(await screen.findByTestId("agent-open-log-kimi"));
    await waitFor(() => expect(api.openInstallLog).toHaveBeenCalledWith("kimi"));
  });

  // 回归：claude 的安装器不写 PATH 也照样 exit 0——「装好了」不等于「终端里敲得出来」。
  // 提示条须在挂载时就查（多数受害者是早就装好、从没进过 PATH 的用户），不能只在装完时查。
  it("已装但 bin 目录不在 PATH：显示提示条，可一键写入", async () => {
    api.agentPathGap.mockResolvedValue("C:\\Users\\x\\.local\\bin");
    render(<AccountSection />);
    const gap = await screen.findByTestId("agent-path-gap-claude");
    // 提示条刻意低调：正文只留一句「为什么该点」，**完整路径挪进 tooltip**——
    // 这条对多数人是背景噪音（装完就在 PATH 上），横一条长提示会喧宾夺主。
    expect(gap.getAttribute("data-tip")).toContain(".local\\bin");
    expect(gap.textContent).not.toContain(".local\\bin");

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
    await selectAgent("Kimi Code");
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
    await selectAgent("Kimi Code");
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
    await selectAgent("Kimi Code");
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
    // claude（默认选中）已装且有账号 → 已登录，无登录按钮。
    await screen.findByTestId("agent-card-claude");
    expect(screen.queryByTestId("agent-login-claude")).toBeNull();
    // codex 已装但 getAccounts 没返回它 → account 为 null → 未登录，给登录按钮。
    await selectAgent("Codex");
    expect(await screen.findByTestId("profile-login-codex-__default__")).toBeTruthy();
    // kimi 未装 → 该先安装，不给登录按钮。
    await selectAgent("Kimi Code");
    await screen.findByTestId("agent-install-kimi");
    expect(screen.queryByTestId("profile-login-kimi-__default__")).toBeNull();
  });

  /**
   * 没有账号概念的 agent：已装也不给登录按钮、不报「未登录」。
   *
   * 回归：`getAccounts()` 只返回**声明了账号能力**的 agent。旧逻辑靠 `payload == null` 判定未登录，
   * 于是给没有账号能力的 agent 也亮出登录按钮——而它的 `login_argv()` 是 None，点下去后端只能回
   * 一句「拉起登录失败」。给出走不通的入口，比不给入口更糟：用户会以为是自己的环境有问题，
   * 反复去点。（opencode 真的这么翻过车，直到它的登录入口被补上。）
   *
   * 判据必须是后端下发的 `supports_account`，不能靠「账号查不出来」去猜——那与「真的没登录」
   * （codex 那种）长得一模一样。antigravity 就没有账号能力（凭据在系统钥匙串、没有登录子命令）。
   */
  it("无账号概念的 agent 已装也不给登录按钮，且不报未登录", async () => {
    api.listAgents.mockResolvedValue([
      { id: "claude", display_name: "Claude Code", installed: true, supports_proxy: true, supports_account: true, supports_profiles: true },
      { id: "codex", display_name: "Codex", installed: true, supports_proxy: true, supports_account: true, supports_profiles: true },
      { id: "antigravity", display_name: "Antigravity CLI", installed: true, supports_proxy: true, supports_account: false, supports_profiles: false },
    ]);
    render(<AccountSection />);

    // 对照：codex 有账号能力但没登录 → 照旧给按钮。
    await selectAgent("Codex");
    expect(await screen.findByTestId("profile-login-codex-__default__")).toBeTruthy();

    // 已装，但没有账号概念 → 卡片在，登录按钮不在。
    await selectAgent("Antigravity CLI");
    expect(await screen.findByTestId("agent-card-antigravity")).toBeTruthy();
    expect(screen.queryByTestId("agent-login-antigravity")).toBeNull();
    // 也不该显示「未登录」——它没有「登录」这回事。
    expect(screen.queryByTestId("agent-desc-antigravity")).toBeNull();
  });

  it("点登录调 loginAgent 并进入等待态", async () => {
    api.loginAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("Codex");
    const btn = await screen.findByTestId("profile-login-codex-__default__");
    fireEvent.click(btn);
    // 系统默认那张卡登录：显式传 null（= 默认账号），不是省略（= 当前活跃账号）。
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalledWith("codex", undefined, null, expect.any(String)));
    // spawn 成功后不落回 idle——等 login-done。按钮此时变成「取消等待」，而不是一个死掉的禁用按钮：
    // 终端可能已被关掉，而后端要 5 分钟才超时。
    await waitFor(() => expect(screen.getByTestId("profile-login-codex-__default__").textContent).toBe(zh.account.cancelLogin));
    expect((screen.getByTestId("profile-login-codex-__default__") as HTMLButtonElement).disabled).toBe(false);
  });

  /// 等待态零指引曾是实测痛点：不说窗口开在哪个终端、最多等多久，用户对着「等待登录…」干等。
  it("等待中显示「已在哪个终端打开、最长 5 分钟」", async () => {
    api.getSettings.mockResolvedValue({ sticker_quota_providers: [], resume_terminal: "wt" });
    api.availableTerminals.mockResolvedValue(["wt", "powershell"]);
    api.loginAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("Codex");
    fireEvent.click(await screen.findByTestId("profile-login-codex-__default__"));
    await waitFor(() =>
      expect(screen.getByTestId("agent-login-error-codex").textContent).toBe(zh.account.loginWaiting("Windows Terminal")),
    );
  });

  it("切换 agent 后仍保留等待中的 operationId，并可用同一 id 取消", async () => {
    api.loginAgent.mockResolvedValue(undefined);
    api.cancelLogin.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("Codex");
    fireEvent.click(await screen.findByTestId("profile-login-codex-__default__"));
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalled());
    const operationId = api.loginAgent.mock.calls[0][3];

    await selectAgent("Claude Code");
    await selectAgent("Codex");
    await waitFor(() => expect(screen.getByTestId("profile-login-codex-__default__").textContent).toBe(zh.account.cancelLogin));

    fireEvent.click(screen.getByTestId("profile-login-codex-__default__"));
    await waitFor(() => expect(api.cancelLogin).toHaveBeenCalledWith("codex", operationId));
  });

  it("login-done 成功 → 重查账号（卡片可转已登录）", async () => {
    api.loginAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("Codex");
    fireEvent.click(await screen.findByTestId("profile-login-codex-__default__"));
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalled());
    const before = api.getAccounts.mock.calls.length;
    // 登录成功后 codex 也有账号了
    api.getAccounts.mockResolvedValue([
      { provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true },
      { provider: "codex", account: { login_label: "API Key" }, usage: null, usage_supported: false },
    ]);
    fireLogin("codex", "success");
    await waitFor(() => expect(api.getAccounts.mock.calls.length).toBeGreaterThan(before));
    await waitFor(() => expect(screen.queryByTestId("profile-login-codex-__default__")).toBeNull());
  });

  it("login-done 超时 → 落回可点 + 显示本地化提示（超时不是登录失败）", async () => {
    api.loginAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("Codex");
    fireEvent.click(await screen.findByTestId("profile-login-codex-__default__"));
    await waitFor(() => expect(screen.getByTestId("profile-login-codex-__default__").textContent).toBe(zh.account.cancelLogin));
    fireLogin("codex", "timeout");
    await waitFor(() => expect(screen.getByTestId("profile-login-codex-__default__").textContent).toBe(zh.account.login));
    expect(screen.getByTestId("agent-login-error-codex").textContent).toBe(zh.account.loginTimeout);
  });

  /// 起因：点完登录后如果终端被关掉（用户手动关、崩溃、agent 自己退出），后端只轮询账号文件，
  /// 要 5 分钟才超时。这五分钟里按钮一直是「等待登录…」且不可点——用户既不能重来也不知道发生了什么。
  it("等待中点按钮 → 调 cancel_login，落回可点并提示已取消（而非「未检测到登录完成」）", async () => {
    api.loginAgent.mockResolvedValue(undefined);
    api.cancelLogin.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("Codex");
    fireEvent.click(await screen.findByTestId("profile-login-codex-__default__"));
    await waitFor(() => expect(screen.getByTestId("profile-login-codex-__default__").textContent).toBe(zh.account.cancelLogin));

    fireEvent.click(screen.getByTestId("profile-login-codex-__default__"));
    await waitFor(() => expect(api.cancelLogin).toHaveBeenCalledWith("codex", expect.any(String)));

    // 收尾由后端 emit login-done（它会再查一次账号；这里模拟「确实没登上」）。
    fireLogin("codex", "cancelled");
    await waitFor(() => expect(screen.getByTestId("profile-login-codex-__default__").textContent).toBe(zh.account.login));
    // 取消 ≠ 超时：文案必须区分，否则用户以为是没检测到。
    expect(screen.getByTestId("agent-login-error-codex").textContent).toBe(zh.account.loginCancelled);
  });

  /// 取消时后端会再查一次账号——用户可能已经在终端里登完了，只是嫌等得慢。此时应转「已登录」。
  it("取消时若其实已登录成功 → 转已登录，不显示取消提示", async () => {
    api.loginAgent.mockResolvedValue(undefined);
    api.cancelLogin.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("Codex");
    fireEvent.click(await screen.findByTestId("profile-login-codex-__default__"));
    await waitFor(() => expect(screen.getByTestId("profile-login-codex-__default__").textContent).toBe(zh.account.cancelLogin));
    fireEvent.click(screen.getByTestId("profile-login-codex-__default__"));
    await waitFor(() => expect(api.cancelLogin).toHaveBeenCalled());

    api.getAccounts.mockResolvedValue([
      { provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true },
      { provider: "codex", account: { login_label: "API Key" }, usage: null, usage_supported: false },
    ]);
    fireLogin("codex", "success"); // 后端复查发现真登上了
    await waitFor(() => expect(screen.queryByTestId("profile-login-codex-__default__")).toBeNull());
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
    await selectAgent("Codex");

    // 未接入 → 提示条 + 修复按钮在。
    expect(await screen.findByTestId("agent-repair-hooks-codex")).toBeTruthy();

    fireEvent.click(screen.getByTestId("profile-login-codex-__default__"));
    fireLogin("codex", "success");
    // 登录成功 → 重查 hooks 得 installed → 提示条消失。
    await waitFor(() => expect(screen.queryByTestId("agent-repair-hooks-codex")).toBeNull());
  });

  it("拉起登录失败 → 落回可点 + 显示提示", async () => {
    api.loginAgent.mockRejectedValue(new Error("启动终端失败"));
    render(<AccountSection />);
    await selectAgent("Codex");
    fireEvent.click(await screen.findByTestId("profile-login-codex-__default__"));
    await waitFor(() => expect(screen.getByTestId("agent-login-error-codex")).toBeTruthy());
    expect((screen.getByTestId("profile-login-codex-__default__") as HTMLButtonElement).disabled).toBe(false);
  });

  it("装完自动引导：install-done 成功后，该 agent 的登录按钮被标为主要动作", async () => {
    api.installAgent.mockResolvedValue(undefined);
    // 初次未装 kimi；装完重查返回含 kimi（此时 kimi 无账号 → 未登录）
    api.listAgents.mockResolvedValueOnce(descriptors(["claude", "codex"])).mockResolvedValue(descriptors(["claude", "codex", "kimi"]));
    render(<AccountSection />);
    await selectAgent("Kimi Code");
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    fireDone("kimi", true);
    const loginBtn = await screen.findByTestId("profile-login-kimi-__default__");
    // 「装完 → 登录」串成一条链路：按钮升为 primary，而非埋在一堆次要按钮里。
    expect(loginBtn.className).toContain("btn-primary");
    // 未经安装流程的 codex 则是普通次要按钮（切过去看）。
    await selectAgent("Codex");
    expect((await screen.findByTestId("profile-login-codex-__default__")).className).not.toContain("btn-primary");
  });
});

describe("AccountSection 多账号", () => {
  /**
   * 登录**必须带上那个账号自己的 id**。
   *
   * 这是整个多账号功能的命门：登录会把凭据写进「注入的隔离变量所指的目录」，而那个变量由 profile id
   * 决定。漏传 id，新账号的登录就会把**默认账号**的凭据覆盖掉——用户以为自己加了个账号，
   * 其实是把原来那个换掉了，而且毫无提示。
   */
  it("给某个账号点登录时，把它的 id 传给 loginAgent", async () => {
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "work", name: "工作", active: false, account: null }, // 未登录
    ]);
    api.loginAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    // claude 是默认选中卡，直接就在，不必切换。

    fireEvent.click(await screen.findByTestId("profile-login-claude-work"));
    await waitFor(() =>
      expect(api.loginAgent).toHaveBeenCalledWith("claude", undefined, "work", expect.any(String))
    );
  });

  it("查看其他账号用量不切换 active；可切回 active 视图", async () => {
    api.getAccounts.mockResolvedValue([{ provider: "claude", account: { email: "a@b.c" }, usage: usageAt(11), usage_supported: true }]);
    api.refreshUsage.mockResolvedValue(usageAt(11));
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "work", name: "工作", active: false, account: { email: "w@b.c" } },
    ]);
    api.refreshProfileUsage.mockResolvedValue(usageAt(73));
    render(<AccountSection />);

    const activeView = await screen.findByTestId("profile-usage-claude-__default__");
    expect(activeView.getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(await screen.findByTestId("profile-usage-claude-work"));
    await waitFor(() => expect(screen.getByRole("progressbar", { name: zh.account.laneFiveHour }).getAttribute("aria-valuenow")).toBe("73"));
    expect(api.refreshProfileUsage).toHaveBeenCalledWith("claude", "work");
    expect(screen.getByText(zh.account.usageFor("工作"))).toBeTruthy();
    expect(screen.queryByRole("switch", { name: zh.settings.showQuotaOnSticker })).toBeNull();
    expect(api.setActiveProfile).not.toHaveBeenCalled();
    expect(api.applyActiveProfileToSessions).not.toHaveBeenCalled();

    fireEvent.click(activeView);
    await waitFor(() => expect(screen.getByRole("progressbar", { name: zh.account.laneFiveHour }).getAttribute("aria-valuenow")).toBe("11"));
    expect(screen.getByText(zh.account.quota)).toBeTruthy();
    expect(screen.getByRole("switch", { name: zh.settings.showQuotaOnSticker })).toBeTruthy();
    expect(api.refreshProfileUsage).toHaveBeenCalledTimes(1);
  });

  it("active 未登录时也能查看其他已登录账号", async () => {
    api.getAccounts.mockResolvedValue([{ provider: "claude", account: null, usage: null, usage_supported: false }]);
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: null },
      { id: "work", name: "工作", active: false, account: { email: "w@b.c" } },
    ]);
    api.refreshProfileUsage.mockResolvedValue(usageAt(33));
    render(<AccountSection />);

    expect(await screen.findByTestId("profile-usage-claude-work")).toBeTruthy();
    expect(screen.queryByRole("progressbar")).toBeNull();
    fireEvent.click(screen.getByTestId("profile-usage-claude-work"));
    await waitFor(() => expect(screen.getByRole("progressbar", { name: zh.account.laneFiveHour }).getAttribute("aria-valuenow")).toBe("33"));
  });

  it("刷新其他账号失败时保留已显示的用量", async () => {
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "work", name: "工作", active: false, account: { email: "w@b.c" } },
    ]);
    api.refreshProfileUsage.mockResolvedValueOnce(usageAt(45));
    render(<AccountSection />);

    fireEvent.click(await screen.findByTestId("profile-usage-claude-work"));
    await waitFor(() => expect(screen.getByRole("progressbar", { name: zh.account.laneFiveHour }).getAttribute("aria-valuenow")).toBe("45"));
    api.refreshProfileUsage.mockRejectedValueOnce("offline");
    fireEvent.click(screen.getByRole("button", { name: zh.account.refresh }));
    expect(await screen.findByText(zh.account.refreshFailed)).toBeTruthy();
    expect(screen.getByRole("progressbar", { name: zh.account.laneFiveHour }).getAttribute("aria-valuenow")).toBe("45");
  });

  it("其他账号不支持查询时显示能力状态并停用刷新", async () => {
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "work", name: "工作", active: false, account: { email: "w@b.c" } },
    ]);
    api.refreshProfileUsage.mockRejectedValue("USAGE_UNSUPPORTED");
    render(<AccountSection />);

    fireEvent.click(await screen.findByTestId("profile-usage-claude-work"));
    expect(await screen.findByText(zh.account.usageUnsupported)).toBeTruthy();
    expect((screen.getByRole("button", { name: zh.account.refresh }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("快速切换查看账号时，旧请求响应不能覆盖新视图", async () => {
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "work", name: "工作", active: false, account: { email: "w@b.c" } },
      { id: "personal", name: "个人", active: false, account: { email: "p@b.c" } },
    ]);
    let resolveWork!: (u: ReturnType<typeof usageAt>) => void;
    let resolvePersonal!: (u: ReturnType<typeof usageAt>) => void;
    api.refreshProfileUsage.mockImplementation((_provider, profile) => new Promise((resolve) => {
      if (profile === "work") resolveWork = resolve;
      else resolvePersonal = resolve;
    }));
    render(<AccountSection />);

    fireEvent.click(await screen.findByTestId("profile-usage-claude-work"));
    fireEvent.click(screen.getByTestId("profile-usage-claude-personal"));
    await act(async () => { resolvePersonal(usageAt(21)); });
    expect(screen.getByRole("progressbar", { name: zh.account.laneFiveHour }).getAttribute("aria-valuenow")).toBe("21");
    await act(async () => { resolveWork(usageAt(88)); });
    expect(screen.getByRole("progressbar", { name: zh.account.laneFiveHour }).getAttribute("aria-valuenow")).toBe("21");
    expect(screen.getByText(zh.account.usageFor("个人"))).toBeTruthy();
  });

  it("别处（手机远程）切了账号：列表、正在看的账号、额度都跟上，不留旧账号的数字", async () => {
    let active: "default" | "work" = "default";
    api.getAccounts.mockImplementation(async () => [
      // 后端切换时已清掉这家的用量缓存：新快照不带额度。
      { provider: "claude", account: { email: "a@b.c" }, usage: active === "default" ? usageAt(11) : null, usage_supported: true },
    ]);
    api.refreshUsage.mockImplementation(async () => usageAt(active === "default" ? 11 : 64));
    api.listProfiles.mockImplementation(async () => [
      { id: null, name: "", active: active === "default", account: { email: "a@b.c" } },
      { id: "work", name: "工作", active: active === "work", account: { email: "w@b.c" } },
    ]);
    api.refreshProfileUsage.mockResolvedValue(usageAt(40));
    render(<AccountSection />);

    await waitFor(() => expect(screen.getByRole("progressbar", { name: zh.account.laneFiveHour }).getAttribute("aria-valuenow")).toBe("11"));
    // 桌面上正在看「工作」的用量
    fireEvent.click(await screen.findByTestId("profile-usage-claude-work"));
    await waitFor(() => expect(screen.getByText(zh.account.usageFor("工作"))).toBeTruthy());

    active = "work";
    act(() => ev.profileCbs.forEach((cb) => cb({ payload: "claude" })));

    // 「使用中」移到工作账号
    await waitFor(() =>
      expect(screen.getByTestId("profile-claude-work").textContent).toContain(zh.account.activeProfile),
    );
    expect(screen.getByTestId("profile-claude-__default__").textContent).not.toContain(zh.account.activeProfile);
    // 退回看当前账号，额度是新账号的
    expect(screen.getByText(zh.account.quota)).toBeTruthy();
    await waitFor(() => expect(screen.getByRole("progressbar", { name: zh.account.laneFiveHour }).getAttribute("aria-valuenow")).toBe("64"));
    expect(api.setActiveProfile).not.toHaveBeenCalled();
  });

  it("别家切了账号不影响这一家", async () => {
    api.getAccounts.mockResolvedValue([{ provider: "claude", account: { email: "a@b.c" }, usage: usageAt(11), usage_supported: true }]);
    api.refreshUsage.mockResolvedValue(usageAt(11));
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "work", name: "工作", active: false, account: { email: "w@b.c" } },
    ]);
    render(<AccountSection />);
    await screen.findByTestId("profile-claude-work");
    const lists = api.listProfiles.mock.calls.length;
    act(() => ev.profileCbs.forEach((cb) => cb({ payload: "codex" })));
    await new Promise((r) => setTimeout(r, 20));
    expect(api.listProfiles.mock.calls.length).toBe(lists);
    expect(screen.getByRole("progressbar", { name: zh.account.laneFiveHour }).getAttribute("aria-valuenow")).toBe("11");
  });

  it("点账号行即切换；已是活跃的那行不可点", async () => {
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "work", name: "工作", active: false, account: { email: "w@b.c" } },
    ]);
    api.setActiveProfile.mockResolvedValue(undefined);
    render(<AccountSection />);

    const row = await screen.findByTestId("profile-claude-work");
    fireEvent.click(row.querySelector(".acct-main")!);
    await waitFor(() => expect(api.setActiveProfile).toHaveBeenCalledWith("claude", "work"));

    // 活跃那行（默认账号）的主按钮不可点——点它没有意义。用 aria-disabled（做空第一轮 #4）：
    // 刚切过去的那行会变成活跃，真 disabled 会把焦点踢到 body。
    const def = screen.getByTestId("profile-claude-__default__");
    const main = def.querySelector(".acct-main") as HTMLButtonElement;
    expect(main.getAttribute("aria-disabled")).toBe("true");
    api.setActiveProfile.mockClear();
    fireEvent.click(main);
    expect(api.setActiveProfile).not.toHaveBeenCalled();
  });

  /**
   * 切换账号对**运行中**的会话：进程中途换不了账号，得停掉再按新账号恢复。
   *
   * 顺序是行为的一部分：**先切设置、再问要不要重启**。反过来（问完才切）会让「不重启」
   * 连账号都切不成——而那正是此前一直可用的行为。
   */
  it("切账号：先落设置，再问要不要把运行中的会话重启到新账号", async () => {
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "work", name: "工作", active: false, account: { email: "w@b.c" } },
    ]);
    api.sessionsOffActiveProfileCount.mockResolvedValue(1);
    api.setActiveProfile.mockResolvedValue(undefined);
    render(<AccountSection />);

    const row = await screen.findByTestId("profile-claude-work");
    fireEvent.click(row.querySelector(".acct-main")!);
    await waitFor(() => expect(api.setActiveProfile).toHaveBeenCalledWith("claude", "work"));
    // 待重启数问的是后端（看板列表分页，自己数会漏），且必须切完再问。
    await waitFor(() => expect(api.sessionsOffActiveProfileCount).toHaveBeenCalledWith("claude"));
    expect(api.setActiveProfile.mock.invocationCallOrder[0])
      .toBeLessThan(api.sessionsOffActiveProfileCount.mock.invocationCallOrder[0]);
    await waitFor(() => expect(dialog.confirm).toHaveBeenCalled());
    expect(dialog.confirm.mock.calls[0][0]).toContain("1");
    expect(dialog.confirm.mock.calls[0][1]).toMatchObject({ danger: true });
    // 确认（beforeEach 默认 resolve true）→ 后端把这些会话就地重启到新账号。
    await waitFor(() => expect(api.applyActiveProfileToSessions).toHaveBeenCalledWith("claude"));
  });

  /** 不重启 = 退回老行为：账号照切，运行中的会话留在原账号上。绝不能连账号都没切成。 */
  it("切账号：确认框点取消，账号仍已切换，只是不重启会话", async () => {
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "work", name: "工作", active: false, account: { email: "w@b.c" } },
    ]);
    api.sessionsOffActiveProfileCount.mockResolvedValue(1);
    api.setActiveProfile.mockResolvedValue(undefined);
    dialog.confirm.mockResolvedValue(false);
    render(<AccountSection />);

    const row = await screen.findByTestId("profile-claude-work");
    fireEvent.click(row.querySelector(".acct-main")!);
    await waitFor(() => expect(api.setActiveProfile).toHaveBeenCalledWith("claude", "work"));
    await waitFor(() => expect(dialog.confirm).toHaveBeenCalled());
    expect(api.applyActiveProfileToSessions).not.toHaveBeenCalled();
  });

  /** 设置「切换后运行中的会话」（照 Orca）：直接重启不再弹确认；留在原账号连数都不问。 */
  it("切账号：设成直接重启时不问，设成留在原账号时不重启", async () => {
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "work", name: "工作", active: false, account: { email: "w@b.c" } },
    ]);
    api.sessionsOffActiveProfileCount.mockResolvedValue(2);
    api.setActiveProfile.mockResolvedValue(undefined);
    api.getSettings.mockResolvedValue({ sticker_quota_providers: [], profile_switch_running: "restart" });
    const first = render(<AccountSection />);
    let row = await screen.findByTestId("profile-claude-work");
    fireEvent.click(row.querySelector(".acct-main")!);
    await waitFor(() => expect(api.applyActiveProfileToSessions).toHaveBeenCalledWith("claude"));
    expect(dialog.confirm).not.toHaveBeenCalled();
    first.unmount();

    api.applyActiveProfileToSessions.mockClear();
    api.sessionsOffActiveProfileCount.mockClear();
    api.setActiveProfile.mockClear();
    api.getSettings.mockResolvedValue({ sticker_quota_providers: [], profile_switch_running: "keep" });
    render(<AccountSection />);
    row = await screen.findByTestId("profile-claude-work");
    // 设置读进来之后再点（与用户的实际顺序一致）。
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled());
    await screen.findByTestId("profile-switch-running-claude");
    fireEvent.click(row.querySelector(".acct-main")!);
    await waitFor(() => expect(api.setActiveProfile).toHaveBeenCalledWith("claude", "work"));
    expect(api.sessionsOffActiveProfileCount).not.toHaveBeenCalled();
    expect(dialog.confirm).not.toHaveBeenCalled();
    expect(api.applyActiveProfileToSessions).not.toHaveBeenCalled();
  });

  /**
   * 会话搬不过去的 agent（opencode：会话存储没取证过，插件没声明 CrossAccountSession）连问都
   * 不问——它恢复时仍会回到会话原本的账号，重启只是白杀一次进程、把正在生成的回答丢掉。
   * 判据是后端下发的 `moves_sessions_across_accounts`，**不得按 id 判断**。
   */
  it("切账号：会话搬不过去的 agent 不问也不重启，覆盖面文案照实说", async () => {
    api.listAgents.mockResolvedValue(descriptors(["claude", "opencode"]));
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "work", name: "工作", active: false, account: { email: "w@b.c" } },
    ]);
    api.sessionsOffActiveProfileCount.mockResolvedValue(1); // 就算后端说有，也不该问到这一步
    api.setActiveProfile.mockResolvedValue(undefined);
    render(<AccountSection />);
    await selectAgent("OpenCode");

    expect((await screen.findByTestId("profiles-opencode")).textContent)
      .toContain(zh.account.switchCoverage);
    const row = await screen.findByTestId("profile-opencode-work");
    fireEvent.click(row.querySelector(".acct-main")!);
    await waitFor(() => expect(api.setActiveProfile).toHaveBeenCalledWith("opencode", "work"));
    expect(api.sessionsOffActiveProfileCount).not.toHaveBeenCalled();
    expect(dialog.confirm).not.toHaveBeenCalled();
    expect(api.applyActiveProfileToSessions).not.toHaveBeenCalled();
  });

  /**
   * 不支持多账号的 agent（数据目录不可被环境变量覆盖，谎称支持会让两个账号共用凭据）不显示账号列表。
   *
   * 判据必须是后端下发的 `supports_profiles`，不能靠「列表只有一条」推断——那与「只建了默认账号」
   * 长得一模一样。谎称支持的后果是两个「账号」静默共用同一份凭据。
   */
  it("不支持多账号的 agent 不显示账号列表", async () => {
    api.listAgents.mockResolvedValue([
      { id: "claude", display_name: "Claude Code", installed: true, supports_proxy: true, supports_account: true, supports_profiles: true },
      // 有账号、但不能多账号：现役几家没有这种组合，手造一个。
      { id: "single", display_name: "Single Account", installed: true, supports_proxy: false, supports_account: true, supports_profiles: false },
    ]);
    render(<AccountSection />);

    await waitFor(() => expect(screen.getByTestId("profiles-claude")).toBeTruthy());
    expect(screen.queryByTestId("profiles-single")).toBeNull();
  });

  /**
   * 上下文占用不支持的 agent（opencode）明写「不支持」，不留空白让用户以为是 bug。
   * 支持的（claude）不显示这行。
   */
  it("上下文不支持的 agent 显式标注，支持的不标", async () => {
    // opencode 需已装才谈得上「能力」——未装时卡片只有安装按钮。
    api.listAgents.mockResolvedValue(descriptors(["claude", "codex", "opencode"]));
    render(<AccountSection />);
    // claude（默认选中、支持上下文）→ 没有这行。
    await screen.findByTestId("agent-card-claude");
    expect(screen.queryByTestId("agent-context-unsupported-claude")).toBeNull();
    // 切到 opencode（已装、不支持上下文）→ 有这行。
    await selectAgent("OpenCode");
    expect(await screen.findByTestId("agent-context-unsupported-opencode")).toBeTruthy();
  });

  /**
   * 照 Orca：套餐写在详情行（组织 · 套餐 · 邮箱），标题是名字；没起名的账号标题就是登录身份。
   * kimi 给不出邮箱，身份退到 userId 短码。
   */
  it("托管账号的详情行写套餐和登录身份；没起名的标题用登录身份", async () => {
    api.listAgents.mockResolvedValue([
      { id: "kimi", display_name: "Kimi Code", installed: true, supports_proxy: true, supports_account: true, supports_profiles: true },
    ]);
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: false, account: { login_label: "cnta…5a4g" } },
      { id: "alt", name: "小号", active: true, account: { login_label: "d0ah…9x2k", plan: "Allegretto" } },
      { id: "account", name: "", active: false, account: { login_label: "e1bc…7z0q" } },
    ]);
    render(<AccountSection />);

    const sys = await screen.findByTestId("profile-kimi-__default__");
    expect(sys.querySelector(".acct-name")?.textContent).toBe(zh.account.systemDefaultNamed("cnta…5a4g"));
    const alt = screen.getByTestId("profile-kimi-alt");
    expect(alt.querySelector(".acct-name")?.textContent).toBe("小号");
    expect(alt.querySelector(".acct-desc")?.textContent).toBe("Allegretto · d0ah…9x2k");
    expect(alt.querySelector(".acct-badge")?.textContent).toBe(zh.account.activeProfile);
    const unnamed = screen.getByTestId("profile-kimi-account");
    expect(unnamed.querySelector(".acct-name")?.textContent).toBe("e1bc…7z0q");
  });

  /** 没登录的账号不能切过去（Orca：needsSignIn 时禁用选择）：切过去新会话直接卡在登录页。 */
  it("没登录的账号写明要重新登录，且点它不切换", async () => {
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "work", name: "工作", active: false, account: null },
    ]);
    render(<AccountSection />);
    const row = await screen.findByTestId("profile-claude-work");
    expect(row.querySelector(".acct-desc")?.textContent).toBe(zh.account.signInRequired);
    fireEvent.click(row.querySelector(".acct-main")!);
    await new Promise((r) => setTimeout(r, 20));
    expect(api.setActiveProfile).not.toHaveBeenCalled();
    expect(screen.getByTestId("profile-login-claude-work").textContent).toBe(zh.account.login);
  });

  it("一个账号都没加时显示空状态", async () => {
    render(<AccountSection />);
    expect((await screen.findByTestId("profiles-empty-claude")).textContent).toBe(zh.account.accountsEmpty("Claude Code"));
  });
});

describe("AccountSection 添加账号（照 Orca：直接登录）", () => {
  /** 点「添加账号」不先问名字：建一个没名字的账号，立刻在它里面登录（带它自己的 id）。 */
  it("建一个没名字的账号并立刻在它里面登录", async () => {
    api.createProfile.mockResolvedValue("account");
    api.loginAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("profile-add-claude"));
    await waitFor(() => expect(api.createProfile).toHaveBeenCalledWith("claude", ""));
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalledWith("claude", undefined, "account", expect.any(String)));
    expect(screen.getByTestId("profile-add-cancel-claude")).toBeTruthy();
  });

  /** 登录没完成（取消 / 超时）：刚建的空账号撤掉，不留未登录的空壳。 */
  it("登录取消后撤掉刚建的空账号", async () => {
    api.createProfile.mockResolvedValue("account");
    api.loginAgent.mockResolvedValue(undefined);
    api.cancelLogin.mockResolvedValue(undefined);
    api.deleteProfile.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("profile-add-claude"));
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalled());
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "account", name: "", active: false, account: null },
    ]);
    fireEvent.click(screen.getByTestId("profile-add-cancel-claude"));
    await waitFor(() => expect(api.cancelLogin).toHaveBeenCalled());
    fireLogin("claude", "cancelled");
    await waitFor(() => expect(api.deleteProfile).toHaveBeenCalledWith("claude", "account"));
    expect(await screen.findByText(zh.account.addRolledBack)).toBeTruthy();
  });

  /** 取消前一刻其实已经登完（后端复查到账号）：留着，不能把刚登好的账号删了。 */
  it("登录成功不撤账号", async () => {
    api.createProfile.mockResolvedValue("account");
    api.loginAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("profile-add-claude"));
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalled());
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "account", name: "", active: false, account: { email: "new@b.c" } },
    ]);
    fireLogin("claude", "success");
    expect((await screen.findByTestId("profile-claude-account")).querySelector(".acct-name")?.textContent).toBe("new@b.c");
    expect(api.deleteProfile).not.toHaveBeenCalled();
  });

  /** 老后端取消时查的是默认账号，默认账号登着就报 success：按新账号自己的凭据判断，照样撤掉。 */
  it("报了 success 但新账号其实没登上，照样撤掉", async () => {
    api.createProfile.mockResolvedValue("account");
    api.loginAgent.mockResolvedValue(undefined);
    api.deleteProfile.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("profile-add-claude"));
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalled());
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "account", name: "", active: false, account: null },
    ]);
    fireLogin("claude", "success");
    await waitFor(() => expect(api.deleteProfile).toHaveBeenCalledWith("claude", "account"));
  });

  it("拉起登录失败也撤掉刚建的账号并说明", async () => {
    api.createProfile.mockResolvedValue("account");
    api.loginAgent.mockRejectedValue(new Error("启动终端失败"));
    api.deleteProfile.mockResolvedValue(undefined);
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" } },
      { id: "account", name: "", active: false, account: null },
    ]);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("profile-add-claude"));
    await waitFor(() => expect(api.deleteProfile).toHaveBeenCalledWith("claude", "account"));
    expect((await screen.findByRole("alert")).textContent).toContain(zh.account.loginFailed);
  });
});

describe("AccountSection 移除与改名", () => {
  const twoProfiles = () =>
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: false, account: { email: "a@b.c" } },
      { id: "work", name: "工作", active: true, account: { email: "w@b.c" } }, // 正在使用
    ]);

  /** 正在使用的账号也能移除（后端把活跃标记落回系统默认）；确认框走 appConfirm，说清删什么不删什么。 */
  it("正在使用的账号也能移除，确认框说历史保留", async () => {
    twoProfiles();
    api.deleteProfile.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("profile-remove-claude-work"));
    await waitFor(() => expect(dialog.confirm).toHaveBeenCalled());
    expect(dialog.confirm.mock.calls[0][0]).toBe(zh.account.removeAccountConfirmShared("工作"));
    expect(dialog.confirm.mock.calls[0][1]).toMatchObject({ danger: true, confirmLabel: zh.account.removeAccountConfirmLabel });
    await waitFor(() => expect(api.deleteProfile).toHaveBeenCalledWith("claude", "work"));
  });

  it("确认框点取消 → 不删", async () => {
    twoProfiles();
    dialog.confirm.mockResolvedValue(false);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("profile-remove-claude-work"));
    await waitFor(() => expect(dialog.confirm).toHaveBeenCalled());
    expect(api.deleteProfile).not.toHaveBeenCalled();
  });

  /** 系统默认是 agent 自己的目录：没有移除、改名，也不再有退出登录（照 Orca）。 */
  it("系统默认卡没有移除和改名", async () => {
    twoProfiles();
    render(<AccountSection />);
    await screen.findByTestId("profile-claude-__default__");
    expect(screen.queryByTestId("profile-remove-claude-__default__")).toBeNull();
    expect(screen.queryByTestId("profile-rename-claude-__default__")).toBeNull();
  });

  /** 改名只动展示名，不动 id——id 是目录名，改了等于换了个账号。 */
  it("改名弹窗：保存新名字", async () => {
    twoProfiles();
    api.renameProfile.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("profile-rename-claude-work"));
    const input = await screen.findByTestId("profile-rename-input");
    expect(screen.getByText(zh.account.renameHint("w@b.c"))).toBeTruthy();
    fireEvent.change(input, { target: { value: "个人" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(api.renameProfile).toHaveBeenCalledWith("claude", "work", "个人"));
  });

  /** 照 Orca：名字留空 = 显示登录邮箱；「恢复默认」就是存空名字。 */
  it("改名弹窗：恢复默认存空名字", async () => {
    twoProfiles();
    api.renameProfile.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("profile-rename-claude-work"));
    fireEvent.click(await screen.findByTestId("profile-rename-reset"));
    await waitFor(() => expect(api.renameProfile).toHaveBeenCalledWith("claude", "work", ""));
  });

  it("改名可以取消——不调后端", async () => {
    twoProfiles();
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("profile-rename-claude-work"));
    const input = await screen.findByTestId("profile-rename-input");
    fireEvent.change(input, { target: { value: "别的名字" } });
    fireEvent.click(screen.getByText(zh.account.cancelEdit));
    await waitFor(() => expect(screen.queryByTestId("profile-rename-input")).toBeNull());
    expect(api.renameProfile).not.toHaveBeenCalled();
  });

  /** 账号区顶部「共用 / 各自」按后端 profile_shares 出：claude 会话历史共用，opencode 的归到「各自」。 */
  it("共用/各自两排按插件声明出；没共用会话历史的归到各自", async () => {
    api.listAgents.mockResolvedValue(descriptors(["claude", "opencode"]));
    twoProfiles();
    render(<AccountSection />);
    const claude = await screen.findByTestId("profile-share-claude");
    const chips = (el: Element, own: boolean) =>
      [...el.querySelectorAll(own ? ".profile-chip-own" : ".profile-chip:not(.profile-chip-own)")].map((c) => c.textContent);
    expect(chips(claude, false)).toEqual(["规则", "设置", "插件", "skill", "命令", "MCP", "会话历史"]);
    expect(chips(claude, true)).toEqual(["登录", "额度"]);
    expect(screen.getByTestId("profiles-claude").textContent).not.toContain("相互独立");

    await selectAgent("OpenCode");
    const opencode = await screen.findByTestId("profile-share-opencode");
    expect(chips(opencode, false)).toEqual(["skill"]);
    expect(chips(opencode, true)).toEqual(["登录", "额度", "会话历史"]);
  });

  /** 首次对齐留存的冲突：摊开在该账号下面，列出条目和文件数，能在文件夹中显示。 */
  it("账号有留存的冲突时在它下面摊开提醒，点按钮在文件夹中显示", async () => {
    api.listProfiles.mockResolvedValue([
      { id: null, name: "", active: true, account: { email: "a@b.c" }, set_aside: [] },
      {
        id: "work", name: "工作", active: false, account: { email: "w@b.c" },
        set_aside: [
          { path: "C:/p/work/keybindings.json.meowo-profile-merge", name: "keybindings.json.meowo-profile-merge", files: 1 },
          { path: "C:/p/work/plugins.meowo-profile-merge", name: "plugins.meowo-profile-merge", files: 111 },
        ],
      },
    ]);
    api.revealTerminalPath.mockResolvedValue(undefined);
    render(<AccountSection />);
    const note = await screen.findByTestId("profile-aside-claude-work");
    expect(note.textContent).toContain(zh.account.setAsideTitle(2));
    expect(note.textContent).toContain("plugins.meowo-profile-merge" + zh.account.setAsideFiles(111));
    // 紧跟在该账号行后面，不在列表底部。
    expect(screen.getByTestId("profile-claude-work").nextElementSibling).toBe(note);
    expect(screen.queryByTestId("profile-aside-claude-default")).toBeNull();
    fireEvent.click(screen.getByTestId("profile-aside-reveal-claude-work"));
    await waitFor(() => expect(api.revealTerminalPath).toHaveBeenCalledWith("", "C:/p/work/keybindings.json.meowo-profile-merge"));
  });

  /** 列表读失败不再静默当成空列表（看着像账号全丢了），就地报错。 */
  it("账号列表读取失败时就地报错", async () => {
    api.listProfiles.mockRejectedValue("boom");
    render(<AccountSection />);
    expect((await screen.findByTestId("profiles-claude")).textContent).toContain("boom");
  });
});

describe("AccountSection 版本与更新", () => {
  const updates = (
    list: Array<{
      provider: string;
      installed_version: string | null;
      latest_version: string | null;
      update_available: boolean;
    }>,
  ) => api.checkAgentUpdates.mockResolvedValue(list);

  it("两段式：联网未返回时先显示本机版本，返回后补上更新入口", async () => {
    api.installedAgentVersions.mockResolvedValue([
      { provider: "claude", installed_version: "1.2.3", latest_version: null, update_available: false },
    ]);
    let resolveFull!: (v: unknown) => void;
    api.checkAgentUpdates.mockReturnValue(new Promise((r) => { resolveFull = r; }));
    render(<AccountSection />);
    const row = await screen.findByTestId("agent-version-claude");
    expect(row.textContent).toContain("1.2.3");
    expect(screen.queryByTestId("agent-update-claude")).toBeNull();
    resolveFull([{ provider: "claude", installed_version: "1.2.3", latest_version: "1.3.0", update_available: true }]);
    await screen.findByTestId("agent-update-claude");

    // 聚焦重查：本机阶段返回空表（后端出错吞成 []）时不得清掉已有的更新入口。
    api.installedAgentVersions.mockResolvedValue([]);
    api.checkAgentUpdates.mockReturnValue(new Promise(() => {}));
    act(() => { window.dispatchEvent(new Event("focus")); });
    await waitFor(() => expect(api.installedAgentVersions.mock.calls.length).toBeGreaterThanOrEqual(2));
    await act(async () => {});
    expect(screen.getByTestId("agent-update-claude")).toBeTruthy();
  });

  it("无新版：显示当前版本，无更新按钮", async () => {
    updates([{ provider: "claude", installed_version: "1.2.3", latest_version: "1.2.3", update_available: false }]);
    render(<AccountSection />);
    const row = await screen.findByTestId("agent-version-claude");
    expect(row.textContent).toContain("1.2.3");
    expect(screen.queryByTestId("agent-update-claude")).toBeNull();
    expect(screen.queryByTestId("agent-update-available-claude")).toBeNull();
  });

  it("有新版：显示新版号与更新按钮；有活跃托管会话时先弹危险确认，确认后走安装入口", async () => {
    updates([{ provider: "claude", installed_version: "1.2.3", latest_version: "1.3.0", update_available: true }]);
    api.getLiveSessionsPage.mockResolvedValue({
      items: [
        { connected: true, pty_managed: true, provider: "claude" },
        { connected: true, pty_managed: true, provider: "codex" }, // 别家的不算
        { connected: false, pty_managed: true, provider: "claude" }, // 断连的不算
      ],
      next_cursor: null,
    });
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    expect((await screen.findByTestId("agent-update-available-claude")).textContent).toContain("1.3.0");
    fireEvent.click(screen.getByTestId("agent-update-claude"));
    // 只数本 provider 的 connected && pty_managed 会话 → 1 个，弹 danger 确认。
    await waitFor(() => expect(dialog.confirm).toHaveBeenCalled());
    expect(dialog.confirm.mock.calls[0][0]).toContain("1");
    expect(dialog.confirm.mock.calls[0][1]).toMatchObject({ danger: true });
    // 确认（beforeEach 默认 resolve true）→ 走与「安装」同一入口（装的就是 latest）。
    await waitFor(() => expect(api.installAgent).toHaveBeenCalledWith("claude"));
    // install-done 成功后重查版本（后端在安装成功后已失效版本缓存）。
    fireDone("claude", true);
    await waitFor(() => expect(api.checkAgentUpdates.mock.calls.length).toBeGreaterThanOrEqual(2));
  });

  it("确认框点取消 → 不更新", async () => {
    updates([{ provider: "claude", installed_version: "1.2.3", latest_version: "1.3.0", update_available: true }]);
    api.getLiveSessionsPage.mockResolvedValue({
      items: [{ connected: true, pty_managed: true, provider: "claude" }],
      next_cursor: null,
    });
    dialog.confirm.mockResolvedValue(false);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-update-claude"));
    await waitFor(() => expect(dialog.confirm).toHaveBeenCalled());
    expect(api.installAgent).not.toHaveBeenCalled();
  });

  it("无活跃托管会话：不弹确认直接装", async () => {
    updates([{ provider: "claude", installed_version: "1.2.3", latest_version: "1.3.0", update_available: true }]);
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-update-claude"));
    await waitFor(() => expect(api.installAgent).toHaveBeenCalledWith("claude"));
    expect(dialog.confirm).not.toHaveBeenCalled();
  });

  it("latest_version 为 null（unknown）：只显示当前版本，无更新入口", async () => {
    updates([{ provider: "claude", installed_version: "1.2.3", latest_version: null, update_available: false }]);
    render(<AccountSection />);
    const row = await screen.findByTestId("agent-version-claude");
    expect(row.textContent).toContain("1.2.3");
    expect(screen.queryByTestId("agent-update-claude")).toBeNull();
    expect(screen.queryByTestId("agent-update-available-claude")).toBeNull();
  });

  it("installed_version 为 null：版本行整块不渲染", async () => {
    updates([{ provider: "claude", installed_version: null, latest_version: "1.3.0", update_available: true }]);
    render(<AccountSection />);
    await screen.findByTestId("agent-card-claude");
    // 探测在 listAgents 之后串行触发，等它兑现后再断言「什么都没有」。
    await waitFor(() => expect(api.checkAgentUpdates).toHaveBeenCalled());
    expect(screen.queryByTestId("agent-version-claude")).toBeNull();
    expect(screen.queryByTestId("agent-update-claude")).toBeNull();
  });

  it("checkAgentUpdates 失败：页面正常渲染，无版本 UI、无红字", async () => {
    api.checkAgentUpdates.mockRejectedValue(new Error("boom"));
    render(<AccountSection />);
    await screen.findByTestId("agent-card-claude");
    await waitFor(() => expect(api.checkAgentUpdates).toHaveBeenCalled());
    expect(screen.queryByTestId("agent-version-claude")).toBeNull();
    expect(document.querySelector(".agent-install-error")).toBeNull();
  });
});

describe("AccountSection：发图工具没接上（mcp_config_unreadable）", () => {
  const FILE = "C:\Users\me\.kimi\config.toml";

  it("字段有值就提示：一句结论、原因、文件路径，点「在文件夹中显示」交给 reveal；不看是哪家", async () => {
    // 故意放在 claude 上：判据只是字段有无，不按 Provider 名。
    api.listAgents.mockResolvedValue(
      descriptors(["claude", "codex"]).map((agent) => (agent.id === "claude" ? { ...agent, mcp_config_unreadable: FILE } : agent)),
    );
    api.revealTerminalPath.mockResolvedValue(undefined);
    render(<AccountSection />);
    const note = await screen.findByTestId("agent-mcp-unreadable-claude");
    expect(note.textContent).toContain(zh.account.mcpConfigUnreadable);
    expect(note.textContent).toContain("读不懂");
    expect(note.textContent).toContain(FILE);
    fireEvent.click(screen.getByTestId("agent-mcp-reveal-claude"));
    expect(api.revealTerminalPath).toHaveBeenCalledWith("", FILE);
    // 没有这个字段的 agent 不提示。
    await selectAgent("Codex");
    await screen.findByTestId("agent-card-codex");
    expect(screen.queryByTestId("agent-mcp-unreadable-codex")).toBeNull();
  });

  it("「在文件夹中显示」失败时就地写原因", async () => {
    api.listAgents.mockResolvedValue(descriptors(["kimi"]).map((agent) => ({ ...agent, mcp_config_unreadable: FILE })));
    api.getAccounts.mockResolvedValue([]);
    api.revealTerminalPath.mockRejectedValue("not found");
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-mcp-reveal-kimi"));
    await waitFor(() => expect(screen.getByTestId("agent-mcp-unreadable-kimi").querySelector(".agent-mcp-unreadable-err")).toBeTruthy());

  });
});
