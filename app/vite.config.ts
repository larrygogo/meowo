/// <reference types="vitest/config" />
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";

// 剔除 @fontsource CSS 里的 .woff 兜底源（只留 woff2）：Tauri 的 WebView2/WKWebView 均支持
// woff2，woff 永远不会被加载却会被打包，白占体积（中文字库双格式约差一半）。
function stripWoffFallback(): Plugin {
  return {
    name: "strip-woff-fallback",
    enforce: "pre",
    transform(code, id) {
      if (id.includes("@fontsource") && id.endsWith(".css")) {
        return code.replace(/,\s*url\([^)]+\.woff\)\s*format\(["']woff["']\)/g, "");
      }
      return null;
    },
  };
}

export default defineConfig({
  plugins: [stripWoffFallback(), react()],
  clearScreen: false,
  server: { port: 1268, strictPort: true },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
  },
});
