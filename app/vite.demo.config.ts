// 把内嵌演示（embed-demo.html + src/demo/embed.tsx）单独构建成静态站点，
// 输出到 site/public/demo/，供官网 hero 以 <iframe src="/demo/embed-demo.html?lang=…"> 引用。
// 与主 app 构建（tauri）互不影响。用法：cd app && bun x vite build -c vite.demo.config.ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

export default defineConfig({
  plugins: [react()],
  base: "/demo/",
  build: {
    outDir: resolve(__dirname, "../site/public/demo"),
    emptyOutDir: true,
    rollupOptions: {
      // 贴纸演示（旧，官网已不再引用）与灵动岛演示（0.8.0 起官网 hero 用这个）。
      input: {
        demo: resolve(__dirname, "embed-demo.html"),
        island: resolve(__dirname, "embed-island.html"),
        // 官网「对话窗口」一段：真实 ChatWindow + 假会话（src/demo/chatEmbed.tsx）。
        chat: resolve(__dirname, "embed-chat.html"),
      },
    },
  },
});
