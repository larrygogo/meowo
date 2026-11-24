// 视觉回归截图专用 vite 配置：root 是 app/，harness 页面在 app/visual/。由 shoot.mjs 以 API 方式起停。
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { existsSync, readFileSync, realpathSync } from "node:fs";

const appDir = fileURLToPath(new URL("..", import.meta.url));
// worktree 里 app/node_modules 常是指向主仓库的 junction：vite 按真实路径判定可否读取，
// 不把真实路径加进白名单，字体文件（@fontsource）会 403、静默回退到系统字体。
const nodeModulesReal = realpathSync(join(appDir, "node_modules"));

// 端口与依赖缓存可用 VISUAL_PORT 覆盖：多条车道各自在 worktree 里拍图时，写死 5191 会互相抢端口、
// 共用一个缓存目录还会读到别的 worktree 预构建的依赖（icon 车道实际撞上）。
export const VISUAL_PORT = Number(process.env.VISUAL_PORT) || 5191;

/** HTML 视图卡演示：`/__view/<指纹>.html` 出 visual/views/ 下的样本页，照 chat_views.rs 的做法注入同一份桥脚本、
 *  带同样的沙箱 CSP（真实桌面走 meowo-view 协议，手机走 /view/，这里只为截图）。 */
function viewPages() {
  const head = '<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="light dark">';
  return {
    name: "meowo-view-pages",
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const m = /^\/__view\/([0-9a-f]{32})\.html/.exec(req.url ?? "");
        if (!m) return next();
        const bridge = `<script>${readFileSync(join(appDir, "src-tauri/src/view_bridge.js"), "utf8")}</script>`;
        const file = join(appDir, "visual/views", `${m[1]}.html`);
        let body;
        if (existsSync(file)) {
          const src = readFileSync(file, "utf8");
          const doctype = /^\s*<!doctype[^>]*>/i.exec(src);
          body = doctype ? `${doctype[0]}${head}${bridge}${src.slice(doctype[0].length)}` : `<!doctype html>${head}${bridge}${src}`;
        } else {
          body = `<!doctype html>${head}<script>parent.postMessage({jsonrpc:"2.0",method:"meowo/view-missing",params:{}},"*")</script>`;
          // 真实路由回 404；截图工具把任何 4xx 都当失败，演示这里回 200，失效态照样由页面里的消息触发。
        }
        res.setHeader("content-type", "text/html; charset=utf-8");
        res.setHeader("content-security-policy", "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'; sandbox allow-scripts");
        res.end(body);
      });
    },
  };
}

export default defineConfig({
  root: appDir,
  plugins: [react(), viewPages()],
  clearScreen: false,
  logLevel: "warn",
  // 与 vite.demo.config.ts 相同：mock 挂到演示专用的全局名上，不碰 __TAURI_INTERNALS__。
  define: {
    "window.__TAURI_INTERNALS__": "window.__MEOWO_DEMO_TAURI__",
    "window.__TAURI_EVENT_PLUGIN_INTERNALS__": "window.__MEOWO_DEMO_TAURI_EVENT__",
  },
  // 依赖预构建缓存另放：app/node_modules 在 worktree 里常是指向主仓库的 junction，
  // 默认的 node_modules/.vite 会和主仓库正在跑的 tauri dev 互相覆盖。
  cacheDir: join(tmpdir(), `meowo-visual-vite-${VISUAL_PORT}`),
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
