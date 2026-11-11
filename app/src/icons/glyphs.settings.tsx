// 图标路径注册表（只在设置窗（About 懒加载块）里用到的图形。）
// 按 chunk 分文件、每个图形单独具名导出：rollup 只打包被 import 的常量，且一个模块整体落在
// 共同上游的 chunk 里——全塞一个文件会把对话窗的图形全拖进入口 chunk。新图形放进用到它的那一档，
// 被多个窗口共用就放 glyphs.tsx。路径沿用原先的手绘形，统一 24 网格、只写形状不写描边。
import type { Glyph } from "./Icon";

export const icBot: Glyph = { body: <><path d="M12 8V4H8" /><rect x="4" y="8" width="16" height="12" rx="2.5" /><path d="M2 14h2" /><path d="M20 14h2" /><path d="M9 13.5v2" /><path d="M15 13.5v2" /></> };
export const icCards: Glyph = { body: <><rect x="3" y="3" width="13" height="13" rx="2.5" /><path d="M8 21h10.5a2.5 2.5 0 0 0 2.5-2.5V8" /></> };
export const icInfo: Glyph = { body: <><circle cx="12" cy="12" r="9" /><line x1="12" y1="11" x2="12" y2="16" /><line x1="12" y1="8" x2="12" y2="8" /></> };
export const icKeyboard: Glyph = { body: <><rect x="2" y="5" width="20" height="14" rx="2.5" /><path d="M6 9h.01M10 9h.01M14 9h.01M18 9h.01M6 13h.01M10 13h.01M14 13h.01M18 13h.01M8 16h8" /></> };
export const icPuzzle: Glyph = { body: <path d="M15.4 4.6a2.4 2.4 0 1 0-4.8 0V6H7a1 1 0 0 0-1 1v3.6h1.4a2.4 2.4 0 1 1 0 4.8H6V19a1 1 0 0 0 1 1h3.6v-1.4a2.4 2.4 0 1 1 4.8 0V20H19a1 1 0 0 0 1-1v-3.6h-1.4a2.4 2.4 0 1 1 0-4.8H20V7a1 1 0 0 0-1-1h-3.6z" /> };
export const icSmartphone: Glyph = { body: <><rect x="6.5" y="2.5" width="11" height="19" rx="2.5" /><line x1="10.5" y1="18" x2="13.5" y2="18" /></> };
