/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** "1" 表示 E2E 构建：启用 @wdio/tauri-plugin 前端桥与 board-changed 观测计数。生产构建下为 undefined。 */
  readonly VITE_E2E?: string;
}
