// 图标路径注册表（只在设置窗（About 懒加载块）里用到的图形。）
// 按 chunk 分文件、每个图形单独具名导出：rollup 只打包被 import 的常量，且一个模块整体落在
// 共同上游的 chunk 里——全塞一个文件会把对话窗的图形全拖进入口 chunk。新图形放进用到它的那一档，
// 被多个窗口共用就放 glyphs.tsx。路径沿用原先的手绘形，统一 24 网格、只写形状不写描边。
import type { Glyph } from "./Icon";

export const icBot: Glyph = { body: <><path d="M12 8V4H8" /><rect x="4" y="8" width="16" height="12" rx="2.5" /><path d="M2 14h2" /><path d="M20 14h2" /><path d="M9 13.5v2" /><path d="M15 13.5v2" /></> };
export const icCards: Glyph = { body: <><rect x="3" y="3" width="13" height="13" rx="2.5" /><path d="M8 21h10.5a2.5 2.5 0 0 0 2.5-2.5V8" /></> };
export const icGitPr: Glyph = { body: <><circle cx="18" cy="18" r="2.5" /><circle cx="6" cy="6" r="2.5" /><path d="M13 6h3a2 2 0 0 1 2 2v7.5" /><line x1="6" y1="8.5" x2="6" y2="21" /></> };
export const icInfo: Glyph = { body: <><circle cx="12" cy="12" r="9" /><line x1="12" y1="11" x2="12" y2="16" /><line x1="12" y1="8" x2="12" y2="8" /></> };
export const icKeyboard: Glyph = { body: <><rect x="2" y="5" width="20" height="14" rx="2.5" /><path d="M6 9h.01M10 9h.01M14 9h.01M18 9h.01M6 13h.01M10 13h.01M14 13h.01M18 13h.01M8 16h8" /></> };
export const icPuzzle: Glyph = { body: <path d="M15.4 4.6a2.4 2.4 0 1 0-4.8 0V6H7a1 1 0 0 0-1 1v3.6h1.4a2.4 2.4 0 1 1 0 4.8H6V19a1 1 0 0 0 1 1h3.6v-1.4a2.4 2.4 0 1 1 4.8 0V20H19a1 1 0 0 0 1-1v-3.6h-1.4a2.4 2.4 0 1 1 0-4.8H20V7a1 1 0 0 0-1-1h-3.6z" /> };
export const icSmartphone: Glyph = { body: <><rect x="6.5" y="2.5" width="11" height="19" rx="2.5" /><line x1="10.5" y1="18" x2="13.5" y2="18" /></> };
// 代码托管平台（设置 › 代码托管账号的账号行）：同一套 24 网格线稿，只取各家标志的轮廓意思，不照搬商标。
export const icForgeGithub: Glyph = { body: <><path d="M5.2 9.6 4.6 4.2l4 2.6a8.6 8.6 0 0 1 6.8 0l4-2.6-.6 5.4A7.6 7.6 0 1 1 5.2 9.6z" /><path d="M9.5 14.5h.01M14.5 14.5h.01" /></> };
export const icForgeGitlab: Glyph = { body: <path d="M12 20.5 3.6 14.2 6.2 4.5l2.6 7.3h6.4l2.6-7.3 2.6 9.7z" /> };
export const icForgeGitea: Glyph = { body: <><path d="M4 9h12v4.5a5 5 0 0 1-5 5H9a5 5 0 0 1-5-5z" /><path d="M16 10.5h1.5a2.5 2.5 0 0 1 0 5H15" /><path d="M8 3.5v2.5M12 3.5v2.5" /></> };
export const icForgeGitee: Glyph = { body: <><circle cx="12" cy="12" r="8.5" /><path d="M16 9h-5a2 2 0 0 0-2 2v2.5a2 2 0 0 0 2 2h3a2 2 0 0 0 2-2V12h-4" /></> };
export const icForgeBitbucket: Glyph = { body: <><path d="M3.5 4.5h17l-2.7 15H6.2z" /><path d="M9.6 10.5h4.8l-.8 4.5h-3.2z" /></> };
