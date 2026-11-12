import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { App } from "./App";
import { TooltipLayer } from "./Tooltip";
import { lockdownInProduction, suppressNativeFind } from "./devtools-guard";
import { installInputModality } from "./input-modality";
import { bootAppearance } from "./appearance";
import { detectHostOs } from "./platform";
import { bootWindowMaterial } from "./windowMaterial";
import { bootI18n, I18nProvider } from "./i18n";
import { WindowErrorBoundary } from "./ErrorBoundary";
import "./fonts"; // 内置字体集的唯一入口(取舍理由见 fonts.ts;demo/poster 也走它,不再各自 import)
import "./styles.css";

// 除常驻的贴纸窗(App)外,其余窗口全部懒加载:此前 7 个窗口共享一个 963KB 的静态 chunk,
// 贴纸窗启动要解析它根本用不到的 xterm(~250KB)与 react-markdown(~150KB)——它们只从
// ChatWindow 可达,拆开后各窗口只付自己那份。窗口本就 visible:false、由前端量好再 show
// (useShowWhenReady),懒加载多的一拍不产生可见闪烁。
const About = React.lazy(() => import("./views/About").then((m) => ({ default: m.About })));
const Updater = React.lazy(() => import("./views/Updater").then((m) => ({ default: m.Updater })));
const NewSessionPanel = React.lazy(() =>
  import("./views/NewSessionPanel").then((m) => ({ default: m.NewSessionPanel })),
);
const Onboarding = React.lazy(() =>
  import("./views/Onboarding").then((m) => ({ default: m.Onboarding })),
);
// 对话窗套在名单外壳里（views/roster）：首屏是 agent 名单，进了某一行才是原来的对话窗。
const RosterShell = React.lazy(() =>
  import("./views/roster/RosterShell").then((m) => ({ default: m.RosterShell })),
);
const ConfirmWindow = React.lazy(() =>
  import("./views/ConfirmWindow").then((m) => ({ default: m.ConfirmWindow })),
);

// E2E 构建（VITE_E2E=1）才注入 @wdio/tauri-plugin 前端桥（console 转发 / invoke 拦截 /
// window.wdioTauri）。生产构建下 VITE_E2E 未定义，该动态 import 被 vite 死代码消除，
// 这个 devDependency 不进产物。见 app/e2e/README.md。
if (import.meta.env.VITE_E2E === "1") {
  void import("@wdio/tauri-plugin");
}

// 正式构建下封死右键菜单与 DevTools 快捷键（dev 放行）。
lockdownInProduction();

// 全部窗口按掉 WebView 原生页内查找条（Ctrl+F/F3），应用内搜索自行接管。
suppressNativeFind();

// 焦点框只在键盘导航时显示（避免打开面板时 WKWebView 自动聚焦首元素亮起 UA 焦点框）。
installInputModality();

// 平台标记（同步，供 CSS 做平台差异，如 macOS 无边框设置窗需自行圆角）。WKWebView 的 UA 含 "Mac"。
if (/Mac/i.test(navigator.userAgent)) {
  document.documentElement.classList.add("platform-macos");
} else if (/Windows/i.test(navigator.userAgent)) {
  // WebView2 的 UA 含 "Windows NT"。Linux 两个类都不加。
  // 目前没有任何 CSS 用到这个类：Windows 的版式（标题行高 40、内容面右下各留 8）就是样式表的默认值，
  // macOS 用 .platform-macos 覆盖。这里留着是给以后的 Windows 专属规则挂的钩子，别当死代码删。
  document.documentElement.classList.add("platform-windows");
}

// 同一份前端按窗口 label 分流：about 窗口渲染设置页、updater 渲染更新页，其余渲染主贴纸。
const label = (() => {
  try {
    return getCurrentWindow().label;
  } catch {
    return "main";
  }
})();

// 套用外观设置（明暗/不透明度各窗都套；界面缩放下发贴纸窗与对话窗）。
bootAppearance({ scale: label === "main" || label === "chat", panel: label === "main" });

// 渲染前先探测宿主平台：isMacPanel 等同步判定在首帧与各 effect 中即正确，
// 消除「effect 跑在探测 resolve 前、guard 固化为 false」的竞态。
// detectHostOs 内部兜底（非 Tauri 环境立即落为 other），不会悬挂。
// 同时把当前语言的字典取到手（非中文时是一次动态 import，见 i18n/index.tsx）：首帧就是对的
// 语言。bootI18n 失败退回中文、不会 reject。
// 窗口材质也在渲染前问到：mat-on 决定窗口底是实色还是半透明染色层，晚一拍就是一次闪色。
// bootWindowMaterial 同样自带兜底，不会 reject。
void Promise.all([detectHostOs(), bootI18n(), bootWindowMaterial()]).then(() => {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <I18nProvider>
        {/* 渲染崩溃时整窗换成「出错了 / 重新加载」，不白屏（ErrorBoundary.tsx）。 */}
        <WindowErrorBoundary>
          <React.Suspense fallback={null}>
            {label === "about" ? (
              <About />
            ) : label === "updater" ? (
              <Updater />
            ) : label === "new-session" ? (
              <NewSessionPanel />
            ) : label === "onboarding" ? (
              <Onboarding />
            ) : label === "chat" ? (
              <RosterShell />
            ) : label.startsWith("confirm-") ? (
              <ConfirmWindow />
            ) : (
              <App />
            )}
          </React.Suspense>
        </WindowErrorBoundary>
        <TooltipLayer />
      </I18nProvider>
    </React.StrictMode>,
  );
});
