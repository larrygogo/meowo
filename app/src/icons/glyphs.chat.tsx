// 图标路径注册表（只在对话窗（ChatWindow 及其懒加载子块）里用到的图形。）
// 按 chunk 分文件、每个图形单独具名导出：rollup 只打包被 import 的常量，且一个模块整体落在
// 共同上游的 chunk 里——全塞一个文件会把对话窗的图形全拖进入口 chunk。新图形放进用到它的那一档，
// 被多个窗口共用就放 glyphs.tsx。路径沿用原先的手绘形，统一 24 网格、只写形状不写描边。
import type { Glyph } from "./Icon";

export const icAgents: Glyph = { body: <><circle cx="12" cy="5" r="2.4" /><circle cx="5.5" cy="18.5" r="2.4" /><circle cx="18.5" cy="18.5" r="2.4" /><path d="M12 7.4v3.2M12 10.6H5.5v5.5M12 10.6h6.5v5.5" /></> };
export const icArrowLeftRight: Glyph = { body: <path d="m16 3 4 4-4 4M20 7H5M8 21l-4-4 4-4M4 17h15" /> };
export const icArrowUp: Glyph = { body: <path d="M12 19V5M5 12l7-7 7 7" /> };
export const icArrowUpRight: Glyph = { body: <path d="M7 17 17 7M9 7h8v8" /> };
export const icAt: Glyph = { body: <><circle cx="12" cy="12" r="4" /><path d="M16 8v5a3 3 0 0 0 6 0v-1a10 10 0 1 0-4 8" /></> };
export const icChevronLeft: Glyph = { body: <path d="M15 18l-6-6 6-6" /> };
export const icChevronUp: Glyph = { body: <path d="m18 15-6-6-6 6" /> };
export const icChevronsDownUp: Glyph = { body: <path d="M8 5l4 4 4-4M8 19l4-4 4 4" /> };
export const icChevronsUpDown: Glyph = { body: <path d="M8 9l4-4 4 4M8 15l4 4 4-4" /> };
export const icCircle: Glyph = { body: <circle cx="12" cy="12" r="7.5" /> };
export const icClock: Glyph = { body: <><circle cx="12" cy="12" r="8.5" /><path d="M12 7v5l3 2" /></> };
export const icCode: Glyph = { body: <path d="m8 7-5 5 5 5M16 7l5 5-5 5" /> };
export const icCollapse: Glyph = { body: <path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" /> };
export const icCopy: Glyph = { body: <><rect x="8" y="8" width="11" height="12" rx="2" /><path d="M15 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h2" /></> };
export const icExpand: Glyph = { body: <path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" /> };
export const icFile: Glyph = { body: <><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /></> };
export const icFilter: Glyph = { body: <><path d="M8 4v3.2M8 12.4v7.6M16 4v7.6M16 16.8V20" /><circle cx="8" cy="9.8" r="2.2" /><circle cx="16" cy="14.2" r="2.2" /></> };
export const icFlag: Glyph = { body: <path d="M4 21V4h11l-1.5 4L15 12H4" /> };
export const icFolderPlus: Glyph = { body: <><path d="M3.5 7v10a2 2 0 0 0 2 2h13a2 2 0 0 0 2-2V9.5a2 2 0 0 0-2-2h-6l-2-2.5h-5a2 2 0 0 0-2 2z" /><path d="M12 11v5M9.5 13.5h5" /></> };
export const icFolderTree: Glyph = { body: <path d="M6 4v12a2 2 0 0 0 2 2h4M6 9h6M15 6.5h4v4h-4zM15 15.5h4v4h-4z" /> };
export const icGitBranch: Glyph = { body: <><circle cx="6" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><circle cx="18" cy="7" r="2" /><path d="M6 7v10" /><path d="M18 9c0 5-6 5-11.5 8" /></> };
export const icImage: Glyph = { body: <><rect x="3" y="5" width="18" height="14" rx="2" /><circle cx="9" cy="10" r="1.6" /><path d="m5 17 4.5-4.5L13 16l3-3 3 4" /></> };
export const icLightbulb: Glyph = { body: <path d="M9 18h6M10 22h4M12 2a7 7 0 0 0-4 12.7V17h8v-2.3A7 7 0 0 0 12 2z" /> };
export const icList: Glyph = { body: <><path d="M9 6h11" /><path d="M9 12h11" /><path d="M9 18h11" /><circle cx="4.5" cy="6" r="1" /><circle cx="4.5" cy="12" r="1" /><circle cx="4.5" cy="18" r="1" /></> };
export const icMenu: Glyph = { body: <path d="M4 6h16M4 12h16M4 18h16" /> };
export const icMessage: Glyph = { body: <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /> };
export const icMinus: Glyph = { body: <path d="M5 12h14" /> };
export const icPaperclip: Glyph = { body: <path d="m21.4 11.6-9.2 9.2a6 6 0 0 1-8.5-8.5l9.2-9.2a4 4 0 0 1 5.7 5.7l-9.2 9.2a2 2 0 0 1-2.8-2.8l8.5-8.5" /> };
export const icPlusCircle: Glyph = { body: <><circle cx="12" cy="12" r="8.5" /><path d="M12 8.5v7M8.5 12h7" /></> };
export const icPullRequest: Glyph = { body: <><circle cx="6" cy="6" r="2.2" /><circle cx="6" cy="18" r="2.2" /><circle cx="18" cy="18" r="2.2" /><path d="M6 8.2v7.6M18 15.8V9a3 3 0 0 0-3-3h-4M13 3.5 10.5 6 13 8.5" /></> };
export const icShield: Glyph = { body: <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" /> };
export const icSidebarLeft: Glyph = { body: <><rect x="3.5" y="4.5" width="17" height="15" rx="3" /><path d="M9.5 4.5v15" /></> };
export const icSidebarRight: Glyph = { body: <><rect x="3.5" y="4.5" width="17" height="15" rx="3" /><path d="M14.5 4.5v15" /></> };
export const icSlash: Glyph = { body: <path d="M15 4 9 20" /> };
export const icSliders: Glyph = { body: <><path d="M4 7h9M17 7h3M4 17h3M11 17h9" /><circle cx="15" cy="7" r="2" /><circle cx="9" cy="17" r="2" /></> };
export const icSparkles: Glyph = { body: <><path d="M12 3l1.8 4.7L18.5 9.5l-4.7 1.8L12 16l-1.8-4.7L5.5 9.5l4.7-1.8z" /><path d="M19 15l.8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8z" /></> };
export const icStop: Glyph = { body: <rect x="4" y="4" width="16" height="16" rx="4.5" />, fill: true };
export const icSwap: Glyph = { body: <path d="M7 8h10l-3-3M17 16H7l3 3" /> };
export const icTerminalBox: Glyph = { body: <><rect x="3" y="4.5" width="18" height="15" rx="3" /><path d="m7.5 9.5 3 2.5-3 2.5M13 15h4" /></> };
export const icText: Glyph = { body: <path d="M4 6h16M4 12h10M4 18h7" /> };
export const icUndo: Glyph = { body: <><path d="M9 14 4 9l5-5" /><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11" /></> };
export const icWindowMaximize: Glyph = { body: <rect x="5.5" y="5.5" width="13" height="13" rx="1.5" /> };
export const icWindowRestore: Glyph = { body: <><rect x="5" y="8.5" width="10.5" height="10.5" rx="1.5" /><path d="M8.8 8.5V6.6A1.6 1.6 0 0 1 10.4 5h7A1.6 1.6 0 0 1 19 6.6v7a1.6 1.6 0 0 1-1.6 1.6h-1.9" /></> };
