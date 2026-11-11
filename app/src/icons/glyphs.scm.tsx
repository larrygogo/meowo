// 图标路径注册表（只在「更改」面板 / diff / 面板浏览器（GitDiffView、SidePanel 懒加载块）里用到的图形。）
// 按 chunk 分文件、每个图形单独具名导出：rollup 只打包被 import 的常量，且一个模块整体落在
// 共同上游的 chunk 里——全塞一个文件会把对话窗的图形全拖进入口 chunk。新图形放进用到它的那一档，
// 被多个窗口共用就放 glyphs.tsx。路径沿用原先的手绘形，统一 24 网格、只写形状不写描边。
import type { Glyph } from "./Icon";

export const icAppWindow: Glyph = { body: <><rect x="2" y="4" width="20" height="16" rx="2" /><path d="M10 4v4" /><path d="M2 8h20" /><path d="M6 4v4" /></> };
export const icArrowLeft: Glyph = { body: <><path d="M19 12H5" /><path d="M12 19l-7-7 7-7" /></> };
export const icArrowRight: Glyph = { body: <><path d="M5 12h14" /><path d="M12 5l7 7-7 7" /></> };
export const icBan: Glyph = { body: <><circle cx="12" cy="12" r="8.5" /><path d="M6 18L18 6" /></> };
export const icBook: Glyph = { body: <><path d="M4 19.5V5a2 2 0 0 1 2-2h13v15H6.5A2.5 2.5 0 0 0 4 20.5z" /><path d="M8 7h7" /></> };
export const icChevronRight: Glyph = { body: <path d="M9 6l6 6-6 6" /> };
export const icExternal: Glyph = { body: <><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M19 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h5" /></> };
export const icTree: Glyph = { body: <><rect x="3" y="3" width="7" height="6" rx="1.5" /><rect x="14" y="10" width="7" height="5" rx="1.5" /><rect x="14" y="17" width="7" height="4" rx="1.5" /><path d="M6.5 9v9.5H14M6.5 12.5H14" /></> };
export const icWrap: Glyph = { body: <><path d="M4 6h16M4 12h13a3 3 0 0 1 0 6h-4" /><path d="M15 16l-2 2 2 2" /><path d="M4 18h5" /></> };
