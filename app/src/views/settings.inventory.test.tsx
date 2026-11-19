import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

// 设置页精简（2026-11-19）的「不丢设置」清单测试。
//
// ITEMS 是改前（main 上 cecd0b49 的 About.tsx 及各分区）设置窗里每一个用户能改的项与动作，
// before 写改前的界面标题（及所在分区），field 写它读写的 settings 字段或动作；后面三项是新结构里的位置：
// 分区、行标题（找行用）、控件的无障碍名（开关类会真的拨一下，确认写回的还是原字段）。
// 每一项都要：① 在指定分区里找得到；② 用行标题搜索时这一行可见（搜索结果按新分区平铺）；
// ③ 开关拨一次，store 里的原字段跟着变。
//
// 平台：灵动岛、任务栏、防休眠这些行只在 Windows / macOS 出现，CI 的 vitest 跑在 Linux——先把 UA
// 钉成 Windows（模块顶层按 UA 判平台，必须在导入组件之前）。
vi.hoisted(() => {
  Object.defineProperty(window.navigator, "userAgent", { value: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) jsdom", configurable: true });
});
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn(() => Promise.resolve("0.8.5")) }));
vi.mock("../useUpdate", () => ({ useUpdate: () => ({ status: "idle", version: null }) }));
vi.mock("../useShowWhenReady", () => ({ useShowWhenReady: () => {} }));

import { clearMocks } from "@tauri-apps/api/mocks";
import { installMocks, setExtraMock, store } from "../demo/mock";
import { About, resolveSection, type Section } from "./About";
import { SETTINGS_SECTION_KEY } from "../settingsNav";
import { zh } from "../i18n/zh";

const AGENTS = [
  { id: "claude", display_name: "Claude Code", installed: true, supports_profiles: true, supports_account: true, supports_proxy: true, proxy_covers_all_launches: true, proxy_accepts_socks: false, launch_options: [
    { id: "permission", default: "default", choices: [{ id: "default", label: "Default", args: [], risk: false }, { id: "plan", label: "Plan", args: [], risk: false }] },
    { id: "model", default: "default", choices: [{ id: "default", label: "Default", args: [], risk: false }, { id: "opus", label: "Opus", args: [], risk: false }] },
  ] },
  { id: "codex", display_name: "Codex", installed: true, supports_proxy: true, proxy_covers_all_launches: false, proxy_accepts_socks: false, launch_options: [] },
];
const EXTENSIONS = [
  { provider: "claude", displayName: "Claude Code", dataDir: "C:\\Users\\demo\\.claude", groups: [
    { kind: "mcp", source: "C:\\Users\\demo\\.claude.json", shared: false, items: [
      { name: "playwright", description: null, rel: null, detail: "stdio · npx @playwright/mcp", enabled: true, managed: false },
    ] },
  ] },
];

beforeAll(() => {
  setExtraMock((cmd, args) => {
    // demo mock 不接 set_settings：这里替后端落盘，拨开关后才能断言写回的是哪个字段。
    if (cmd === "set_settings") { store.settings = (args as { settings: typeof store.settings }).settings; return null; }
    if (cmd === "list_agents") return AGENTS;
    if (cmd === "list_agent_extensions") return EXTENSIONS;
    if (cmd === "available_terminals") return ["wt", "powershell"];
    // 装着 Lead：「有关卡待拍板时」防休眠子行只在装了 Lead 后出现（与改前同一条件）。
    if (cmd === "lead_workflow_installed") return true;
    if (cmd === "lead_provider_installations") return [{ provider: "codex", name: "Codex", profileId: null, profileName: null, version: "1", configured: true, supported: true, managed: false, installed: false, assetsInstalled: false, hooksInstalled: false, automaticContinue: false, notes: [], reason: null }];
    if (cmd === "installed_agent_versions" || cmd === "check_agent_updates") return [];
    if (cmd === "get_autostart") return false;
    if (cmd === "get_effective_proxy") return null;
    if (cmd === "git_tool_status") return { found: true, path: "C:\\git.exe", version: "2.47", userName: "Larry", userEmail: "l@example.com", error: null };
    if (cmd === "forge_accounts") return { accounts: [], credentialSupported: true, pending: null };
    if (cmd === "remote_access_info") return { enabled: true, port: 18620, boundPort: 18620, token: "tok", lastError: null, ips: [{ ip: "100.64.0.8", kind: "tailscale" }] };
    return undefined;
  });
});

let baseSettings: Record<string, unknown>;
beforeEach(() => {
  localStorage.clear();
  clearMocks();
  installMocks("about");
  baseSettings ??= JSON.parse(JSON.stringify(store.settings));
  store.settings = JSON.parse(JSON.stringify(baseSettings));
  Object.assign(store.settings, { remote_access_enabled: true, notifications_enabled: true, chat_enabled: true, bark_url: "https://api.day.app/KEY/" });
  // 自定义代理 + codex 单独设了自定义地址：代理地址框、各 Agent 的模式与地址都在页面上。
  store.settings.proxy = { mode: "custom", url: "http://127.0.0.1:7890", per_agent: { codex: { mode: "custom", url: "http://10.0.0.2:8080" } } };
});
afterEach(() => cleanup());

type Item = {
  /** 改前的界面标题（分区）。 */
  before: string;
  /** 读写的 settings 字段，或动作。 */
  field: string;
  sec: Section;
  /** 新结构里的行标题：找行、搜索都用它。 */
  label: string;
  /** 开关的无障碍名：给了就拨一下，断言 field 被写回（布尔字段取反、数组字段变化）。 */
  toggle?: string;
  /** 没有行标题的项（地址输入框）：自己定位到那一行；query 是用来搜它的词。 */
  find?: () => HTMLElement | null;
  query?: string;
};

const k = zh.settings.islandAlertKinds;
const ITEMS: Item[] = [
  // ── 改前「通用」 ──
  { before: "通用 · 语言", field: "language", sec: "general", label: zh.settings.language },
  { before: "通用 · 开机自启", field: "动作 set_autostart", sec: "general", label: zh.settings.autostart },
  { before: "通用 · 自动更新", field: "auto_update_enabled", sec: "general", label: zh.settings.autoUpdate, toggle: zh.settings.autoUpdate },
  { before: "通用 · 运行 · 对话窗口功能", field: "chat_enabled", sec: "general", label: zh.settings.chatFeature, toggle: zh.settings.chatFeature },
  { before: "通用 · 运行 · 运行时防止休眠", field: "keep_awake_while_running", sec: "general", label: zh.settings.keepAwakeRunning, toggle: `${zh.settings.keepAwakeGroup} · ${zh.settings.keepAwakeRunning}` },
  { before: "通用 · 运行 · 允许 Agent 操作面板浏览器", field: "browser_agent", sec: "general", label: zh.settings.browserAgent, toggle: zh.settings.browserAgent },
  { before: "通用 · Lead 工作流 · Lead 工作流", field: "动作 set_lead_workflow", sec: "general", label: zh.settings.leadWorkflow },
  { before: "通用 · Lead 工作流 · 其他 CLI（说明 + 刷新）", field: "动作 lead_provider_installations / 刷新", sec: "general", label: zh.settings.leadProviders },
  { before: "通用 · Lead 工作流 · 各 CLI 安装 / 卸载", field: "动作 set_lead_provider_enabled", sec: "general", label: "Codex" },
  { before: "通用 · Lead 工作流 · 关卡待拍板时防止休眠", field: "keep_awake_for_lead_gate", sec: "general", label: zh.settings.keepAwakeLeadGate, toggle: `${zh.settings.keepAwakeGroup} · ${zh.settings.keepAwakeLeadGate}` },
  { before: "通用 · 通知 · 桌面通知", field: "notifications_enabled", sec: "notify", label: zh.settings.notify, toggle: zh.settings.notify },
  ...(["waiting", "approval", "question", "error"] as const).map((kind): Item => (
    { before: `通用 · 通知 · 通知这些事 · ${k[kind]}`, field: "notify_kinds", sec: "notify", label: k[kind], toggle: `${zh.settings.notifyColSystem} · ${k[kind]}` })),
  { before: "通用 · 通知 · 通知提示音", field: "notification_sound", sec: "notify", label: zh.settings.notifySound, toggle: zh.settings.notifySound },
  { before: "通用 · 通知 · 任务栏提醒", field: "attention_flash_enabled", sec: "notify", label: zh.settings.attentionFlash, toggle: zh.settings.attentionFlash },
  // ── 改前「会话」 ──
  { before: "会话 · 默认 Agent", field: "default_agent", sec: "sessions", label: zh.settings.defaultAgent },
  { before: "会话 · 默认权限", field: "localStorage 新建会话选项", sec: "sessions", label: zh.newSession.defaultOf(zh.newSession.launchOption.permission ?? "permission") },
  { before: "会话 · 默认模型", field: "localStorage 新建会话选项", sec: "sessions", label: zh.newSession.defaultOf(zh.newSession.launchOption.model ?? "model") },
  { before: "会话 · 打开会话", field: "session_open_in", sec: "sessions", label: zh.settings.sessionOpenIn },
  { before: "会话 · 新 worktree 的位置", field: "worktree.location", sec: "sessions", label: zh.wtnew.settingsLocation },
  { before: "会话 · 分支前缀", field: "worktree.branch_prefix", sec: "sessions", label: zh.wtnew.settingsPrefix },
  { before: "会话 · 默认终端", field: "resume_terminal", sec: "sessions", label: zh.settings.resumeTerm },
  { before: "会话 · 会话列表 · 会话菜单", field: "card_menu_mode", sec: "sessions", label: zh.settings.cardMenu },
  { before: "会话 · 已完成会话 · 在会话管理中查看", field: "动作 open_session_admin", sec: "sessions", label: zh.settings.archivedSessions },
  // ── 改前「外观」 ──
  { before: "外观 · 外观模式", field: "theme", sec: "appearance", label: zh.settings.theme },
  { before: "外观 · 界面缩放", field: "ui_scale", sec: "appearance", label: zh.settings.fontSize },
  { before: "外观 · 对话内容宽度", field: "chat_content_width", sec: "appearance", label: zh.settings.chatWidth },
  { before: "外观 · 消息自动折叠", field: "chat_turn_folding", sec: "appearance", label: zh.settings.turnFolding, toggle: zh.settings.turnFolding },
  { before: "外观 · 灵动岛 · 极简模式", field: "island_strip", sec: "appearance", label: zh.settings.islandStrip, toggle: zh.settings.islandStrip },
  { before: "外观 · 灵动岛 · 避开最大化窗口", field: "island_auto_strip", sec: "appearance", label: zh.settings.islandAutoStrip, toggle: zh.settings.islandAutoStrip },
  { before: "外观 · 灵动岛 · 收起时显示", field: "island_compact", sec: "appearance", label: zh.settings.islandCompact },
  { before: "外观 · 灵动岛 · 提醒停留", field: "island_alert_seconds", sec: "appearance", label: zh.settings.islandAlertSeconds },
  ...(["waiting", "approval", "question", "error"] as const).map((kind): Item => (
    { before: `外观 · 灵动岛 · 自动弹出提醒 · ${k[kind]}`, field: "island_alerts", sec: "notify", label: k[kind], toggle: `${zh.settings.notifyColIsland} · ${k[kind]}` })),
  { before: "外观 · 终端 · 终端字号", field: "terminal_font_size", sec: "appearance", label: zh.settings.termFontSize },
  { before: "外观 · 终端 · 终端行高", field: "terminal_line_height", sec: "appearance", label: zh.settings.termLineHeight },
  { before: "外观 · 终端 · 终端回滚行数", field: "terminal_scrollback", sec: "appearance", label: zh.settings.termScrollback },
  // ── 改前「手机远程」 ──
  { before: "手机远程 · 开启远程访问", field: "remote_access_enabled", sec: "remote", label: zh.remote.enable, toggle: zh.remote.enable },
  { before: "手机远程 · 端口", field: "remote_access_port", sec: "remote", label: zh.remote.port },
  { before: "手机远程 · 绑定网卡", field: "remote_access_bind", sec: "remote", label: zh.remote.bind },
  { before: "手机远程 · 扫码连接（复制地址 / 重新生成令牌）", field: "动作 copy / regenerate_remote_token", sec: "remote", label: zh.remote.scan },
  { before: "手机远程 · 手机推送（Bark）", field: "bark_url / bark_paused", sec: "notify", label: zh.remote.pushLabel, toggle: zh.remote.pushToggle },
  // ── 改前「网络」 ──
  { before: "网络 · 默认代理（+ 地址）", field: "proxy.mode / proxy.url", sec: "general", label: zh.proxy.mode },
  { before: "网络 · 按 Agent 覆盖", field: "proxy.per_agent", sec: "general", label: zh.proxy.perAgent },
  { before: "网络 · 默认代理 · 自定义地址", field: "proxy.url", sec: "general", label: "代理地址框", query: "代理",
    find: () => (document.querySelector<HTMLInputElement>('.main-sec[data-sec="general"] input[value="http://127.0.0.1:7890"]')?.closest<HTMLElement>(".row") ?? null) },
  { before: "网络 · 按 Agent 覆盖 · Codex 的模式", field: "proxy.per_agent.codex.mode", sec: "general", label: "Codex 代理模式", query: "Codex",
    find: () => Array.from(document.querySelectorAll<HTMLElement>('.main-sec[data-sec="general"] .proxy-agent-name')).find((el) => el.textContent === "Codex")?.closest<HTMLElement>(".row") ?? null },
  { before: "网络 · 按 Agent 覆盖 · Codex 的自定义地址", field: "proxy.per_agent.codex.url", sec: "general", label: "Codex 代理地址框", query: "网络",
    find: () => (document.querySelector<HTMLInputElement>('.main-sec[data-sec="general"] input[value="http://10.0.0.2:8080"]')?.closest<HTMLElement>(".row") ?? null) },
  // ── 改前「代码托管」（本次未改，只验位置与搜索） ──
  { before: "代码托管 · user.name", field: "git config user.name", sec: "github", label: zh.codeHosting.userName },
  { before: "代码托管 · user.email", field: "git config user.email", sec: "github", label: zh.codeHosting.userEmail },
  // ── 改前「账号与用量」「扩展」→ Agent 页 ──
  { before: "账号与用量 · 账号卡 · 在灵动岛显示配额", field: "sticker_quota_providers", sec: "account", label: zh.settings.showQuotaOnSticker, toggle: zh.settings.showQuotaOnSticker },
  { before: "扩展 · MCP / 技能 / 钩子（只读，在文件夹中显示）", field: "动作 reveal_path", sec: "account", label: "playwright" },
  // ── 改前「关于」 ──
  { before: "关于 · 版本信息（检查更新）", field: "动作 open_update_window", sec: "about", label: zh.about.versionInfo },
  { before: "关于 · 官方网站", field: "动作 open_url", sec: "about", label: zh.about.website },
  { before: "关于 · 源码仓库", field: "动作 open_url", sec: "about", label: zh.about.source },
  { before: "关于 · 使用引导", field: "动作 open_onboarding", sec: "about", label: zh.about.guide },
  { before: "关于 · 诊断日志", field: "动作 export_diag_log", sec: "about", label: zh.about.diagLog },
];

/** 元素自己和祖先（到 .main-body 为止）都没被搜索过滤或分区切换藏起来。 */
const shown = (el: Element) => {
  for (let cur: HTMLElement | null = el as HTMLElement; cur && !cur.classList.contains("main-body"); cur = cur.parentElement) {
    if (cur.hidden || cur.style.display === "none") return false;
  }
  return true;
};
/** 分区里以 label 为行标题（或行内文本）的那一行。 */
const rowIn = (sec: Section, label: string) => {
  const root = document.querySelector(`.main-sec[data-sec="${sec}"]`);
  if (!root) return null;
  return Array.from(root.querySelectorAll<HTMLElement>(".row, .usage-sticker-row")).find((row) =>
    Array.from(row.querySelectorAll(".row-label, .proxy-agent-name, .usage-sticker-label")).some((el) => el.textContent === label)) ?? null;
};
const navTo = (sec: Section) => {
  const button = Array.from(document.querySelectorAll<HTMLButtonElement>(".side-nav .nav-item")).find((b) => b.textContent?.startsWith(zh.settings.nav[sec]));
  fireEvent.click(button!);
};
const fieldValue = (field: string) => JSON.stringify(field.split(/[ /]/)[0]!.split(".").reduce<unknown>((v, key) => (v as Record<string, unknown> | undefined)?.[key], store.settings));

describe("不丢设置：改前每一项在新结构里都在、搜得到、写回原字段", () => {
  it(`清单共 ${ITEMS.length} 项，覆盖改前十个分区`, () => {
    const before = new Set(ITEMS.map((item) => item.before.split(" · ")[0]));
    expect([...before].sort()).toEqual(["代码托管", "会话", "关于", "外观", "手机远程", "扩展", "账号与用量", "通用", "网络"].sort());
  });

  for (const item of ITEMS) {
    it(`${item.before}（${item.field}）→ ${zh.settings.nav[item.sec]} · ${item.label}`, async () => {
      // 扩展的行在 Agent 页的「扩展」段：按改前的分区 id 进入，同时验旧 id 的映射。
      if (item.before.startsWith("扩展")) localStorage.setItem(SETTINGS_SECTION_KEY, "extensions");
      render(<About />);
      await screen.findByPlaceholderText(zh.settings.searchPlaceholder);
      if (!item.before.startsWith("扩展")) navTo(item.sec);
      const row = await waitFor(() => {
        const found = item.find ? item.find() : rowIn(item.sec, item.label);
        expect(found, `${zh.settings.nav[item.sec]} 分区里找不到「${item.label}」`).toBeTruthy();
        return found!;
      });
      expect(shown(row)).toBe(true);

      if (item.toggle) {
        const sw = screen.getByRole("switch", { name: item.toggle });
        await waitFor(() => expect((sw as HTMLButtonElement).disabled).toBe(false));
        const field = item.field.split(" / ").at(-1)!;
        const before = fieldValue(field);
        await act(async () => { fireEvent.click(sw); });
        await waitFor(() => expect(fieldValue(field)).not.toBe(before));
      }

      // 搜索：用行标题搜，这一行可见；所在分区的标题也在（搜索结果按新分区平铺）。
      const input = screen.getByPlaceholderText(zh.settings.searchPlaceholder);
      const query = item.query ?? item.label;
      fireEvent.change(input, { target: { value: query } });
      await waitFor(() => {
        const hit = item.find ? item.find() : rowIn(item.sec, item.label);
        expect(hit && shown(hit), `搜「${query}」时 ${item.sec} 分区「${item.label}」不可见`).toBe(true);
      });
      const title = document.querySelector<HTMLElement>(`.main-sec[data-sec="${item.sec}"] .sec-title`)!;
      expect(shown(title)).toBe(true);
    });
  }
});

describe("分区结构", () => {
  it("导航 9 项，按 通用、外观、通知、会话、Agent、代码托管、手机远程、快捷键、关于 排列", async () => {
    render(<About />);
    await screen.findByPlaceholderText(zh.settings.searchPlaceholder);
    const labels = Array.from(document.querySelectorAll(".side-nav .nav-item span:not(.nav-tag)")).map((el) => el.textContent);
    expect(labels).toEqual(["通用", "外观", "通知", "会话", "Agent", "代码托管", "手机远程", "快捷键", "关于"]);
  });

  it("旧分区 id 映射到新位置：network → 通用，extensions → Agent 页的扩展段，account → Agent 页账号段", () => {
    expect(resolveSection("network")).toEqual({ sec: "general" });
    expect(resolveSection("extensions")).toEqual({ sec: "account", agentView: "extensions" });
    expect(resolveSection("account")).toEqual({ sec: "account", agentView: "account" });
    expect(resolveSection("github")).toEqual({ sec: "github" });
    expect(resolveSection("bogus")).toBeNull();
  });

  for (const [stored, sec, extra] of [
    ["network", "general", zh.proxy.mode],
    ["extensions", "account", "playwright"],
    ["account", "account", zh.settings.showQuotaOnSticker],
    ["github", "github", zh.codeHosting.userName],
    ["notify", "notify", zh.settings.notify],
  ] as const) {
    it(`记住的 / 外部入口给的分区「${stored}」进窗落在 ${sec}，不是空白页`, async () => {
      localStorage.setItem(SETTINGS_SECTION_KEY, stored);
      render(<About />);
      await waitFor(() => expect(rowIn(sec, extra)).toBeTruthy());
      const visible = Array.from(document.querySelectorAll<HTMLElement>(".main-sec")).filter((el) => !el.hidden);
      expect(visible.map((el) => el.dataset.sec)).toEqual([sec]);
      expect(document.querySelector(".side-nav .nav-item.on")?.textContent).toBe(zh.settings.nav[sec]);
    });
  }

  it("窗口开着时外部入口 openSettingsAt(\"extensions\") 经 storage 事件切到 Agent 页扩展段；再给 account 切回账号段", async () => {
    render(<About />);
    await screen.findByPlaceholderText(zh.settings.searchPlaceholder);
    act(() => { window.dispatchEvent(new StorageEvent("storage", { key: SETTINGS_SECTION_KEY, newValue: "extensions" })); });
    await waitFor(() => expect(rowIn("account", "playwright")).toBeTruthy());
    expect(screen.getByRole("radio", { name: zh.settings.agentViewExtensions }).getAttribute("aria-checked")).toBe("true");
    act(() => { window.dispatchEvent(new StorageEvent("storage", { key: SETTINGS_SECTION_KEY, newValue: "account" })); });
    await waitFor(() => expect(rowIn("account", "playwright")).toBeNull());
    expect(screen.getByRole("radio", { name: zh.settings.agentViewAccount }).getAttribute("aria-checked")).toBe("true");
  });

  it("Agent 页只有一个 Agent 选择器，账号与扩展两段共用；切段记进分区键", async () => {
    localStorage.setItem(SETTINGS_SECTION_KEY, "account");
    render(<About />);
    await waitFor(() => expect(screen.getByRole("radio", { name: zh.settings.agentViewExtensions })).toBeTruthy());
    fireEvent.click(screen.getByRole("radio", { name: zh.settings.agentViewExtensions }));
    await waitFor(() => expect(rowIn("account", "playwright")).toBeTruthy());
    expect(document.querySelectorAll(".main-sec[data-sec=account] .account-agent-switch .dd-btn")).toHaveLength(1);
    expect(localStorage.getItem(SETTINGS_SECTION_KEY)).toBe("extensions");
  });
});

describe("搜旧分区名「网络」与「代理」：只出代理这一组，地址与各 Agent 行都在（审查 2026-11-19 第 1 条）", () => {
  const urlRow = (value: string) => document.querySelector<HTMLInputElement>(`.main-sec[data-sec="general"] input[value="${value}"]`)?.closest(".row") ?? null;
  for (const q of ["网络", "代理", "network", "proxy"]) {
    it(`搜「${q}」：网络代理、地址框、各 Agent 单独设置与各家行可见，通用页其余卡片隐藏`, async () => {
      render(<About />);
      const input = await screen.findByPlaceholderText(zh.settings.searchPlaceholder);
      fireEvent.change(input, { target: { value: q } });
      await waitFor(() => expect(urlRow("http://10.0.0.2:8080")).toBeTruthy());
      expect(shown(rowIn("general", zh.proxy.mode)!)).toBe(true);
      expect(shown(urlRow("http://127.0.0.1:7890")!)).toBe(true);
      expect(shown(rowIn("general", zh.proxy.perAgent)!)).toBe(true);
      // 按 .proxy-agent-name 找：通用页 Lead 组里也有一行「Codex」（其他 CLI），那一行理应被滤掉。
      const agentRow = (name: string) => Array.from(document.querySelectorAll('.main-sec[data-sec="general"] .proxy-agent-name')).find((el) => el.textContent === name)?.closest(".row") ?? null;
      for (const name of ["Claude Code", "Codex"]) expect(shown(agentRow(name)!), name).toBe(true);
      expect(shown(urlRow("http://10.0.0.2:8080")!)).toBe(true);
      // 搜索词挂在「网络代理」这一行上，不挂在分区名上：通用页别的卡片不跟着出来。
      for (const label of [zh.settings.language, zh.settings.autoUpdate, zh.settings.chatFeature, zh.settings.leadWorkflow]) {
        const row = rowIn("general", label)!;
        expect(shown(row), label).toBe(false);
        expect(shown(row.closest(".row-card")!), label).toBe(false);
      }
    });
  }

  it("搜索结果里把代理切到「自定义」：新出现的地址框不被过滤掉", async () => {
    store.settings.proxy = { mode: "off", url: "", per_agent: {} };
    render(<About />);
    const input = await screen.findByPlaceholderText(zh.settings.searchPlaceholder);
    // 「默认代理」是改前网络页的行标题，现在只命中「网络代理」这一行本身，不靠分区名带出整页。
    fireEvent.change(input, { target: { value: "网络代理" } });
    const custom = await waitFor(() => {
      const el = screen.getAllByRole("radio", { name: zh.proxy.custom }).find((r) => shown(r));
      expect(el).toBeTruthy();
      return el!;
    });
    fireEvent.click(custom);
    const box = await screen.findByPlaceholderText(zh.proxy.urlPlaceholder);
    await waitFor(() => expect(shown(box)).toBe(true));
  });
});

describe("按新结构搜索", () => {
  const search = async (q: string) => {
    render(<About />);
    const input = await screen.findByPlaceholderText(zh.settings.searchPlaceholder);
    // 搜索态才挂载全部分区（通知页此前没访问过，矩阵还不在 DOM 里）。
    fireEvent.change(input, { target: { value: q } });
    await waitFor(() => expect(document.querySelector(".notify-matrix")).toBeTruthy());
  };
  const matrixRowsShown = () => Array.from(document.querySelectorAll(".notify-matrix-row")).filter(shown).map((r) => r.querySelector(".row-label")!.textContent);

  it("搜事件名（审批）：矩阵里那一行和列头都在，别的事件让位", async () => {
    await search("批准");
    await waitFor(() => expect(matrixRowsShown()).toEqual([k.approval]));
    expect(shown(document.querySelector(".notify-matrix-head")!)).toBe(true);
  });

  it("搜列名（灵动岛弹出）：列头命中，四个事件整组带出", async () => {
    await search(zh.settings.notifyColIsland);
    await waitFor(() => expect(matrixRowsShown()).toHaveLength(4));
  });

  it("搜「通知」：通知页每一个可见的表头行下面都有控件，不留空表头", async () => {
    await search("通知");
    await waitFor(() => expect(matrixRowsShown()).toHaveLength(4));
    for (const row of Array.from(document.querySelectorAll<HTMLElement>(".main-body .row")).filter(shown)) {
      if (row.classList.contains("chat-sronly")) continue;
      const hasControl = !!row.querySelector("button, input, [role=switch], [role=radiogroup]");
      const next = row.nextElementSibling;
      const subsShown = !!next && next.classList.contains("row-sub") && shown(next);
      // 矩阵列头没有行标题（只有列名），只看带标题的行。
      expect(hasControl || subsShown || !row.querySelector(".row-label"), `空表头：${row.textContent}`).toBe(true);
    }
  });

  it("搜「休眠」：防止电脑休眠的表头和两个子开关同进退", async () => {
    await search("休眠");
    await waitFor(() => expect(shown(rowIn("general", zh.settings.keepAwakeRunning)!)).toBe(true));
    expect(shown(rowIn("general", zh.settings.keepAwakeLeadGate)!)).toBe(true);
    expect(shown(rowIn("general", zh.settings.keepAwakeGroup)!)).toBe(true);
  });
});
