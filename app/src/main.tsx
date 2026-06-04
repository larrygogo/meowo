import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { App } from "./App";
import { About } from "./views/About";
import "./styles.css";

// 同一份前端按窗口 label 分流：about 窗口渲染关于页，其余渲染主贴纸。
const label = (() => {
  try {
    return getCurrentWindow().label;
  } catch {
    return "main";
  }
})();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{label === "about" ? <About /> : <App />}</React.StrictMode>,
);
