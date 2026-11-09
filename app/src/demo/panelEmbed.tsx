// 官网「右侧面板」演示入口（embed-panel.html）：真实 ChatWindow + 对话演示的同一段假会话，
// 右侧面板预先开好「改动」页签，git 接口 mock 成这次重构改到的三个文件。
// 终端、浏览器页签背后是真进程 / 原生 webview，静态页里起不来，只开改动和文件两个页签。
import "./isolatedStorage"; // 必须第一个：每段演示一份独立的内存 localStorage，见该文件说明
import ReactDOM from "react-dom/client";
import { detectHostOs } from "../platform";
import { bootAppearance } from "../appearance";
import { I18nProvider } from "../i18n";
import { installMocks, setExtraMock, store } from "./mock";
import { chatMock, CHAT_SESSION_ID } from "./chatData";
import { panelMock } from "./panelData";
import { savePanel } from "../views/chat/panelTabs";
import { ChatWindow } from "../views/ChatWindow";
import type { DemoLang } from "./strings";
import "../fonts";
import "../styles.css";
import "./demo.css";

const params = new URLSearchParams(location.search);
const lang: DemoLang = params.get("lang") === "en" ? "en" : "zh";

const FIXED_NOW = 1_780_000_000_000;
Date.now = () => FIXED_NOW;

savePanel(CHAT_SESSION_ID, {
  tabs: [{ key: "diff", type: "diff" }, { key: "files", type: "files" }],
  active: "diff",
  visible: true,
  expanded: false,
});

window.history.replaceState({}, "", `${location.pathname}?lang=${lang}&sessionId=${CHAT_SESSION_ID}`);
const chat = chatMock(lang);
const panel = panelMock();
setExtraMock((cmd, args) => panel(cmd, args) ?? chat(cmd));
installMocks("chat");
store.settings.language = lang;
bootAppearance({ scale: true });

(async () => {
  await detectHostOs();
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <I18nProvider initial={lang}>
      <div className="demo-chat-frame">
        <ChatWindow />
      </div>
    </I18nProvider>,
  );
  // 改动页没有「默认打开某个文件」的入口（也不该为演示加），等列表渲染出来替用户点开
  // LoginForm.tsx，让画面直接是一段 diff。
  const started = performance.now();
  const timer = window.setInterval(() => {
    const row = document.querySelector<HTMLElement>('.chat-diff-tree-file[data-tip="src/components/LoginForm.tsx"]');
    if (row) row.click();
    if (row || performance.now() - started > 5000) window.clearInterval(timer);
  }, 100);
})();
