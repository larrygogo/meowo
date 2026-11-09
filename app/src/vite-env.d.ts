/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** "1" 表示 E2E 构建：启用 @wdio/tauri-plugin 前端桥与 board-changed 观测计数。生产构建下为 undefined。 */
  readonly VITE_E2E?: string;
}

// 官网吉祥物（src/demo/mascot.tsx）：vite.mascot.config.ts 在构建时从 styles.css 挑出的眼睛样式。
declare module "virtual:mascot-css" {
  const css: string;
  export default css;
}
