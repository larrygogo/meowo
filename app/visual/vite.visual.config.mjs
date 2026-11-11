// 视觉回归截图专用 vite 配置：root 是 app/，harness 页面在 app/visual/。由 shoot.mjs 以 API 方式起停。
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { realpathSync } from "node:fs";

const appDir = fileURLToPath(new URL("..", import.meta.url));
// worktree 里 app/node_modules 常是指向主仓库的 junction：vite 按真实路径判定可否读取，
// 不把真实路径加进白名单，字体文件（@fontsource）会 403、静默回退到系统字体。
const nodeModulesReal = realpathSync(join(appDir, "node_modules"));

export const VISUAL_PORT = 5191;

export default defineConfig({
  root: appDir,
  plugins: [react()],
  clearScreen: false,
  logLevel: "warn",
  // 与 vite.demo.config.ts 相同：mock 挂到演示专用的全局名上，不碰 __TAURI_INTERNALS__。
  define: {
    "window.__TAURI_INTERNALS__": "window.__MEOWO_DEMO_TAURI__",
    "window.__TAURI_EVENT_PLUGIN_INTERNALS__": "window.__MEOWO_DEMO_TAURI_EVENT__",
  },
  // 依赖预构建缓存另放：app/node_modules 在 worktree 里常是指向主仓库的 junction，
  // 默认的 node_modules/.vite 会和主仓库正在跑的 tauri dev 互相覆盖。
  cacheDir: join(tmpdir(), "meowo-visual-vite"),
  // 首屏就把两个 harness 的依赖扫全：否则第一张截图途中发现新依赖会触发整页重载。
  optimizeDeps: { entries: ["visual/desktop.html", "visual/mobile.html"] },
  server: {
    port: VISUAL_PORT,
    strictPort: true,
    host: "127.0.0.1",
    hmr: false,
    fs: { allow: [appDir, nodeModulesReal] },
    watch: { ignored: ["**/src-tauri/**", "**/node_modules/**"] },
  },
});
