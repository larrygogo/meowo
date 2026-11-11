import type { ReactNode } from "react";
import { isMac } from "../../platform";
import type { PanelTabType } from "./panelTabs";
import { Icon } from "../../icons/Icon";
import { icGitBranch } from "../../icons/glyphs.chat";
import { icFolder, icGlobe, icTerminal } from "../../icons/glyphs";

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
    <Icon glyph={icGitBranch} size={12} />
  ),
  files: (
    <Icon glyph={icFolder} size={12} />
  ),
  term: (
    <Icon glyph={icTerminal} size={12} />
  ),
  browser: (
    <Icon glyph={icGlobe} size={12} />
  ),
};
