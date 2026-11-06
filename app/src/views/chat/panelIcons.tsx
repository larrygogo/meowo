import type { ReactNode } from "react";
import { isMac } from "../../platform";
import type { PanelTabType } from "./panelTabs";

/** 右侧面板各类页签的图标和快捷键，面板（SidePanel）和会话信息栏（SessionInfo）共用。
 *  单独成文件：SidePanel 是懒加载的，信息栏不该为几个图标把它拖进首屏包。 */
/** 快捷键（与 usePanel 的窗口级监听同一套；照 Kimi Code 1.3.1）。 */
export const PANEL_SHORTCUTS: Record<PanelTabType, string> = {
  diff: "Ctrl+Shift+G",
  browser: "Ctrl+T",
  term: "Ctrl+`",
  files: "Ctrl+Shift+E",
};
/** 快捷键提示文字：macOS 上 Ctrl 显示成 ⌘。 */
export const shortcutLabel = (combo: string) => (isMac() ? combo.replace("Ctrl+", "⌘") : combo);

export const PANEL_ICONS: Record<PanelTabType, ReactNode> = {
  diff: (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="6" cy="6" r="2.5" /><circle cx="6" cy="18" r="2.5" /><circle cx="18" cy="9" r="2.5" />
      <path d="M6 8.5v7" /><path d="M18 11.5c0 4-4 4.5-9.2 4.6" />
    </svg>
  ),
  files: (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
    </svg>
  ),
  term: (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M4 17l6-6-6-6" /><path d="M12 19h8" />
    </svg>
  ),
  browser: (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="9" /><path d="M3 12h18" /><path d="M12 3a14 14 0 0 1 0 18a14 14 0 0 1 0-18" />
    </svg>
  ),
};
