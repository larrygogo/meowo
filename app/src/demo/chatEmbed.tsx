// 官网「对话窗口」演示入口（embed-chat.html，官网用 iframe 引）：渲染应用里真实的 ChatWindow，
// 后端接口全部 mock 成一段假会话（Claude 在重构登录页表单校验，停在一条命令审批上）。
// 原来这一段是手画的仿制品（site 的 ChatWindowMock），字小、样子也旧，用户要求优化。
import "./isolatedStorage"; // 必须第一个：每段演示一份独立的内存 localStorage，见该文件说明
import ReactDOM from "react-dom/client";
import { detectHostOs } from "../platform";
import { bootAppearance } from "../appearance";
import { I18nProvider } from "../i18n";
import { installMocks, setExtraMock, store } from "./mock";
import { chatMock, CHAT_SESSION_ID } from "./chatData";
import { ChatWindow } from "../views/ChatWindow";
import type { DemoLang } from "./strings";
import "../fonts";
import "../styles.css";
import "./demo.css";

const params = new URLSearchParams(location.search);
const lang: DemoLang = params.get("lang") === "en" ? "en" : "zh";

const FIXED_NOW = 1_780_000_000_000;
Date.now = () => FIXED_NOW;

// ChatWindow 从地址栏取当前会话 id。
window.history.replaceState({}, "", `${location.pathname}?lang=${lang}&sessionId=${CHAT_SESSION_ID}`);
setExtraMock(chatMock(lang));
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
})();
