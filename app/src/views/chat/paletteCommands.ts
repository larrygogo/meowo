/**
 * Ctrl/Cmd+K 命令面板的命令清单（U1-14 起步，UI 优化 #53 补齐）。从 ChatWindow 拆出来成纯函数：
 * 哪条出现、右侧标什么键只看传入的能力，单测不必挂整个对话窗。
 *
 * 为什么补这些：窄窗时标题栏的「打开方式」和 PR 按钮被 container query 收起（system 调研第 8 条），
 * 面板的 6 个快捷键只能靠 `?` 速查表发现。对照 Raycast 动作面板与 Linear 的 ⌘K：每个动作都能搜到，
 * 右侧标出快捷键。仍是扁平数组、不做最近使用排序（专项已定论）。
 */
import type { useT } from "../../i18n";
import { displayKeys, type PaletteCommand } from "./WindowModals";

type T = ReturnType<typeof useT>;

/** 「用…打开」的候选（编辑器 + 系统文件管理器），与标题栏 OpenInEditor 同一份清单。 */
export interface PaletteOpener { id: string; name: string }

export interface PaletteContext {
  /** 远程手机页：面板、终端视图、宿主执行类都不给（同面板快捷键监听的 remoteUi 早退）。 */
  remote: boolean;
  /** 会话有工作目录：面板（文件 / 浏览器 / 终端）才开得出来。 */
  hasCwd: boolean;
  /** 工作目录是 git 仓库：「改动」页签只在仓库里有。 */
  isRepo: boolean;
  /** 面板开着：Ctrl+Tab 才有页签可切。 */
  panelVisible: boolean;
  /** 底栏模型按钮可用。 */
  canSwitchModel: boolean;
  /** 底栏有权限模式按钮。 */
  canSwitchPermission: boolean;
  /** 当前分支的 PR（标题栏同一份）。 */
  pullRequest: { number: number; url: string } | null;
  openers: readonly PaletteOpener[];
  mac?: boolean;
}

/** 「用…打开」命令 id 的前缀，后半截是 opener id。 */
export const OPEN_WITH_PREFIX = "open-with:";

export function buildPaletteCommands(t: T, ctx: PaletteContext): PaletteCommand[] {
  const panel = !ctx.remote && ctx.hasCwd;
  const list: Array<PaletteCommand | false> = [
    { id: "new-session", label: t.chat.shortcutNewSession, keys: "Ctrl+N" },
    { id: "view-chat", label: t.chat.cmdViewChat, keys: "Ctrl+1" },
    { id: "view-terminal", label: t.chat.cmdViewTerminal, keys: "Ctrl+2" },
    { id: "toggle-sidebar", label: t.chat.shortcutSidebar, keys: "Ctrl+B" },
    // 搜会话没有单独的快捷键了：命令面板（Ctrl+K）本身就能搜会话（二期 #8）。
    { id: "focus-search", label: t.chat.shortcutSearch },
    { id: "search-transcript", label: t.chat.searchTranscript, keys: "Ctrl+F" },
    { id: "jump-latest", label: t.chat.jumpLatest },
    // 面板 6 条：键位与 ChatWindow 的面板快捷键监听一一对应（Ctrl+Shift+B/G/E、Ctrl+T、Ctrl+`、Ctrl+Tab）。
    panel && { id: "panel-toggle", label: t.chat.shortcutPanel, keys: "Ctrl+Shift+B" },
    panel && ctx.isRepo && { id: "panel-changes", label: t.chat.cmdOpenChanges, keys: "Ctrl+Shift+G" },
    panel && { id: "panel-files", label: t.chat.cmdToggleFiles, keys: "Ctrl+Shift+E" },
    panel && { id: "panel-browser", label: t.chat.shortcutNewBrowser, keys: "Ctrl+T" },
    panel && { id: "panel-terminal", label: t.chat.shortcutShell, keys: "Ctrl+`" },
    panel && ctx.panelVisible && { id: "panel-next-tab", label: t.chat.cmdNextPanelTab, keys: "Ctrl+Tab" },
    // 模型与权限模式没有窗口级快捷键（终端里的 Shift+Tab 是 CLI 自己的键），不标键位。
    ctx.canSwitchModel && { id: "switch-model", label: t.chat.switchModel },
    ctx.canSwitchPermission && { id: "switch-permission", label: t.chat.cmdSwitchPermission },
    ctx.pullRequest ? { id: "open-pr", label: t.chat.cmdOpenPr(ctx.pullRequest.number) } : false,
    ...(!ctx.remote && ctx.hasCwd
      ? ctx.openers.map((opener) => ({ id: `${OPEN_WITH_PREFIX}${opener.id}`, label: t.chat.openWith(opener.name) }))
      : []),
    { id: "show-shortcuts", label: t.chat.cmdShowShortcuts, keys: "?" },
    { id: "open-settings", label: t.sticker.openSettings },
  ];
  return list
    .filter((cmd): cmd is PaletteCommand => !!cmd)
    .map((cmd) => (cmd.keys ? { ...cmd, keys: displayKeys(cmd.keys, ctx.mac) } : cmd));
}
