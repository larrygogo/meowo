import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { App } from "./App";
import { About } from "./views/About";
import { lockdownInProduction } from "./devtools-guard";
import { bootAppearance } from "./appearance";
import { detectHostOs } from "./platform";
import { I18nProvider } from "./i18n";
import "@fontsource-variable/inter"; // 内置 Inter 可变字体做西文（自托管，全平台一致）
// 内置 Noto Sans SC 做中文（思源黑体）：按 Unicode 子集切分、本地按需加载，不联网。
// 只取 400(正文)/600(标题)两档控制体积；500 等会自动回退到最近档。
import "@fontsource/noto-sans-sc/400.css";
import "@fontsource/noto-sans-sc/600.css";
import "./styles.css";

// 正式构建下封死右键菜单与 DevTools 快捷键（dev 放行）。
lockdownInProduction();

// 平台标记（同步，供 CSS 做平台差异，如 macOS 无边框设置窗需自行圆角）。WKWebView 的 UA 含 "Mac"。
if (/Mac/i.test(navigator.userAgent)) {
  document.documentElement.classList.add("platform-macos");
}

// 同一份前端按窗口 label 分流：about 窗口渲染关于页，其余渲染主贴纸。
const label = (() => {
  try {
    return getCurrentWindow().label;
  } catch {
    return "main";
  }
})();

// 套用外观设置（明暗/不透明度两窗都套；界面密度仅贴纸窗口）。
bootAppearance({ scale: label !== "about" });

// 渲染前先探测宿主平台：isMacPanel 等同步判定在首帧与各 effect 中即正确，
// 消除「effect 跑在探测 resolve 前、guard 固化为 false」的竞态。
// detectHostOs 内部兜底（非 Tauri 环境立即落为 other），不会悬挂。
void detectHostOs().then(() => {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <I18nProvider>{label === "about" ? <About /> : <App />}</I18nProvider>
    </React.StrictMode>,
  );
});
