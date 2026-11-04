// 手机远程 UI 的独立构建(mobile.html + src/mobile/main.tsx)。输出 dist-mobile/,
// 由 tauri.conf.json 的 bundle.resources 打进安装包,运行时 remote.rs 从 resource_dir()/dist-mobile
// 静态托管。base="/" 因为内嵌 server 把它挂在根路径。与主 app 构建(tauri)、demo 构建互不影响。
// 用法:cd app && bun run build:mobile
import { defineConfig, type Plugin, type ResolvedConfig } from "vite";
import react from "@vitejs/plugin-react";
import { extname, join, resolve } from "node:path";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { brotliCompressSync, constants as zlib } from "node:zlib";

// 预压缩：给 assets/ 下的文本产物各生成一份同名 .br，remote.rs 看到 Accept-Encoding 带 br
// 就直接回它（content-encoding: br）。手机页此前裸传，首屏 1.1MB 压完约 0.45MB。
// 只压 assets/（文件名带内容哈希，.br 与源文件天然配对，不会错配）；mobile.html 不带哈希、
// 也只有 1KB，不压——否则手工覆盖 dist-mobile 里的 html 时旧 .br 会被继续送出去。
// woff2/png 本身已压缩，跳过。用 Node 自带 zlib，不引依赖。
const PRECOMPRESS_EXT = new Set([".js", ".css", ".svg", ".json"]);
function precompressBrotli(): Plugin {
  let config: ResolvedConfig;
  return {
    name: "meowo:precompress-brotli",
    apply: "build",
    configResolved(resolved) {
      config = resolved;
    },
    closeBundle() {
      const dir = join(config.build.outDir, "assets");
      let files: string[];
      try {
        files = readdirSync(dir);
      } catch {
        return;
      }
      for (const name of files) {
        if (!PRECOMPRESS_EXT.has(extname(name))) continue;
        const file = join(dir, name);
        const raw = readFileSync(file);
        const br = brotliCompressSync(raw, {
          params: {
            [zlib.BROTLI_PARAM_QUALITY]: 11,
            [zlib.BROTLI_PARAM_SIZE_HINT]: raw.length,
          },
        });
        // 压不动的（极小文件）不留 .br，省得多一次无谓的文件读取。
        if (br.length < raw.length * 0.9) writeFileSync(`${file}.br`, br);
      }
    },
  };
}

// 「添加到主屏幕」：应用清单 + 图标放在根路径（文件名固定，清单里要按名引用）。图标直接取
// src-tauri/icons 的现成文件，不在仓库里另存一份副本。不带 start_url：令牌只存在
// localStorage（URL 里的 #token 读完即清，防泄露），起始页写死成什么都不会带上令牌。
// iOS 主屏应用与 Safari 不共享存储，第一次打开要在配对页粘贴一次令牌。
function homeScreenAssets(): Plugin {
  const icon = (name: string) => readFileSync(resolve(__dirname, "src-tauri/icons", name));
  return {
    name: "meowo:home-screen-assets",
    apply: "build",
    generateBundle() {
      this.emitFile({ type: "asset", fileName: "apple-touch-icon.png", source: icon("128x128@2x.png") });
      this.emitFile({ type: "asset", fileName: "icon-512.png", source: icon("icon.png") });
      this.emitFile({
        type: "asset",
        fileName: "manifest.json",
        source: JSON.stringify({
          name: "Meowo",
          short_name: "Meowo",
          display: "standalone",
          background_color: "#121212",
          theme_color: "#121212",
          icons: [
            { src: "/apple-touch-icon.png", sizes: "256x256", type: "image/png" },
            { src: "/icon-512.png", sizes: "512x512", type: "image/png" },
          ],
        }),
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), precompressBrotli(), homeScreenAssets()],
  base: "/",
  build: {
    outDir: resolve(__dirname, "dist-mobile"),
    emptyOutDir: true,
    rollupOptions: {
      input: resolve(__dirname, "mobile.html"),
      output: {
        // 产物名只留 remote.rs 静态托管认的字符：Logo 源文件叫 `128x128@2x.png`，带 `@` 的
        // 文件名被白名单拒成 404，手机页的 Logo 一直是裂图（用户实拍）。后端已放行 `@`，
        // 这里再换掉是让旧安装版（热换 dist-mobile 时）也能取到。
        assetFileNames: (asset) => `assets/${(asset.names?.[0] ?? asset.name ?? "asset").replace(/\.[^.]+$/, "").replace(/[^A-Za-z0-9._-]/g, "-")}-[hash][extname]`,
      },
    },
  },
});
