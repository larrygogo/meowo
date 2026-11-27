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
import { checkpointSceneMock } from "./checkpointScene";
import { reviewSceneMock } from "./reviewScene";
import { makeSession } from "../src/demo/data";
import { savePanel } from "../src/views/chat/panelTabs";
import { SETTINGS_SECTION_KEY } from "../src/settingsNav";
import { CHAT_SESSION_ID, FIXED_NOW, flowStateItems, markReady, questionInput, scmMock, withLongItems } from "./fixtures";
import { qaFlowBoot, qaFlowMock } from "./qaFlow";
import { tmsChatMock, tmsCommonMock, tmsProviderOf } from "./tmsFixtures";
import type { ProviderUsage } from "../src/api";
import type { IslandAlertEvent } from "../src/generated/contracts/IslandAlertEvent";
import "../src/fonts";
import "../src/styles.css";

const q = new URLSearchParams(location.search);
const lang: "zh" | "en" = q.get("lang") === "en" ? "en" : "zh";
const theme = q.get("theme") === "light" ? "light" : "dark";
const scene = q.get("scene") ?? "chat";

// 默认把 Date.now 冻结在 FIXED_NOW。?clock=live：不冻结，时间跟着截图脚本的假时钟走（它同样从 FIXED_NOW 起步，
// 只在脚本推进时前进），用来拍要等时间过去才出现的状态（审批卡倒计时归零后的「已过期」）。
if (q.get("clock") !== "live") Date.now = () => FIXED_NOW;
// 截图里关掉 WebGL，让 xterm 退回 DOM 渲染（ManagedTerminal / ShellTerminal 加载 WebglAddon 失败时自己兜底）：
// WebGL 画布不保留绘制缓冲，Playwright 截图那一帧重新合成时画布是空的，托管终端的八个场景
// （qa-term*）终端区整片漆黑，搜索命中、链接提示都看不到（做空审查 2026-11-11）。
const getContext = HTMLCanvasElement.prototype.getContext;
HTMLCanvasElement.prototype.getContext = function (this: HTMLCanvasElement, kind: string, ...rest: unknown[]) {
  if (kind === "webgl" || kind === "webgl2") return null;
  return (getContext as (...a: unknown[]) => unknown).call(this, kind, ...rest);
} as typeof HTMLCanvasElement.prototype.getContext;
store.settings.language = lang;
store.settings.theme = theme;
// ?ui=1.5：界面缩放（只有对话窗与灵动岛吃，设置等定尺寸窗口的 bootAppearance 不下发）。
if (Number(q.get("ui")) > 0) store.settings.ui_scale = Math.round(Number(q.get("ui")) * 100);

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
  // 多账号照 Orca 共用（2026-11-27）：账号区顶部「共用 / 各自」两排按 profile_shares 出，切换后运行中的会话那一行按
  // moves_sessions_across_accounts 出。
  { id: "claude", display_name: "Claude Code", installed: true, supports_profiles: true, moves_sessions_across_accounts: true,
    profile_shares: ["rules", "settings", "plugins", "skills", "commands", "mcp", "history"], launch_options: [
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
  ], supports_proxy: true, proxy_covers_all_launches: true, proxy_accepts_socks: false },
  // supports_proxy：通用页「各 Agent 单独设置」代理只列支持代理的已装 agent，缺了这三项那一行拍不出来。
  { id: "codex", display_name: "Codex", installed: true, launch_options: [], supports_proxy: true, proxy_covers_all_launches: false, proxy_accepts_socks: false },
  { id: "kimi", display_name: "Kimi Code", installed: true, launch_options: [], supports_proxy: true, proxy_covers_all_launches: false, proxy_accepts_socks: true },
  { id: "antigravity", display_name: "Antigravity CLI", installed: false, launch_options: [] },
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
  // 任务 20261114-tui-model-switch 截图验收：四家原生 TUI 会话（tms-<家>?state=…）与新建页（tms-new-<家>）。
  if (scene.startsWith("tms-new-")) {
    const provider = tmsProviderOf(scene);
    setExtraMock(tmsCommonMock(provider, q.get("cfg")));
    installMocks("chat");
    bootAppearance({ scale: true });
    const { NewSessionPage } = await import("../src/views/chat/NewSessionPage");
    return (
      <div className="chat-window" style={{ position: "fixed", inset: 0 }}>
        <div className="chat-body">
          <div className="chat-main">
            <NewSessionPage prefill={{ cwd: RECENT_DIRS[0], provider }} onLaunched={noop} onOpenSession={noop} onMoreSessions={noop} />
          </div>
        </div>
      </div>
    );
  }
  if (scene.startsWith("tms-")) {
    const provider = tmsProviderOf(scene);
    window.history.replaceState({}, "", `${location.pathname}?${q.toString()}&sessionId=${CHAT_SESSION_ID}`);
    const chat = chatMock(lang);
    const tms = tmsChatMock(provider, q.get("state") ?? "idle", chat, q.get("modes"));
    setExtraMock((cmd, args) => tms(cmd, args) ?? scmMock(cmd) ?? chat(cmd));
    installMocks("chat");
    bootAppearance({ scale: true });
    const { ChatWindow } = await import("../src/views/ChatWindow");
    return <div className="demo-chat-frame" style={{ position: "fixed", inset: 0, overflow: "hidden" }}><ChatWindow /></div>;
  }
  if (scene === "chat" || scene === "chat-long" || scene.startsWith("panel-")) {
    window.history.replaceState({}, "", `${location.pathname}?${q.toString()}&sessionId=${CHAT_SESSION_ID}`);
    const chat = chatMock(lang);
    const panel = panelMock();
    if (scene.startsWith("panel-")) {
      savePanel(CHAT_SESSION_ID, { tabs: [{ key: "diff", type: "diff" }, { key: "files", type: "files" }], active: scene === "panel-files" ? "files" : "diff", visible: true, expanded: q.get("max") === "1" });
    }
    setExtraMock((cmd, args) => {
      // ?usage=warn|limited|expired：对话页上下文卡片里「此对话使用的账号」的几种状态（照 Orca 对照用）。
      const usageCase = q.get("usage");
      // ?usage=multi：底部状态条面板里的多账号列表（系统默认 + 两个已登录 + 一个已退出，照 Orca 对照用）。
      if (usageCase === "multi" && cmd === "list_profiles") {
        const acct = (email: string, plan: string) => ({ email, display_name: null, login_label: null, plan, org: null });
        return [
          { id: null, name: "", active: true, account: acct("me@example.com", "Max 5x"), set_aside: [] },
          { id: "work", name: "工作", active: false, account: acct("work@example.com", "Max 20x"), set_aside: [] },
          { id: "side", name: "", active: false, account: acct("side@example.com", "Pro"), set_aside: [] },
          { id: "old", name: "", active: false, account: null, set_aside: [] },
        ];
      }
      if (usageCase === "multi" && cmd === "refresh_profile_usage") {
        const at = (h: number) => new Date(Date.now() + h * 3_600_000).toISOString();
        const lane = (kind: string, used_pct: number, h: number, label?: string) => ({ kind, used_pct, used: null, limit: null, unit: null, resets_at: at(h), ...(label ? { label } : {}) });
        const who = (args as { profile?: string | null } | undefined)?.profile ?? null;
        if (who === "side") return Promise.reject("USAGE_EXPIRED");
        if (who === "work") return { note: null, lanes: [lane("five_hour", 93, 1.2), lane("seven_day", 64, 90), lane("model_weekly", 41, 90, "Fable")] };
        return { note: null, lanes: [lane("five_hour", 16, 3.2), lane("seven_day", 81, 84), lane("model_weekly", 23, 84, "Fable")] };
      }
      if (usageCase && cmd === "refresh_profile_usage") {
        if (usageCase === "limited") return Promise.reject("USAGE_RATE_LIMITED:300");
        if (usageCase === "expired") return Promise.reject("USAGE_EXPIRED");
        const at = (h: number) => new Date(Date.now() + h * 3_600_000).toISOString();
        return { note: null, lanes: [
          { kind: "five_hour", used_pct: 84, used: null, limit: null, unit: null, resets_at: at(2.3) },
          { kind: "seven_day", used_pct: 37, used: null, limit: null, unit: null, resets_at: at(61) },
        ] };
      }
      if (cmd === "get_chat_history") {
        if (scene.startsWith("chat")) return withLongItems(chat(cmd), lang);
        // 最大化场景要让「全部 / 本会话 / 本轮」出现：它只在本会话改过文件时才显示，
        // 这里借一条现成的工具调用，改成对「更改」假数据里 LoginForm.tsx 的编辑。
        if (q.get("max") === "1") {
          const h = chat(cmd) as { items: { type: string; id?: string; detail?: unknown }[] };
          const tool = h.items.find((item) => item.type === "tool_use");
          if (tool) return { ...h, items: [...h.items, { ...tool, id: "visual-max-edit", detail: { kind: "edit", path: "C:/dev/web-app/src/components/LoginForm.tsx" } }] };
        }
        // 会话里 agent 在临时 worktree 里另切分支建了 #171（2026-11-16 实拍的形状）：
        // 标题栏和「更改」面板要显示它，而不只是当前分支上已合并的 #162。
        if (scene === "panel-session-pr") {
          const h = chat(cmd) as { items: unknown[] };
          return { ...h, items: [...h.items,
            { type: "tool_use", id: "visual-gh-pr", timestamp: null, name: "Bash", kind: "run", summary: "gh pr create", detail: { kind: "command", command: "cd /tmp/wt-captcha && gh pr create --base main --fill", description: null } },
            { type: "tool_result", id: "visual-gh-pr-r", timestamp: null, tool_use_id: "visual-gh-pr", text: "https://github.com/example/web-app/pull/171\n", is_error: false },
          ] };
        }
        return chat(cmd);
      }
      if (scene === "panel-session-pr") {
        if (cmd === "git_pull_request_by_url") return { number: 171, state: "open", url: "https://github.com/example/web-app/pull/171", title: lang === "zh" ? "登录页加图形验证码" : "Add a captcha to the sign-in page" };
        if (cmd === "git_pull_request") return { number: 162, state: "merged", url: "https://github.com/example/web-app/pull/162", title: lang === "zh" ? "重构登录页表单校验" : "Refactor login form validation" };
        if (cmd === "git_forge_status") {
          const status = scmMock(cmd) as { pr: Record<string, unknown> };
          return { ...status, pr: { ...status.pr, state: "merged" } };
        }
      }
      // ?forge=gitlab：仓库在 gitlab.com，MR !42 带流水线（一项失败）；?forge=noaccount：认得出 GitLab 但没加账号；
      // ?forge=codeberg / bitbucket / gitee：各平台的 PR 区块，侧栏角标与标题栏同一个 PR。
      const forgeKind = q.get("forge");
      const title = lang === "zh" ? "重构登录页表单校验" : "Refactor login form validation";
      const PLATFORM_PR: Record<string, { forge: string; host: string; repo: string; number: number; url: string; methods: string[]; checks: { name: string; workflow: string | null; state: string; url: null }[]; summary: string }> = {
        gitlab: { forge: "gitlab", host: "gitlab.com", repo: "https://gitlab.com/example/web-app", number: 42, url: "https://gitlab.com/example/web-app/-/merge_requests/42", methods: ["merge", "squash"], summary: "fail",
          checks: [{ name: "lint", workflow: "test", state: "pass", url: null }, { name: "unit", workflow: "test", state: "fail", url: null }, { name: "e2e", workflow: "test", state: "pending", url: null }, { name: "deploy-review", workflow: "deploy", state: "skipped", url: null }] },
        codeberg: { forge: "gitea", host: "codeberg.org", repo: "https://codeberg.org/example/web-app", number: 57, url: "https://codeberg.org/example/web-app/pulls/57", methods: ["merge", "squash", "rebase"], summary: "pass",
          checks: [{ name: "ci / test (pull_request)", workflow: null, state: "pass", url: null }, { name: "ci / lint (pull_request)", workflow: null, state: "pass", url: null }, { name: "issue-labels (pull_request_target)", workflow: null, state: "skipped", url: null }] },
        bitbucket: { forge: "bitbucket", host: "bitbucket.org", repo: "https://bitbucket.org/example/web-app", number: 18, url: "https://bitbucket.org/example/web-app/pull-requests/18", methods: ["merge", "squash", "rebase"], summary: "pending",
          checks: [{ name: "Pipeline #231 for refactor/login-schema", workflow: null, state: "pending", url: null }, { name: "SonarCloud", workflow: null, state: "pass", url: null }] },
        gitee: { forge: "gitee", host: "gitee.com", repo: "https://gitee.com/example/web-app", number: 316, url: "https://gitee.com/example/web-app/pulls/316", methods: ["merge", "squash", "rebase"], summary: "none", checks: [] },
      };
      // ?forge=ghnoaccount：仓库在 github.com、没添加账号（Meowo 不再调用本机 gh，与其他平台一样引导去设置添加）。
      if (forgeKind === "ghnoaccount" && cmd === "git_forge_status") {
        return { ...(scmMock(cmd) as Record<string, unknown>), gh: "no_account", forge: "github", forgeHost: "github.com", pr: null, mergeMethods: [], lastMergeMethod: null };
      }
      if (forgeKind === "ghnoaccount" && cmd === "sidebar_git_badges") {
        return ((args as { cwds?: string[] } | undefined)?.cwds ?? []).slice(0, 3).map((cwd, i) => ({ cwd, additions: 12 + i, deletions: 3, files: 2 }));
      }
      const picked = forgeKind ? PLATFORM_PR[forgeKind === "noaccount" ? "gitlab" : forgeKind] : undefined;
      if (picked && cmd === "git_pull_request") return { number: picked.number, state: "open", url: picked.url, title, forge: picked.forge === "gitea" ? "gitea" : picked.forge };
      if (picked && forgeKind !== "noaccount" && cmd === "sidebar_git_badges") {
        const cwds = ((args as { cwds?: string[] } | undefined)?.cwds ?? []).slice(0, 3);
        return cwds.map((cwd, i) => ({ cwd, additions: 12 + i, deletions: 3, files: 2, pr: i === 0 ? { number: picked.number, state: "open", checks: picked.summary, url: picked.url, title, forge: picked.forge } : undefined }));
      }
      if (picked && cmd === "git_forge_status") {
        const status = scmMock(cmd) as Record<string, unknown>;
        const base = { ...status, forge: picked.forge, forgeHost: picked.host, repoUrl: picked.repo, mergeMethods: picked.methods, lastMergeMethod: null,
          compareUrl: picked.forge === "gitlab" ? "https://gitlab.com/example/web-app/-/merge_requests/new?merge_request%5Bsource_branch%5D=refactor%2Flogin-schema&merge_request%5Btarget_branch%5D=main" : null };
        if (forgeKind === "noaccount") return { ...base, gh: "no_account", pr: null };
        return { ...base, pr: {
          number: picked.number, state: "open", url: picked.url, title,
          baseRef: "main", headRef: "refactor/login-schema", headOid: "deadbeef", mergeable: picked.forge === "gitee" ? "UNKNOWN" : "MERGEABLE",
          mergeState: picked.summary === "pass" ? "CLEAN" : picked.forge === "gitee" ? "UNKNOWN" : "BLOCKED", forge: picked.forge,
          checks: picked.checks, checksSummary: picked.summary,
        } };
      }
      return checkpointSceneMock(cmd) ?? reviewSceneMock(cmd) ?? scmMock(cmd) ?? panel(cmd, args) ?? chat(cmd);
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
    if (scene === "panel-pr-review") {
      const timer = window.setInterval(() => {
        const button = [...document.querySelectorAll<HTMLButtonElement>(".scm-forge-btn")].find((el) => el.getAttribute("aria-expanded") === "false");
        if (button) { button.click(); window.clearInterval(timer); }
      }, 100);
    }
    if (scene === "panel-checkpoints") {
      const timer = window.setInterval(() => {
        const button = [...document.querySelectorAll<HTMLButtonElement>(".chat-diff-scope button")].find((el) => el.textContent === (lang === "zh" ? "快照" : "Checkpoints"));
        if (button) { button.click(); window.clearInterval(timer); }
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

  // 重设计补的场景：对话流的各种条目、问答卡、空态，以及键盘唤出的弹层。都挂真实 ChatWindow，
  // 只换 get_chat_history / pending_interaction 的返回值；弹层靠派发真实快捷键打开。
  if (scene.startsWith("flow-") || scene.startsWith("pop-")) {
    window.history.replaceState({}, "", `${location.pathname}?${q.toString()}&sessionId=${CHAT_SESSION_ID}`);
    const chat = chatMock(lang);
    const ts = new Date(FIXED_NOW).toISOString();
    // 原有的 flow-states* / flow-question / flow-empty / pop-* 走下面这张表；其余 flow-<名字> 是补看场景（qaFlow.ts）。
    // pop-idle 也归补看场景：它只是「没有待批准请求的对话窗」，菜单、弹层都从它点出来。
    const legacy = (scene.startsWith("pop-") && scene !== "pop-idle") || scene.startsWith("flow-states") || scene === "flow-question" || scene === "flow-empty";
    const qa = qaFlowMock(scene, lang, chat);
    const flowPanel = panelMock();
    setExtraMock((cmd, args) => {
      if (!legacy) {
        const hit = qa(cmd, args);
        if (hit !== undefined) return hit;
      }
      if (cmd === "pending_interaction") {
        if (scene === "flow-question") {
          const base = (chat(cmd) as { approval: Record<string, unknown> }).approval;
          return { approval: null, question: { ...base, requestId: "visual-question", toolName: "AskUserQuestion", description: null, input: questionInput(lang), answerable: true } };
        }
        return { approval: null, question: null };
      }
      if (cmd === "get_chat_history") {
        const h = chat(cmd) as Record<string, unknown>;
        if (scene === "flow-empty") return { ...h, items: [], todos: [], pendingReview: null, status: "waiting", lastUserText: null, lastAiText: null };
        if (scene === "flow-question") return { ...h, pendingReview: "question" };
        if (scene.startsWith("pop-")) return { ...h, pendingReview: null, status: "waiting" };
        return { ...h, items: flowStateItems(lang, ts), pendingReview: null };
      }
      if (cmd === "get_subagent_transcript") {
        return [{ label: null, items: [
          { type: "tool_use", id: "sa-t1", timestamp: ts, name: "Grep", kind: "search", summary: "loginSchema" },
          { type: "tool_result", id: "sa-r1", timestamp: ts, tool_use_id: "sa-t1", text: "4 matches", is_error: false },
          { type: "assistant_text", id: "sa-a1", timestamp: ts, text: lang === "zh" ? "边界情况都覆盖到了，只有加号别名没测。" : "Edge cases are covered except plus aliases." },
        ] }];
      }
      // 单文件 diff、文件树也给上假数据：对话里点文件跳到面板（panelReveal.ts）时面板里有东西可看。
      return scmMock(cmd) ?? flowPanel(cmd, args) ?? chat(cmd);
    });
    installMocks("chat");
    qaFlowBoot(scene, lang);
    bootAppearance({ scale: true });
    const { ChatWindow } = await import("../src/views/ChatWindow");
    const key = (k: string, init: KeyboardEventInit = {}) =>
      window.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init }));
    const started = performance.now();
    let acted = false;
    const timer = window.setInterval(() => {
      const scroller = document.querySelector<HTMLElement>(".chat-scroll");
      const ta = document.querySelector<HTMLTextAreaElement>(".chat-window textarea");
      if (!acted && ta) {
        acted = true;
        if (scene === "pop-palette") key("k", { ctrlKey: true, code: "KeyK" });
        if (scene === "pop-shortcuts") { (document.activeElement as HTMLElement | null)?.blur(); key("?", { shiftKey: true, code: "Slash" }); }
        // 点一下就弹出的浮层：+ 菜单、进度面板。
        const clickFor: Record<string, string> = {
          "pop-add": ".chat-compose .chat-attach-button",
          "pop-progress": ".dock-workbar .ui-pill",
        };
        if (clickFor[scene]) document.querySelector<HTMLElement>(clickFor[scene])?.click();
        return;
      }
      if (scene.startsWith("flow-states") && scroller) {
        // 把工具卡与子任务展开一次（只点还没开的），再按场景滚到顶或中段，默认留在底部。
        scroller.querySelectorAll<HTMLElement>("details.chat-tool:not([open]) > summary, details.chat-subagent:not([open]) > summary").forEach((el) => {
          if (!el.dataset.visualOpened) { el.dataset.visualOpened = "1"; el.click(); }
        });
        if (scene === "flow-states") scroller.scrollTop = scroller.scrollHeight;
        if (scene === "flow-states-top") scroller.scrollTop = 0;
        if (scene === "flow-states-mid") scroller.scrollTop = Math.round((scroller.scrollHeight - scroller.clientHeight) * 0.5);
      }
      if (performance.now() - started > 1500) window.clearInterval(timer);
    }, 100);
    return <div className="demo-chat-frame" style={{ position: "fixed", inset: 0, overflow: "hidden" }}><ChatWindow /></div>;
  }

  if (scene.startsWith("island-")) {
    installMocks("main");
    bootAppearance({ scale: true, panel: true });
    const { Island, IslandStrip } = await import("../src/views/Island");
    const { IslandPanel } = await import("../src/views/island/IslandPanel");
    const titles = lang === "en"
      ? ["Refactor login form validation", "Fix duplicate charge in payment callback", "Add tests for the export API", "Investigate the nightly build failure on the ARM64 runner"]
      : ["重构登录页表单校验", "修支付回调的重复扣款", "补齐导出接口的单测", "排查夜间构建在 ARM64 runner 上偶发失败的原因"];
    const kinds = ["pending", "error", "running", "waiting"] as const;
    const sessions = kinds.map((k, i) => {
      const s = makeSession({ title: titles[i], project: ["web-app", "billing", "api", "infra"][i], provider: (["claude", "kimi", "codex", "antigravity"] as const)[i], status: k === "waiting" ? "waiting" : "running", agoMin: i * 3, todoDone: 1, todoTotal: 3 });
      s.session.id = i + 1;
      if (k === "pending") s.pending_review = "approval";
      if (k === "error") { s.errored = true; s.error_label = "API Error: 529"; }
      if (k === "waiting") s.unread = true;
      return s;
    });
    // 岛在真机上是透明窗口浮在桌面上：给一块固定的中性灰底，深浅主题下都看得清边缘。
    const stage: React.CSSProperties = { position: "fixed", inset: 0, background: "#5b6b7a", display: "flex", justifyContent: "center", alignItems: "flex-start", paddingTop: 12 };
    let el: React.ReactNode;
    // 落点：场景名以 -menubar 结尾的按 macOS 菜单栏弹出面板画（四角圆、底下是系统毛玻璃，这里垫一块渐变近似）。
    const menubar = scene.endsWith("-menubar");
    if (menubar) {
      document.documentElement.classList.add("island-menubar", "platform-macos");
      stage.background = "linear-gradient(135deg, #3a4654, #6b5a4c)";
    }
    // 额度页：两家、多档（含模型周限的长名字），一家快用完。
    const lane = (kind: string, used_pct: number, label?: string) => ({ kind, used_pct, used: null, limit: null, unit: null, resets_at: null, label });
    const usage = {
      claude: { note: null, lanes: [lane("five_hour", 42), lane("seven_day", 68), lane("model_weekly", 91, "Fable")] },
      codex: { note: null, lanes: [lane("five_hour", 12), lane("weekly", 35)] },
    } as unknown as Record<string, ProviderUsage>;
    const panelProps = { onOpenSession: noop, onMeasure: noop, onPointerDownDrag: noop, onNewSession: noop, onOpenChat: noop, onOpenSettings: noop, onToggleStrip: noop };
    // 真机面板窗口宽 600 + 两只耳朵；菜单栏落点没有耳朵。
    const shell = (node: React.ReactNode) => (
      <div className="island-shell" style={{ width: menubar ? 600 : 628, "--island-pill-w": "380px", "--island-pill-h": "46px" } as React.CSSProperties}>{node}</div>
    );
    if (scene === "island-panel" || scene === "island-panel-menubar") {
      el = shell(<IslandPanel sessions={sessions} usage={usage} quotaProviders={["claude", "codex"]} {...panelProps} />);
    } else if (scene === "island-panel-single") {
      // 只有一页、没有额度：不出翻页箭头与圆点。
      el = shell(<IslandPanel sessions={sessions.slice(0, 2)} usage={{}} quotaProviders={[]} {...panelProps} />);
    } else if (scene === "island-panel-empty") {
      el = shell(<IslandPanel sessions={[]} usage={{}} quotaProviders={[]} {...panelProps} strip />);
    } else if (scene === "island-panel-last") {
      // 末页不满三条：四个会话分两页，停在只有一条的第二页（后面还有额度页）。翻页前后面板高度要一样。
      el = shell(<IslandPanel sessions={sessions} usage={usage} quotaProviders={["claude", "codex"]} initialPage={1} {...panelProps} />);
    } else if (scene === "island-panel-empty-usage") {
      // 没有会话但有额度：空态那一页也能翻到额度页，两页高度要一样。
      el = shell(<IslandPanel sessions={[]} usage={usage} quotaProviders={["claude", "codex"]} {...panelProps} />);
    } else if (scene === "island-usage") {
      el = shell(<IslandPanel sessions={sessions} usage={usage} quotaProviders={["claude", "codex"]} initialPage={9} {...panelProps} />);
    } else if (scene === "island-strip") {
      // 极简细条：四种状态各一条，外加没有会话时的一条。真机上它贴着屏幕顶边。
      stage.paddingTop = 0;
      stage.gap = 24;
      el = [0, 1, 2, 3, null].map((k) => (
        <div key={String(k)} style={{ width: 160, height: 8 }}>
          <IslandStrip sessions={k == null ? [] : [sessions[k]]} onPointerDownDrag={noop} onActivate={noop} />
        </div>
      ));
    } else {
      const alerts: Record<string, IslandAlertEvent> = {
        "island-alert": { sessionId: 1, kind: "approval", title: titles[0], project: "web-app", detail: null },
        "island-alert-menubar": { sessionId: 1, kind: "approval", title: titles[0], project: "web-app", detail: null },
        "island-alert-error": { sessionId: 2, kind: "error", title: titles[3], project: "infrastructure-monorepo", detail: "API Error: 529 overloaded_error · the model is overloaded, please retry later" },
        "island-alert-done": { sessionId: 4, kind: "waiting", title: titles[2], project: "api", detail: lang === "en" ? "All 12 tests pass. Pushed to the remote branch." : "12 个测试全部通过，已经推到远端分支。" },
        // 回合结束、排着只含终端项的切换（Kimi「计划：开」）：「等你回复」那条的说明行换成打开后切换（第七轮 R3，
        // watch.rs 调 session_settings::terminal_reminder，只发这一条，不另弹）。
        "island-alert-terminal": { sessionId: 4, kind: "waiting", title: titles[2], project: "api", detail: lang === "en" ? "Open this session to switch to \"Plan: on\"" : "打开这个会话后切换到「计划：开」" },
      };
      const alert = alerts[scene] ?? null;
      const compactStyle = scene.includes("focus") ? "focus" : scene.includes("minimal") ? "minimal" : "counts";
      const clone = (base: number) => sessions.map((s, i) => ({ ...s, session: { ...s.session, id: base + i } }));
      // -idle 没有会话；-calm 只有在跑和跑完的（眼睛是平时的青柠）；-many 十二个（折成 +N）。
      const shown = scene.endsWith("-idle") ? [] : scene.endsWith("-calm") ? sessions.slice(2) : scene.endsWith("-many") ? [...sessions, ...clone(10), ...clone(20)] : sessions;
      const usages = scene.endsWith("-idle") ? [] : [{ provider: "claude", pct: scene.endsWith("-calm") ? 38 : 72 }];
      const notch = scene.endsWith("-notch") ? { width: 200, height: 34 } : undefined;
      el = (
        <div style={alert ? { width: menubar ? 520 : 548, height: 68 } : { height: notch ? 34 : 46 }}>
          <Island sessions={shown} compactStyle={compactStyle} alert={alert} alertQueued={alert && scene !== "island-alert-done" && scene !== "island-alert-terminal" ? 2 : 0} usages={usages} onMeasure={noop} onActivate={noop} onOpenSession={noop} onPointerDownDrag={noop} onDismissAlert={noop} onDismissAllAlerts={noop} notch={notch} />
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
    // ?proxy=custom：通用页的网络代理拍「自定义」，地址输入行只在这一档出现；?proxy=agents 再给 codex 单独设一条，
    // 「各 Agent 单独设置」首次打开即展开。旧场景名 settingswin-network 由分区映射落到通用页。
    const proxyParam = q.get("proxy") ?? (section === "network" ? "custom" : null);
    if (proxyParam === "custom") store.settings.proxy = { mode: "custom", url: "http://127.0.0.1:7890", per_agent: {} };
    if (proxyParam === "agents") store.settings.proxy = { mode: "custom", url: "http://127.0.0.1:7890", per_agent: { codex: { mode: "off", url: "" } } };
    // ?notify=off：通知页的桌面通知总开关关着（系统通知列置灰、灵动岛列照常可改）。
    if (q.get("notify") === "off") store.settings.notifications_enabled = false;
    // ?relay=on|ready|partial：Agent 页 Claude 卡的接入方式——中转正在用 / 配齐了但仍用官方 / 还差模型。
    const relayParam = q.get("relay");
    if (relayParam) {
      store.settings.relay = { per_agent: { claude: {
        enabled: relayParam === "on",
        base_url: "https://relay.example.com/v1",
        model: relayParam === "partial" ? "" : "claude-sonnet-5",
        protocol: "",
        auth: "bearer",
        env_options: [],
      } } };
    }
    const relayAgents = relayParam
      ? AGENTS_WITH_OPTIONS.map((a) => a.id === "claude" ? { ...a, relay: { protocols: [], auth_modes: [{ value: "bearer", label: "Bearer Token" }, { value: "api_key", label: "API Key (x-api-key)" }], default_protocol: "", default_auth: "bearer", suggestions: [{ protocol: "", models: ["claude-sonnet-5"] }], env_options: [] } } : a)
      : AGENTS_WITH_OPTIONS;
    // ?mcp=unreadable：Antigravity 装着，但它的 mcp_config.json 带注释、Meowo 读不懂，发图工具没接上（mcp_config_unreadable）。
    const agentList = q.get("mcp") === "unreadable"
      ? relayAgents.map((a) => a.id === "antigravity" ? { ...a, installed: true, mcp_config_unreadable: "C:\\Users\\demo\\.gemini\\config\\mcp_config.json" } : a)
      : relayAgents;
    // ?upd=checking|error|available|downloading|ready：关于页的更新状态（设置窗开着 10 秒后自动查一次）。
    // ?fail=1：保存设置失败（各分区的错误提示）；远程分区另报一条端口之外的启动错误。
    const upd = q.get("upd");
    const fail = !!q.get("fail");
    setExtraMock((cmd, args) => {
      if (upd && cmd === "check_update") {
        if (upd === "checking") return new Promise(() => {});
        if (upd === "error") throw new Error("network");
        return { version: "0.7.0", body: "", downloadState: upd };
      }
      if (upd && cmd === "download_update") return upd === "available" ? "available" : upd;
      if (upd === "downloading" && cmd === "plugin:event|listen") {
        const a = args as { event?: string; handler?: number };
        if (a.event === "update-download-progress" && a.handler != null) {
          const internals = (window as unknown as { __TAURI_INTERNALS__?: { runCallback?: (id: number, data: unknown) => void } }).__TAURI_INTERNALS__;
          window.setTimeout(() => internals?.runCallback?.(a.handler!, { event: a.event, id: 1, payload: { downloaded: 42, contentLength: 100 } }), 10_400);
        }
        return 1;
      }
      if (fail && cmd === "set_settings") throw new Error("settings/write_failed: C:\Users\demo\.meowo\settings.json is read-only");
      if (cmd === "installed_agent_versions" || cmd === "check_agent_updates") return [];
      if (cmd === "list_agents") return agentList;
      if (relayParam && cmd === "get_relay_secret_status") return { claude: true };
      if (relayParam && cmd === "get_relay_secrets") return { claude: "sk-demo-relay-token" };
      if (cmd === "list_agent_extensions") return EXTENSIONS;
      if (cmd === "available_terminals") return ["wt", "powershell", "cmd"];
      if (cmd === "lead_workflow_installed") return true;
      // 通用页 Lead 组「其他 CLI」：一家装好可自动继续、一家没装，拍出表头 + 子行的样子。
      if (cmd === "lead_provider_installations") {
        const row = (provider: string, name: string, installed: boolean) => ({ provider, name, profileId: null, profileName: null, version: "1.0.0", configured: true, supported: true, managed: installed, installed, assetsInstalled: installed, hooksInstalled: installed, automaticContinue: installed, notes: [], reason: null });
        return [row("codex", "Codex", true), row("kimi", "Kimi Code", false)];
      }
      if (cmd === "check_provider_hooks") return "installed";
      if (cmd === "plugin:app|version") return "0.6.20";
      // ?os=windows：账号行带运行时徽章（照 Orca 的「Windows」）；缺省不给，其余场景的基线不受影响。
      if (cmd === "host_os" && q.get("os")) return q.get("os");
      // 账号区照 Orca 开发实例里的假账号摆：系统默认 + 三个托管账号（起过名的当前账号、带留存项的、没登录的）。
      // ?acct=empty：一个都没加的空状态。
      if (cmd === "list_profiles") {
        const account = (email: string, plan: string | null, organization: string | null = null) => ({ email, display_name: null, organization, plan, login_label: null });
        const system = { id: null, name: "", active: false, account: account("me@example.com", "Pro"), set_aside: [] };
        if (q.get("acct") === "empty") return [{ ...system, active: true }];
        const at = (rel: string) => "C:\Users\demo\.meowo\profiles\claude\side\\" + rel;
        return [
          system,
          { id: "work", name: "工作", active: true, account: account("larry.work@example.com", "Max", "Acme Inc"), set_aside: [] },
          { id: "side", name: "", active: false, account: account("side.project@example.com", "Pro"), set_aside: [
            { path: at("agents.meowo-profile-merge"), name: "agents.meowo-profile-merge", files: 4 },
            { path: at("projects.meowo-profile-merge"), name: "projects.meowo-profile-merge", files: 3 },
            { path: at("skills.meowo-profile-merge"), name: "skills.meowo-profile-merge", files: 2 },
          ] },
          { id: "old", name: "old.trial@example.com", active: false, account: null, set_aside: [] },
        ];
      }
      // 代码托管 › Git：?git=missing（找不到 git）；缺省找到了、身份已设。
      if (cmd === "git_tool_status") {
        if (q.get("git") === "missing") return { found: false, path: null, version: null, userName: null, userEmail: null, error: lang === "zh" ? "系统找不到指定的文件。 (os error 2)" : "program not found" };
        return { found: true, path: "C:\\Program Files\\Git\\cmd\\git.exe", version: "2.47.1.windows.1", userName: "Larry", userEmail: "larry@example.com", error: null };
      }
      // 代码托管 › 平台账号：?fa=list（github.com 与其他平台的账号，一个令牌失效）/ device（github.com 设备码进行中）；
      // 缺省没有账号（添加时默认 GitHub · github.com，先给浏览器登录）。
      if (cmd === "forge_accounts") {
        const fa = q.get("fa");
        const accounts = fa === "list" ? [
          { platform: "github", host: "github.com", login: "octocat", name: "The Octocat", needsRelogin: false, oauth: true },
          { platform: "gitlab", host: "gitlab.com", login: "alice", name: "Alice Chen", needsRelogin: false, oauth: false },
          { platform: "gitea", host: "codeberg.org", login: "carol", name: null, needsRelogin: false, oauth: false },
          { platform: "gitee", host: "gitee.com", login: "erin", name: "Erin", needsRelogin: false, oauth: false },
          { platform: "github", host: "github.corp.example", login: "bob", name: null, needsRelogin: true, oauth: false },
        ] : [];
        const pending = fa === "device" ? { platform: "github", host: "github.com", userCode: "WDJB-MJHT", verificationUri: "https://github.com/login/device", verificationUriComplete: null, expiresAt: Date.now() + 15 * 60_000 } : null;
        return { accounts, credentialSupported: true, pending };
      }
      if (cmd === "remote_access_info") {
        return {
          enabled: true, port: 18620, token: "visual-regression-token", lastError: fail ? "remote/bind_failed: address not available" : null, boundPort: 18620,
          ips: [{ ip: "100.64.0.8", kind: "tailscale" }, { ip: "192.168.1.20", kind: "lan" }],
        };
      }
      return undefined;
    });
    installMocks("about");
    if (q.get("os")) await detectHostOs();
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
    // 独立窗会按表单高度申请加高（NewSessionPanel 的 fitWindowHeight）。浏览器里没有真窗口：
    // 把它申请的尺寸记到 __visualWinSize，shoot.mjs 截图前照着改视口，拍出来的就是加高后的窗口。
    // 无头浏览器的 screen 只有视口那么大，按真实桌面给一个 1080 高的工作区，否则「屏幕里还剩多少地方」恒为 0。
    if (variant.startsWith("window")) Object.defineProperty(window.screen, "availHeight", { value: 1080, configurable: true });
    setExtraMock((cmd, args) => {
      if (cmd === "plugin:window|set_size") {
        const v = JSON.parse(JSON.stringify((args as { value?: unknown })?.value ?? null)) as { Logical?: { width: number; height: number }; width?: number; height?: number } | null;
        const size = v?.Logical ?? v;
        if (size?.width && size.height) (window as unknown as { __visualWinSize?: { w: number; h: number } }).__visualWinSize = { w: Math.round(size.width), h: Math.round(size.height) };
        return null;
      }
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
