// 视觉回归 harness（手机远程页）：复刻 src/mobile/main.tsx 的接线（远程门控、mobile.css、
// visualViewport → --app-height、RemoteApp 结构），桥换成 src/demo/mock 的内存假数据，跳过配对闸门。
// 场景：mobile-list（会话列表）/ mobile-chat（对话 + 审批卡）/ mobile-changes（「更改」页）。
import "../src/demo/isolatedStorage"; // 必须第一个
import ReactDOM from "react-dom/client";
import { markRemoteUi } from "../src/remoteMode";
import { setHostOsUnknown } from "../src/platform";
import { bootAppearance } from "../src/appearance";
import { I18nProvider } from "../src/i18n";
import { TooltipLayer } from "../src/Tooltip";
import { installInputModality } from "../src/input-modality";
import { installMocks, setExtraMock, store } from "../src/demo/mock";
import { chatMock } from "../src/demo/chatData";
import { panelMock } from "../src/demo/panelData";
import { savePanel } from "../src/views/chat/panelTabs";
import { CHAT_SESSION_ID, FIXED_NOW, markReady, scmMock } from "./fixtures";
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
const sid = scene === "mobile-list" ? "" : `&sessionId=${CHAT_SESSION_ID}`;
window.history.replaceState({}, "", `${location.pathname}?${q.toString()}${sid}`);
const chat = chatMock(lang);
const panel = panelMock();
setExtraMock((cmd, args) => scmMock(cmd) ?? panel(cmd, args) ?? chat(cmd));
installMocks("chat");
store.settings.language = lang;
store.settings.theme = theme;
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

void (async () => {
  const { ChatWindow } = await import("../src/views/ChatWindow");
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <I18nProvider initial={lang}>
      <ChatWindow />
      <TooltipLayer />
    </I18nProvider>,
  );
  markReady();
})();
