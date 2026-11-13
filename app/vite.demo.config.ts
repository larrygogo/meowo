// 把官网内嵌的几段演示（embed-*.html + src/demo/*Embed.tsx）单独构建成静态站点，
// 输出到 site/public/demo/，供官网以 <iframe src="/demo/embed-<页>.html?lang=…"> 引用（site/components/DemoFrame.tsx）。
// 与主 app 构建（tauri）互不影响。用法：cd app && bun x vite build -c vite.demo.config.ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

export default defineConfig({
  plugins: [react()],
  base: "/demo/",
  // 演示靠 mockIPC 往 window.__TAURI_INTERNALS__ 上挂假 invoke。但 Meowo 面板浏览器本身是
  // Tauri webview，Tauri 给每个页面（含外部页面）都注入了这个对象，且属性全是只读不可配置
  // （tauri 2.12.5 manager/webview.rs + scripts/core.js 的 defineProperty），mock 装不上、
  // 演示一片空白（2026-11-09 实拍）。初始化脚本先于页面执行，Windows 上连 iframe 也注入
  // （wry lib.rs：subframes 一律注入），页面侧绕不开，只能让演示换个全局名，不碰它。
  // 事件插件的 __TAURI_EVENT_PLUGIN_INTERNALS__ 同理。define 同时作用于源码和 dev 预构建的
  // 依赖（@tauri-apps/* 里全是 window.xxx 这一种写法）。
  define: {
    "window.__TAURI_INTERNALS__": "window.__MEOWO_DEMO_TAURI__",
    "window.__TAURI_EVENT_PLUGIN_INTERNALS__": "window.__MEOWO_DEMO_TAURI_EVENT__",
  },
  build: {
    outDir: resolve(__dirname, "../site/public/demo"),
    emptyOutDir: true,
    rollupOptions: {
      // 灵动岛演示（0.8.0 起官网 hero 用这个）。贴纸演示已随贴纸一起删除（2026-11-12）。
      input: {
        island: resolve(__dirname, "embed-island.html"),
        // 官网「对话窗口」一段：真实 ChatWindow + 假会话（src/demo/chatEmbed.tsx）。
        chat: resolve(__dirname, "embed-chat.html"),
        // 官网「右侧面板」「手机远程」两段：同一段假会话，分别开着改动页签 / 用手机页布局。
        panel: resolve(__dirname, "embed-panel.html"),
        mobile: resolve(__dirname, "embed-mobile.html"),
      },
    },
  },
});
