// 官网「手机远程」演示入口（embed-mobile.html）：和手机远程页（src/mobile/main.tsx）同一套接线——
// 置位远程门控 + mobile.css + 真实 ChatWindow，只是后端换成对话演示（chatData.ts）的同一段假会话。
// 不手画手机截图：手机页改版后演示自动跟上（用户要求视觉复用真实组件）。
import ReactDOM from "react-dom/client";
import { markRemoteUi } from "../remoteMode";
import { setHostOsUnknown } from "../platform";
import { bootAppearance } from "../appearance";
import { I18nProvider } from "../i18n";
import { installMocks, setExtraMock, store } from "./mock";
import { chatMock, CHAT_SESSION_ID } from "./chatData";
import { ChatWindow } from "../views/ChatWindow";
import type { DemoLang } from "./strings";
import "../fonts";
import "../styles.css";
import "../mobile/mobile.css";
import "./demo.css";

const params = new URLSearchParams(location.search);
const lang: DemoLang = params.get("lang") === "en" ? "en" : "zh";

localStorage.clear();
const FIXED_NOW = 1_780_000_000_000;
Date.now = () => FIXED_NOW;

markRemoteUi();
setHostOsUnknown();
window.history.replaceState({}, "", `${location.pathname}?lang=${lang}&sessionId=${CHAT_SESSION_ID}`);
setExtraMock(chatMock(lang));
installMocks("chat");
store.settings.language = lang;
bootAppearance({ scale: false });

ReactDOM.createRoot(document.getElementById("root")!).render(
  <I18nProvider initial={lang}>
    <div className="demo-chat-frame">
      <ChatWindow />
    </div>
  </I18nProvider>,
);
