// 图标路径注册表（入口 chunk（灵动岛、贴纸）用到的，以及被两个以上懒加载窗口共用的图形。）
// 按 chunk 分文件、每个图形单独具名导出：rollup 只打包被 import 的常量，且一个模块整体落在
// 共同上游的 chunk 里——全塞一个文件会把对话窗的图形全拖进入口 chunk。新图形放进用到它的那一档，
// 被多个窗口共用就放 glyphs.tsx。路径沿用原先的手绘形，统一 24 网格、只写形状不写描边。
import type { Glyph } from "./Icon";

export const icAlert: Glyph = { body: <><circle cx="12" cy="12" r="9" /><path d="M12 7.5v5.5M12 16.5h.01" /></> };
export const icChat: Glyph = { body: <><path d="M21 15a4 4 0 0 1-4 4H8l-5 3V7a4 4 0 0 1 4-4h10a4 4 0 0 1 4 4z" /><path d="M8 9h8M8 13h5" /></> };
export const icCheck: Glyph = { body: <path d="M4.5 12.5l5 5 10-11" /> };
export const icChevronDown: Glyph = { body: <path d="M6 9l6 6 6-6" /> };
export const icCircleCheck: Glyph = { body: <><circle cx="12" cy="12" r="9" /><path d="m8.5 12 2.5 2.5 4.5-5" /></> };
export const icClose: Glyph = { body: <path d="M6 6l12 12M18 6L6 18" /> };
export const icContrast: Glyph = { body: <><circle cx="12" cy="12" r="9" /><path d="M12 3a9 9 0 0 0 0 18z" fill="currentColor" stroke="none" /></> };
export const icDownload: Glyph = { body: <><path d="M12 4v11" /><path d="M7 10l5 5 5-5" /><path d="M5 20h14" /></> };
export const icExclamation: Glyph = { body: <path d="M12 7v6M12 16.5v.1" /> };
export const icFolder: Glyph = { body: <path d="M3.5 7v10a2 2 0 0 0 2 2h13a2 2 0 0 0 2-2V9.5a2 2 0 0 0-2-2h-6l-2-2.5h-5a2 2 0 0 0-2 2z" /> };
export const icFolderOpen: Glyph = { body: <path d="M3.5 17V7a2 2 0 0 1 2-2h5l2 2.5h6a2 2 0 0 1 2 2V11M3.5 17l2.2-5.2a1.5 1.5 0 0 1 1.4-.9H20a1 1 0 0 1 .9 1.4L18.6 18a1.5 1.5 0 0 1-1.4 1H5.5a2 2 0 0 1-2-2z" /> };
export const icGlobe: Glyph = { body: <><circle cx="12" cy="12" r="9" /><line x1="3" y1="12" x2="21" y2="12" /><path d="M12 3a15 15 0 0 1 0 18a15 15 0 0 1 0-18z" /></> };
export const icHand: Glyph = { body: <><path d="M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2" /><path d="M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2" /><path d="M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8" /><path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15" /></> };
export const icMonitor: Glyph = { body: <><rect width="20" height="14" x="2" y="3" rx="2" /><line x1="8" y1="21" x2="16" y2="21" /><line x1="12" y1="17" x2="12" y2="21" /></> };
export const icMore: Glyph = { body: <><circle cx="5" cy="12" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle cx="19" cy="12" r="1.6" /></>, fill: true };
export const icNote: Glyph = { body: <><path d="M16 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h11l5-5V5a2 2 0 0 0-2-2z" /><path d="M15 21v-5a1 1 0 0 1 1-1h5" /></> };
export const icPencil: Glyph = { body: <><path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" /></> };
export const icPin: Glyph = { body: <><path d="M12 17v5" /><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" /></> };
export const icPlus: Glyph = { body: <path d="M12 5v14M5 12h14" /> };
export const icQuestion: Glyph = { body: <path d="M9.2 9a2.9 2.9 0 1 1 4.1 2.6c-.8.4-1.3 1-1.3 1.9v.5M12 17.2v.1" /> };
export const icRefresh: Glyph = { body: <><path d="M20 11a8 8 0 1 0-2.3 5.7" /><path d="M20 4v7h-7" /></> };
export const icRefreshCw: Glyph = { body: <><path d="M3 12a9 9 0 0 1 15-6.7L21 8" /><path d="M21 3v5h-5" /><path d="M21 12a9 9 0 0 1-15 6.7L3 16" /><path d="M3 21v-5h5" /></> };
export const icRotateCcw: Glyph = { body: <><path d="M3 12a9 9 0 1 0 3-6.7" /><path d="M3 4v5h5" /></> };
export const icSearch: Glyph = { body: <><circle cx="10.8" cy="10.8" r="6.3" /><path d="m15.5 15.5 4 4" /></> };
export const icSettings: Glyph = { body: <><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" /></> };
export const icShieldAlert: Glyph = { body: <path d="M12 3l8 4v5c0 4.5-3.3 8-8 9-4.7-1-8-4.5-8-9V7zM12 9v4M12 16.2v.1" /> };
export const icStopCircle: Glyph = { body: <><circle cx="12" cy="12" r="8.5" /><rect x="9" y="9" width="6" height="6" rx="1" /></> };
export const icStrip: Glyph = { body: <><rect x="3" y="4" width="18" height="16" rx="3" /><path d="M8 8h8" /></> };
export const icTerminal: Glyph = { body: <path d="M4 17l6-6-6-6M12 19h8" /> };
export const icToTop: Glyph = { body: <><path d="M5 3h14" /><path d="m18 13-6-6-6 6" /><path d="M12 7v14" /></> };
