import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { App } from "./App";
import { About } from "./views/About";
import { lockdownInProduction } from "./devtools-guard";
import { bootAppearance } from "./appearance";
import "./styles.css";

// 正式构建下封死右键菜单与 DevTools 快捷键（dev 放行）。
lockdownInProduction();

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

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{label === "about" ? <About /> : <App />}</React.StrictMode>,
);
