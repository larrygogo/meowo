// 把内嵌演示（embed-demo.html + src/demo/embed.tsx）单独构建成静态站点，
// 输出到 site/public/demo/，供官网 hero 以 <iframe src="/demo/embed-demo.html?lang=…"> 引用。
// 与主 app 构建（tauri）互不影响。用法：cd app && bun x vite build -c vite.demo.config.ts
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

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
  base: "/demo/",
  build: {
    outDir: resolve(__dirname, "../site/public/demo"),
    emptyOutDir: true,
    rollupOptions: {
      input: resolve(__dirname, "embed-demo.html"),
    },
  },
});
