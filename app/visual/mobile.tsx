// 视觉回归 harness（手机远程页）：复刻 src/mobile/main.tsx 的接线（远程门控、mobile.css、
// visualViewport → --app-height、RemoteApp 结构），桥换成 src/demo/mock 的内存假数据，跳过配对闸门。
// 场景：mobile-list（会话列表）/ mobile-chat（对话 + 审批卡）/ mobile-changes（「更改」页），
// 以及 mobile-settings（底栏切到设置屏）/ mobile-new（新建会话浮层）/ mobile-gate*（配对门各态）/
// mobile-boot（加载占位）/ mobile-keys（终端画面兜底）/ mobile-list-empty、mobile-list-error（列表空态与首载失败）。横屏、iPad、键盘弹出复用 list/chat，只换视口（见 shoot.mjs）。
import "../src/demo/isolatedStorage"; // 必须第一个
import React from "react";
import ReactDOM from "react-dom/client";
import { markRemoteUi } from "../src/remoteMode";
import { setHostOsUnknown } from "../src/platform";
import { bootAppearance } from "../src/appearance";
import { I18nProvider } from "../src/i18n";
import { TooltipLayer } from "../src/Tooltip";
import { installInputModality } from "../src/input-modality";
import { demoChatUi, installMocks, setExtraMock, store } from "../src/demo/mock";
import { patchChatUi } from "./qaFlow";
import { chatMock } from "../src/demo/chatData";
import { panelMock } from "../src/demo/panelData";
import { savePanel } from "../src/views/chat/panelTabs";
import { NEW_SESSION_EVENT } from "../src/remoteMode";
import { CHAT_SESSION_ID, FIXED_NOW, leadTask, markReady, MENTION_CANDIDATES, questionInput, scmMock } from "./fixtures";
import "../src/fonts";
import "../src/styles.css";
import "../src/mobile/mobile.css";

const q = new URLSearchParams(location.search);
const lang: "zh" | "en" = q.get("lang") === "en" ? "en" : "zh";
const theme = q.get("theme") === "light" ? "light" : "dark";
const scene = q.get("scene") ?? "mobile-chat";

Date.now = () => FIXED_NOW;

markRemoteUi();
setHostOsUnknown();
installInputModality();
if (scene === "mobile-changes") {
  savePanel(CHAT_SESSION_ID, { tabs: [{ key: "diff", type: "diff" }], active: "diff", visible: true, expanded: false });
}
// mobile-idle：对话页但没有待批准的请求（拍补全、菜单用）；mobile-lead：再加一张 Lead 关卡卡。
// mobile-question：审批卡换成 AskUserQuestion 问答卡（矮屏下选项可见性，与桌面 flow-question 同一份题面）。
// mobile-tasks：派过后台 Bash（一条在跑、三条跑完）和一个在跑的子代理，拍底栏胶囊与后台任务面板。
const inChat = scene === "mobile-chat" || scene === "mobile-changes" || scene === "mobile-idle" || scene === "mobile-lead" || scene === "mobile-question" || scene === "mobile-tasks";
/// 后台任务的假数据：判据是「回执带子任务结局」（与 TasksPanel 同口径），一条调用配一条回执。
function taskItems(zh: boolean): unknown[] {
  const at = (minutes: number) => new Date(FIXED_NOW - minutes * 60_000).toISOString();
  const bash = (id: string, command: string, description: string, start: number, end: number | null) => [
    { type: "tool_use", id, timestamp: at(start), name: "Bash", summary: command, detail: { kind: "command", command, description } },
    { type: "tool_result", id: `r-${id}`, timestamp: at(end ?? start), tool_use_id: id, text: end === null ? "started" : "done", is_error: false,
      subagent: end === null ? { running: 1, completed: 0, failed: 0, task_id: `task-${id}` } : { running: 0, completed: 1, failed: 0, task_id: `task-${id}` } },
  ];
  return [
    { type: "user_text", id: "mt-u1", timestamp: at(30), text: zh ? "起开发服务，顺手把测试和类型检查跑一遍。" : "Start the dev server and run the tests and type check." },
    ...bash("mt-b1", "bunx tsc --noEmit", zh ? "类型检查" : "Type check", 28, 27),
    ...bash("mt-b2", "bun run test", zh ? "跑全量测试" : "Run all tests", 26, 22),
    ...bash("mt-b3", "bun run lint", zh ? "跑 lint" : "Run lint", 21, 20),
    ...bash("mt-b4", "bun run dev --host 127.0.0.1", zh ? "起开发服务" : "Start the dev server", 19, null),
    { type: "tool_use", id: "mt-a1", timestamp: at(10), name: "Agent", summary: zh ? "复核 schema 的边界情况" : "Review schema edge cases",
      subagent: { description: zh ? "复核 schema 的边界情况" : "Review schema edge cases", agent_type: "Explore", count: 1 } },
    { type: "assistant_text", id: "mt-t1", timestamp: at(9), text: zh ? "开发服务已经起来了，子代理在复核边界情况。" : "The dev server is up; a subagent is reviewing the edge cases." },
  ];
}
const sid = inChat ? `&sessionId=${CHAT_SESSION_ID}` : "";
window.history.replaceState({}, "", `${location.pathname}?${q.toString()}${sid}`);
const chat = chatMock(lang);
const panel = panelMock();
const SCREEN = [
  "Do you want to proceed?",
  "❯ 1. Yes",
  "  2. Yes, and always allow git push commands",
  "  3. No, and tell Claude what to do differently (esc)",
];
setExtraMock((cmd, args) => {
  // 远程桥把 open_new_session_window 转成页内事件（transport.ts）；这里没有桥，照样转一次。
  if (cmd === "open_new_session_window") {
    window.dispatchEvent(new CustomEvent(NEW_SESSION_EVENT, { detail: args ?? {} }));
    return null;
  }
  if (cmd === "managed_terminal_screen" && scene === "mobile-keys") return SCREEN;
  if (cmd === "file_mention_candidates") return MENTION_CANDIDATES;
  // 模型菜单要有切换命令才点得开；权限模式要有可选项才出菜单（与桌面补看场景同一份补丁）。
  if (cmd === "agent_chat_ui" && (scene === "mobile-idle" || scene === "mobile-lead")) return patchChatUi(demoChatUi());
  if (cmd === "lead_task_snapshot" && scene === "mobile-lead") return leadTask(lang);
  if (scene === "mobile-tasks") {
    // 模型按钮带档位（「Fable 5.2 · Medium」）：要 effort 能力 + 历史里有模型与档位。
    if (cmd === "agent_chat_ui") return patchChatUi(demoChatUi());
    if (cmd === "pending_interaction") return { approval: null, question: null };
    if (cmd === "get_chat_history") return { ...(chat(cmd) as object), items: taskItems(lang === "zh"), todos: [], pendingReview: null, status: "running", model: "Fable 5.2", effort: "medium" };
  }
  if (scene === "mobile-question" && cmd === "pending_interaction") {
    const base = (chat(cmd) as { approval: Record<string, unknown> }).approval;
    return { approval: null, question: { ...base, requestId: "visual-question", toolName: "AskUserQuestion", description: null, input: questionInput(lang), answerable: true } };
  }
  if (scene === "mobile-question" && cmd === "get_chat_history") return { ...(chat(cmd) as object), pendingReview: "question" };
  if ((scene === "mobile-idle" || scene === "mobile-lead") && cmd === "pending_interaction") return { approval: null, question: null };
  if ((scene === "mobile-idle" || scene === "mobile-lead") && cmd === "get_chat_history") return { ...(chat(cmd) as object), pendingReview: null, status: "waiting" };
  // 列表的空态与首载失败。
  if (cmd === "get_live_sessions_page" && scene === "mobile-list-empty") return [];
  if (cmd === "get_live_sessions_page" && scene === "mobile-list-error") throw new Error("offline");
  return scmMock(cmd) ?? panel(cmd, args) ?? chat(cmd);
});
installMocks("chat");
store.settings.language = lang;
store.settings.theme = theme;
// ?on=1：设置屏的开关全拍成「开」（消息自动折叠、防休眠、手机推送）。
if (q.get("on")) {
  store.settings.chat_turn_folding = true;
  store.settings.bark_url = "https://api.day.app/visual";
}
bootAppearance({ scale: false });

const viewport = window.visualViewport;
if (viewport) {
  const sync = () => {
    if (viewport.scale > 1.01) return;
    document.documentElement.style.setProperty("--app-height", `${Math.round(viewport.height)}px`);
    window.scrollTo(0, 0);
  };
  viewport.addEventListener("resize", sync);
  sync();
}

/// 等某个元素出现后点它（假时钟下 setInterval 由截图脚本按步推进）。
function when(fn: () => boolean): void {
  const timer = window.setInterval(() => { if (fn()) window.clearInterval(timer); }, 50);
}
function clickWhenReady(find: () => HTMLElement | null | undefined): void {
  when(() => {
    const el = find();
    el?.click();
    return !!el;
  });
}
function typeInto(input: HTMLInputElement, value: string): void {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

/// 配对门：未配对时的几种样子。令牌错走「像地址却没带令牌」的同步分支；连不上把 fetch 换成必败；
/// 失效用 transport 的广播事件名（与 transport.ts 的 AUTH_LOST_EVENT 同值）。
async function renderGate(root: ReactDOM.Root): Promise<void> {
  const { TokenGate } = await import("../src/mobile/TokenGate");
  root.render(
    <I18nProvider initial={lang}>
      <TokenGate><div /></TokenGate>
    </I18nProvider>,
  );
  const input = () => document.querySelector<HTMLInputElement>(".remote-gate-card input");
  const submit = (value: string) => when(() => {
    const el = input();
    if (!el) return false;
    typeInto(el, value);
    window.setTimeout(() => document.querySelector<HTMLButtonElement>(".remote-gate-card button")?.click(), 50);
    return true;
  });
  if (scene === "mobile-gate-invalid") submit("http://192.168.1.8:18620/");
  if (scene === "mobile-gate-unreachable") {
    window.fetch = () => Promise.reject(new TypeError("offline"));
    submit("AbC123-xyz_789");
  }
  if (scene === "mobile-gate-expired") when(() => {
    if (!input()) return false;
    window.dispatchEvent(new CustomEvent("meowo:remote-auth-lost"));
    return true;
  });
  if (scene === "mobile-gate-filled") when(() => {
    const el = input();
    if (el) typeInto(el, "AbC123-xyz_789");
    return !!el;
  });
}

void (async () => {
  const root = ReactDOM.createRoot(document.getElementById("root")!);
  if (scene.startsWith("mobile-gate")) {
    await renderGate(root);
    markReady();
    return;
  }
  const dict = lang === "en" ? (await import("../src/i18n/en")).en : (await import("../src/i18n/zh")).zh;
  if (scene === "mobile-boot") {
    // 与 main.tsx 的 RemoteBoot 同一段标记（那边没导出，只有一行）。
    root.render(<div className="remote-boot" role="status">{dict.remote.gateLoading}</div>);
    markReady();
    return;
  }
  if (scene === "mobile-keys") {
    // 终端画面兜底：真实入口要一张屏幕识别不出来的交互卡，mock 造不出来，直接把组件摆进审批卡的壳里看样式。
    const { RemoteTerminalKeys } = await import("../src/views/chat/RemoteTerminalKeys");
    root.render(
      <I18nProvider initial={lang}>
        <div className="chat-window is-remote-compact">
          <div className="chat-main" style={{ justifyContent: "flex-end" }}>
            <div className="chat-approval is-waiting" style={{ marginBottom: 24 }}>
              <div className="chat-approval-copy"><span>{dict.chat.remoteKeysHint}</span><RemoteTerminalKeys sessionId={CHAT_SESSION_ID} /></div>
            </div>
          </div>
        </div>
      </I18nProvider>,
    );
    markReady();
    return;
  }
  const { RemoteApp } = await import("../src/mobile/RemoteApp");
  root.render(
    <I18nProvider initial={lang}>
      <React.Suspense fallback={null}>
        <RemoteApp />
      </React.Suspense>
      <TooltipLayer />
    </I18nProvider>,
  );
  if (scene === "mobile-settings") clickWhenReady(() => document.querySelectorAll<HTMLElement>(".remote-tabbar-item")[2]);
  if (scene === "mobile-new") clickWhenReady(() => document.querySelectorAll<HTMLElement>(".remote-tabbar-item")[1]);
  markReady();
})();
