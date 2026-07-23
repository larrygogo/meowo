/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1268,
    strictPort: true,
    // src-tauri 整个排除出 vite 的文件监视(Tauri 官方模板同款):Rust workspace 连同
    // target/ 都住在 app/src-tauri 下,cargo 编译中的 .o 文件被占用,watcher 碰上就
    // EBUSY 崩掉 dev server;前端热更新也本来就不该关心 Rust 产物。
    watch: { ignored: ["**/src-tauri/**"] },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
  },
});
