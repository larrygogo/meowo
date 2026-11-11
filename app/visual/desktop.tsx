// 视觉回归 harness（桌面窗口）：按 ?scene= 挂真实组件，后端全部换成 src/demo/mock 的内存假数据。
// 接线照抄 src/demo/chatEmbed.tsx / panelEmbed.tsx 与 src/main.tsx，避免截到与真实窗口不同的外观。
// 场景：chat / chat-long / panel-changes / panel-diff / island-compact / island-panel / island-alert /
//       settings-<分区> / settings-search / settings-noresults / settingswin-<分区>（真实窗口尺寸）/
//       onboarding / onboarding-1..4 / updater-<状态> / confirm[-danger|-alert|-long] /
//       newsession-window[-full] / newsession-page[-wt] / crash。主题 ?theme=dark|light，语言 ?lang=zh|en。
import "../src/demo/isolatedStorage"; // 必须第一个：内存 localStorage，每次打开都是干净起点
import React from "react";
import ReactDOM from "react-dom/client";
import { detectHostOs } from "../src/platform";
import { bootAppearance } from "../src/appearance";
import { I18nProvider } from "../src/i18n";
import { TooltipLayer } from "../src/Tooltip";
import { installInputModality } from "../src/input-modality";
import { installMocks, setExtraMock, store } from "../src/demo/mock";
import { chatMock } from "../src/demo/chatData";
import { panelMock } from "../src/demo/panelData";
import { makeSession } from "../src/demo/data";
import { savePanel } from "../src/views/chat/panelTabs";
import { SETTINGS_SECTION_KEY } from "../src/settingsNav";
import { CHAT_SESSION_ID, FIXED_NOW, markReady, scmMock, withLongItems } from "./fixtures";
import "../src/fonts";
import "../src/styles.css";

const q = new URLSearchParams(location.search);
const lang: "zh" | "en" = q.get("lang") === "en" ? "en" : "zh";
const theme = q.get("theme") === "light" ? "light" : "dark";
const scene = q.get("scene") ?? "chat";

Date.now = () => FIXED_NOW;
store.settings.language = lang;
store.settings.theme = theme;

const noop = () => {};

/** 设置输入框的值并触发 React 的 onChange（受控输入直接改 value 不会通知 React）。 */
function typeInto(el: HTMLInputElement, value: string): void {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(el, value);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}
/** 等某个元素出现后做一件事（场景要替用户点一下、填一下时用），只做一次。 */
function whenReady<T extends Element>(selector: string, act: (el: T) => void): void {
  const timer = window.setInterval(() => {
    const el = document.querySelector<T>(selector);
    if (!el) return;
    window.clearInterval(timer);
    act(el);
  }, 100);
}

// 带启动选项的 agent 名单：设置「会话」分区的默认值行、新建会话的权限 / 模型下拉都靠它才出现。
const AGENTS_WITH_OPTIONS = [
  { id: "claude", display_name: "Claude Code", installed: true, supports_profiles: true, launch_options: [
    { id: "permission", default: "default", choices: [
      { id: "default", label: "Default", args: [], risk: false },
      { id: "acceptEdits", label: "Accept edits", args: [], risk: false },
      { id: "bypassPermissions", label: "Bypass permissions", args: [], risk: true },
    ] },
    { id: "model", default: "default", choices: [
      { id: "default", label: "Default", args: [], risk: false },
      { id: "opus", label: "Opus 5", args: [], risk: false },
      { id: "sonnet", label: "Sonnet 5", args: [], risk: false },
    ] },
  ] },
  { id: "codex", display_name: "Codex", installed: true, launch_options: [] },
  { id: "kimi", display_name: "Kimi Code", installed: true, launch_options: [] },
  { id: "gemini", display_name: "Gemini CLI", installed: false, launch_options: [] },
  { id: "opencode", display_name: "opencode", installed: false, launch_options: [] },
];
const RECENT_DIRS = ["C:\\Users\\demo\\workspace\\web-app", "C:\\Users\\demo\\workspace\\billing-service", "D:\\code\\infra\\a-very-long-monorepo-name\\packages\\export-api"];
const EXTENSIONS = [
  { provider: "claude", displayName: "Claude Code", dataDir: "C:\\Users\\demo\\.claude", groups: [
    { kind: "skill", source: "user", shared: false, items: [
      { name: "lead", description: "Leader 工作流：用户只描述一个想要的结果，由 agent 团队完成需求挖掘、多方案、实施和独立做空验证，并拿出证据交付。", rel: "skills/lead", detail: null, enabled: null, managed: true },
      { name: "pr-description", description: "写 PR 描述、commit message 时使用。", rel: "skills/pr-description", detail: null, enabled: null, managed: false },
    ] },
    { kind: "mcp", source: "user", shared: false, items: [
      { name: "meowo", description: null, rel: null, detail: "stdio · meowo-reporter mcp", enabled: true, managed: true },
      { name: "playwright", description: null, rel: null, detail: "stdio · npx @playwright/mcp", enabled: false, managed: false },
    ] },
    { kind: "hook", source: "user", shared: false, items: [
      { name: "PreToolUse", description: null, rel: null, detail: "meowo-reporter hook", enabled: null, managed: true },
    ] },
  ] },
  { provider: "codex", displayName: "Codex", dataDir: "C:\\Users\\demo\\.codex", groups: [] },
];

async function boot(): Promise<React.ReactNode> {
  if (scene === "chat" || scene === "chat-long" || scene.startsWith("panel-")) {
    window.history.replaceState({}, "", `${location.pathname}?${q.toString()}&sessionId=${CHAT_SESSION_ID}`);
    const chat = chatMock(lang);
    const panel = panelMock();
    if (scene.startsWith("panel-")) {
      savePanel(CHAT_SESSION_ID, { tabs: [{ key: "diff", type: "diff" }, { key: "files", type: "files" }], active: "diff", visible: true, expanded: false });
    }
    setExtraMock((cmd, args) => {
      if (cmd === "get_chat_history") return scene.startsWith("chat") ? withLongItems(chat(cmd), lang) : chat(cmd);
      return scmMock(cmd) ?? panel(cmd, args) ?? chat(cmd);
    });
    installMocks("chat");
    bootAppearance({ scale: true });
    const { ChatWindow } = await import("../src/views/ChatWindow");
    if (scene === "chat-long") {
      // 对话默认贴底，长内容（表格、代码块、长输出工具卡、报错工具卡）在视口上方：
      // 把表格滚到顶部附近。贴底逻辑可能在后续轮询里再拉回去，所以在前 1.5s（假时钟）里反复对齐。
      // 顺手把折叠的工具组展开一次，让工具卡（长输出、报错）露出来。
      const started = performance.now();
      let unfolded = false;
      const timer = window.setInterval(() => {
        const scroller = document.querySelector<HTMLElement>(".chat-scroll");
        if (!unfolded && scroller?.querySelector("table")) {
          unfolded = true;
          scroller.querySelectorAll<HTMLElement>('.turn-fold-head[aria-expanded="false"]').forEach((b) => b.click());
          return;
        }
        const table = scroller?.querySelector("table");
        if (scroller && table) {
          scroller.scrollTop += table.getBoundingClientRect().top - scroller.getBoundingClientRect().top - 240;
        }
        if (performance.now() - started > 1500) window.clearInterval(timer);
      }, 100);
    }
    if (scene === "panel-diff") {
      // 改动页没有「默认打开某文件」的入口：等列表出来替用户点开 LoginForm.tsx。
      // （panelEmbed.tsx 还按旧的 .chat-diff-tree-file[data-tip] 找行；「更改」页已换成 SourceControl
      // 的 .scm-row，这里按文件名匹配。）
      const timer = window.setInterval(() => {
        const row = [...document.querySelectorAll<HTMLElement>(".scm-row-main")]
          .find((el) => el.querySelector(".scm-name")?.textContent === "LoginForm.tsx");
        if (row) { row.click(); window.clearInterval(timer); }
      }, 100);
    }
    return <div className="demo-chat-frame" style={{ position: "fixed", inset: 0, overflow: "hidden" }}><ChatWindow /></div>;
  }

  if (scene.startsWith("island-")) {
    installMocks("main");
    bootAppearance({ scale: true, panel: true });
    const { Island } = await import("../src/views/Island");
    const { IslandPanel } = await import("../src/views/island/IslandPanel");
    const titles = lang === "en"
      ? ["Refactor login form validation", "Fix duplicate charge in payment callback", "Add tests for the export API", "Investigate the nightly build failure on the ARM64 runner"]
      : ["重构登录页表单校验", "修支付回调的重复扣款", "补齐导出接口的单测", "排查夜间构建在 ARM64 runner 上偶发失败的原因"];
    const kinds = ["pending", "error", "running", "waiting"] as const;
    const sessions = kinds.map((k, i) => {
      const s = makeSession({ title: titles[i], project: ["web-app", "billing", "api", "infra"][i], provider: (["claude", "kimi", "codex", "gemini"] as const)[i], status: k === "waiting" ? "waiting" : "running", agoMin: i * 3, todoDone: 1, todoTotal: 3 });
      s.session.id = i + 1;
      if (k === "pending") s.pending_review = "approval";
      if (k === "error") { s.errored = true; s.error_label = "API Error: 529"; }
      if (k === "waiting") s.unread = true;
      return s;
    });
    // 岛在真机上是透明窗口浮在桌面上：给一块固定的中性灰底，深浅主题下都看得清边缘。
    const stage: React.CSSProperties = { position: "fixed", inset: 0, background: "#5b6b7a", display: "flex", justifyContent: "center", alignItems: "flex-start", paddingTop: 12 };
    let el: React.ReactNode;
    if (scene === "island-panel") {
      el = (
        <div className="island-shell" style={{ width: 600, "--island-pill-w": "380px", "--island-pill-h": "46px" } as React.CSSProperties}>
          <IslandPanel sessions={sessions} usage={{}} quotaProviders={[]} onOpenSession={noop} onMeasure={noop} onPointerDownDrag={noop} onNewSession={noop} onOpenChat={noop} onOpenSettings={noop} onToggleStrip={noop} />
        </div>
      );
    } else {
      const alert = scene === "island-alert"
        ? { sessionId: 1, kind: "approval", title: titles[0], project: "web-app", detail: null }
        : null;
      el = (
        <div style={alert ? { width: 548, height: 68 } : { height: 46 }}>
          <Island sessions={sessions} compactStyle="counts" alert={alert} alertQueued={alert ? 2 : 0} usages={[{ provider: "claude", pct: 72 }]} onMeasure={noop} onActivate={noop} onOpenSession={noop} onPointerDownDrag={noop} />
        </div>
      );
    }
    return <div style={stage} data-visual-stage>{el}</div>;
  }

  if (scene.startsWith("settings-") || scene.startsWith("settingswin-")) {
    // settingswin-* 与 settings-* 内容相同，只是拍摄尺寸换成真实窗口的 620×460（见 shoot.mjs）。
    const section = scene.slice(scene.indexOf("-") + 1);
    const searchQuery = section === "search" ? (lang === "en" ? "notif" : "通知") : section === "noresults" ? "zzzz" : null;
    try { localStorage.setItem(SETTINGS_SECTION_KEY, searchQuery ? "general" : section); } catch { /* 内存存储不会抛 */ }
    if (searchQuery) whenReady<HTMLInputElement>(".side-search input", (el) => typeInto(el, searchQuery));
    // 账号分区要版本探测、远程分区要配对信息：演示 mock 不管这几条，返回 null 会让分区崩掉。
    store.settings.remote_access_enabled = true;
    // 网络分区拍「自定义代理」：地址输入行与各 Agent 的单独设置只在这一档出现。
    if (section === "network") store.settings.proxy = { mode: "custom", url: "http://127.0.0.1:7890", per_agent: {} };
    setExtraMock((cmd) => {
      if (cmd === "installed_agent_versions" || cmd === "check_agent_updates") return [];
      if (cmd === "list_agents") return AGENTS_WITH_OPTIONS;
      if (cmd === "list_agent_extensions") return EXTENSIONS;
      if (cmd === "available_terminals") return ["wt", "powershell", "cmd"];
      if (cmd === "lead_workflow_installed") return true;
      if (cmd === "check_provider_hooks") return "installed";
      if (cmd === "plugin:app|version") return "0.6.20";
      if (cmd === "list_profiles") {
        const account = (email: string, plan: string) => ({ email, display_name: null, organization: null, plan, login_label: null });
        return [
          { id: null, name: "", active: true, account: account("demo@example.com", "Pro") },
          { id: "work", name: "work-account-with-a-rather-long-name@company.example.com", active: false, account: account("work-account-with-a-rather-long-name@company.example.com", "Max") },
          { id: "spare", name: "备用", active: false, account: null },
        ];
      }
      if (cmd === "remote_access_info") {
        return {
          enabled: true, port: 18620, token: "visual-regression-token", lastError: null, boundPort: 18620,
          ips: [{ ip: "100.64.0.8", kind: "tailscale" }, { ip: "192.168.1.20", kind: "lan" }],
        };
      }
      return undefined;
    });
    installMocks("about");
    bootAppearance({ scale: false });
    const { About } = await import("../src/views/About");
    return <About />;
  }

  if (scene === "onboarding" || scene.startsWith("onboarding-")) {
    const step = Number(scene.split("-")[1] ?? "1");
    setExtraMock((cmd) => (cmd === "list_agents" ? AGENTS_WITH_OPTIONS : undefined));
    installMocks("onboarding");
    bootAppearance({ scale: false });
    const { Onboarding } = await import("../src/views/Onboarding");
    // 后三步没有直达入口：等圆点出来替用户点到那一步。
    if (step > 1) whenReady<HTMLElement>(`.ob-dot:nth-child(${step})`, (el) => el.click());
    return <Onboarding />;
  }

  if (scene.startsWith("updater-")) {
    // 更新窗六种状态：检查中 / 已最新 / 失败 / 发现新版（带更新说明）/ 下载中 / 安装中。
    const state = scene.slice("updater-".length);
    const never = new Promise(() => {});
    const notes = lang === "en"
      ? "## What's new\n\n- The composer is always fully visible, even in narrow windows\n- Keyboard and screen reader support across the chat window\n- Long sessions open about 14x faster\n\n## Fixes\n\n- Discarding changes now moves files to the recycle bin\n- The changes panel no longer freezes with tens of thousands of untracked files"
      : "## 新功能\n\n- 输入框永远完整可见，窄窗下也不会被挤掉\n- 对话窗全面支持键盘与读屏\n- 长会话首次打开快约 14 倍\n\n## 修复\n\n- 放弃更改改为进回收站，不再直接删除\n- 「更改」面板扛住上万个未跟踪文件，不再白屏卡死\n- 灵动岛在最大化窗口前台时自动收成细条";
    const found = (downloadState: string) => ({ version: "0.7.0", body: notes, downloadState });
    setExtraMock((cmd, args) => {
      if (cmd === "plugin:app|version") return "0.6.20";
      if (cmd === "check_update") {
        if (state === "checking") return never;
        if (state === "latest") return null;
        if (state === "error") throw new Error("network");
        return found(state === "installing" ? "ready" : state === "downloading" ? "downloading" : "available");
      }
      if (cmd === "install_downloaded_update") return never;
      if (cmd === "get_live_sessions_page") return { items: [], next_cursor: null };
      // 下载进度靠后端事件推：登记监听时回放一条 42% 的进度事件。
      if (cmd === "plugin:event|listen") {
        const a = args as { event?: string; handler?: number };
        if (state === "downloading" && a.event === "update-download-progress" && a.handler != null) {
          const internals = (window as unknown as { __TAURI_INTERNALS__?: { runCallback?: (id: number, data: unknown) => void } }).__TAURI_INTERNALS__;
          window.setTimeout(() => internals?.runCallback?.(a.handler!, { event: a.event, id: 1, payload: { downloaded: 42, contentLength: 100 } }), 200);
        }
        return 1;
      }
      return undefined;
    });
    installMocks("updater");
    bootAppearance({ scale: false });
    const { Updater } = await import("../src/views/Updater");
    if (state === "installing") whenReady<HTMLElement>(".up-actions .btn-primary", (el) => el.click());
    return <Updater />;
  }

  if (scene === "confirm" || scene.startsWith("confirm-")) {
    const kind = scene.split("-")[1] ?? "plain";
    const payload = kind === "alert"
      ? { title: lang === "en" ? "Export failed" : "导出失败", message: lang === "en" ? "The diagnostic log could not be written. Check that the folder is writable." : "诊断日志没能写入，请检查目标文件夹是否可写。", danger: false, confirm_label: null, alert: true }
      : kind === "danger"
        ? { title: lang === "en" ? "Delete worktree" : "删除 worktree", message: lang === "en" ? "This deletes the folder and its local branch." : "会删除这个目录和它的本地分支。", danger: true, confirm_label: lang === "en" ? "Delete" : "删除" }
        : kind === "long"
          ? { title: lang === "en" ? "Delete worktree" : "删除 worktree", message: "以下程序正在占用这个目录，继续会先结束它们：\nnode.exe (PID 18244)\nesbuild.exe (PID 20112)\n\n会一起删掉这些没有提交的本地文件：\napp/.env.local\napp/src/views/Draft.tsx\nnotes/todo.md\nC:\\Users\\demo\\workspace\\web-app-feature-login\\a\\very\\long\\path\\that\\should\\wrap\\instead\\of\\overflowing.txt\n\n这个操作不能撤销。删除前请确认上面的文件都不再需要，或者已经另存到别处；分支上没有推送的提交也会一并丢失。", danger: true, confirm_label: lang === "en" ? "Delete" : "删除" }
          : { title: lang === "en" ? "End session" : "结束会话", message: lang === "en" ? "The agent will stop. You can resume it later." : "Agent 会停止运行，之后可以再恢复。", danger: false, confirm_label: lang === "en" ? "End" : "结束" };
    setExtraMock((cmd) => (cmd === "confirm_dialog_payload" ? payload : undefined));
    installMocks("confirm-1");
    bootAppearance({ scale: false });
    const { ConfirmWindow } = await import("../src/views/ConfirmWindow");
    return <ConfirmWindow />;
  }

  if (scene.startsWith("newsession-")) {
    const variant = scene.slice("newsession-".length);
    const full = variant.endsWith("-full");
    const withWorktree = variant.endsWith("-wt") || full;
    setExtraMock((cmd) => {
      if (cmd === "list_agents") return AGENTS_WITH_OPTIONS;
      if (cmd === "recent_cwds") return RECENT_DIRS;
      // -full：把警告条也拍出来（接入项没装、目录重名）。
      if (cmd === "check_provider_hooks") return full ? "missing" : "installed";
      if (cmd === "worktree_preview") {
        return {
          repoRoot: RECENT_DIRS[0], originRoot: RECENT_DIRS[0], repoName: "web-app", slug: "login-form", branch: "meowo/login-form",
          path: "C:\\Users\\demo\\workspace\\web-app-login-form", base: "origin/main", dirExists: full, branchExists: false,
          branchCheckedOut: false, suggestedSlug: "login-form-2", suggestedBranch: "meowo/login-form-2", hasSetup: true, subDir: null, subDirMissing: false,
        };
      }
      if (cmd === "get_session_admin_page") {
        const titles = lang === "en" ? ["Refactor login form validation", "Fix duplicate charge in payment callback", "Add tests for the export API"] : ["重构登录页表单校验", "修支付回调的重复扣款", "补齐导出接口的单测，顺带把超长标题在这一行里的省略也验一下看看效果到底怎么样"];
        const items = titles.map((title, i) => {
          const s = makeSession({ title, project: "web-app", provider: (["claude", "codex", "kimi"] as const)[i], status: "waiting", agoMin: 5 + i * 47 });
          s.session.id = 100 + i;
          return s;
        });
        return { items, total: 12 };
      }
      return undefined;
    });
    const tick = () => { if (withWorktree) whenReady<HTMLInputElement>('[data-testid="wt-toggle"]', (el) => { if (!el.checked) el.click(); }); };
    if (variant.startsWith("window")) {
      installMocks("new-session");
      bootAppearance({ scale: false });
      const { NewSessionPanel } = await import("../src/views/NewSessionPanel");
      tick();
      return <NewSessionPanel prefill={{ cwd: RECENT_DIRS[0], provider: "claude" }} onClose={noop} />;
    }
    // 新建会话页长在对话窗的主列里：这里只搭它的外层容器（.chat-window > .chat-body > .chat-main），
    // 侧栏与标题栏归别的车道，不在这张图里。
    installMocks("chat");
    bootAppearance({ scale: true });
    const { NewSessionPage } = await import("../src/views/chat/NewSessionPage");
    tick();
    return (
      <div className="chat-window" style={{ position: "fixed", inset: 0 }}>
        <div className="chat-body">
          <div className="chat-main">
            <NewSessionPage prefill={{ cwd: RECENT_DIRS[0], provider: "claude" }} onLaunched={noop} onOpenSession={noop} onMoreSessions={noop} />
          </div>
        </div>
      </div>
    );
  }

  if (scene === "crash") {
    installMocks("about");
    bootAppearance({ scale: false });
    const { WindowCrash } = await import("../src/ErrorBoundary");
    const { DevBadge } = await import("../src/views/DevBadge");
    return <><WindowCrash /><DevBadge /></>;
  }

  throw new Error("未知场景：" + scene);
}

void (async () => {
  await detectHostOs();
  installInputModality();
  const node = await boot();
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <I18nProvider initial={lang}>
      {node}
      <TooltipLayer />
    </I18nProvider>,
  );
  markReady();
})();
