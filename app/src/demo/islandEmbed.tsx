// 官网灵动岛演示入口（embed-island.html，官网用 iframe 引）：装 mock → 渲染 IslandStage。
// 语言由 URL ?lang=zh|en 决定。时钟冻结，会话的相对时间不随播放漂移。
import ReactDOM from "react-dom/client";
import { detectHostOs } from "../platform";
import { bootAppearance } from "../appearance";
import { I18nProvider } from "../i18n";
import { installMocks, store } from "./mock";
import { IslandStage } from "./IslandStage";
import type { DemoLang } from "./strings";
import "../fonts";
import "../styles.css";
import "./demo.css";

const params = new URLSearchParams(location.search);
const lang: DemoLang = params.get("lang") === "en" ? "en" : "zh";
const scene = params.get("scene") === "alert" ? "alert" : undefined;

localStorage.clear();
const FIXED_NOW = 1_780_000_000_000;
Date.now = () => FIXED_NOW;

installMocks();
store.settings.language = lang;
bootAppearance({ scale: true });

(async () => {
  await detectHostOs();
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <I18nProvider initial={lang}>
      <IslandStage lang={lang} scene={scene} />
    </I18nProvider>,
  );
})();
