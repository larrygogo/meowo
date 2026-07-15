// 网页内嵌演示入口（官网 hero 用 iframe 引这个页面）：装 mock → 渲染舞台 →
// 用 Timeline.play() 实时循环播放整段分镜。语言由 URL ?lang=zh|en 决定：
// 既设 app 的 i18n（Sticker 自身标签），也把 demo 字幕/正文切到对应语言。
import ReactDOM from "react-dom/client";
import { detectHostOs } from "../platform";
import { bootAppearance } from "../appearance";
import { I18nProvider } from "../i18n";
import { installMocks, store, notify } from "./mock";
import { buildScript } from "./script";
import { DemoStage } from "./DemoStage";
import type { DemoLang } from "./strings";
import "../styles.css";
import "./demo.css";

const params = new URLSearchParams(location.search);
const lang: DemoLang = params.get("lang") === "en" ? "en" : "zh";

localStorage.clear();
// 冻结时钟：让卡片相对时间戳（fmtAgo）稳定为「刚刚 / just now」，不随播放漂移。
const FIXED_NOW = 1_780_000_000_000;
Date.now = () => FIXED_NOW;

installMocks();
store.settings.language = lang;
bootAppearance({ scale: true });

function resetStage(): void {
  store.stage.mode = "normal";
  store.stage.caption = null;
  store.stage.finale = false;
  store.stage.glow = false;
}

(async () => {
  await detectHostOs();
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <I18nProvider initial={lang}>
      <DemoStage lang={lang} />
    </I18nProvider>
  );

  // 实时循环：每轮重置舞台 + 重建分镜（拿到全新会话对象）再播放。
  const loop = () => {
    resetStage();
    notify();
    const tl = buildScript(lang);
    tl.play(() => window.setTimeout(loop, 600));
  };
  loop();
})();
