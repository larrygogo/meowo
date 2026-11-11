// 视觉回归 harness（桌面窗口）：按 ?scene= 挂真实组件，后端全部换成 src/demo/mock 的内存假数据。
// 接线照抄 src/demo/chatEmbed.tsx / panelEmbed.tsx 与 src/main.tsx，避免截到与真实窗口不同的外观。
// 场景：chat / chat-long / panel-changes / panel-diff / island-compact / island-panel / island-alert /
//       settings-<分区> / onboarding。主题 ?theme=dark|light，语言 ?lang=zh|en。
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

  if (scene.startsWith("settings-")) {
    try { localStorage.setItem(SETTINGS_SECTION_KEY, scene.slice("settings-".length)); } catch { /* 内存存储不会抛 */ }
    // 账号分区要版本探测、远程分区要配对信息：演示 mock 不管这几条，返回 null 会让分区崩掉。
    store.settings.remote_access_enabled = true;
    setExtraMock((cmd) => {
      if (cmd === "installed_agent_versions" || cmd === "check_agent_updates") return [];
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

  if (scene === "onboarding") {
    installMocks("onboarding");
    bootAppearance({ scale: false });
    const { Onboarding } = await import("../src/views/Onboarding");
    return <Onboarding />;
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
