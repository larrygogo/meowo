import type { ITheme } from "@xterm/xterm";

// 托管终端与面板终端共用的 xterm 配色。xterm 只认具体色值、不认 CSS 变量，
// 所以挂载时从 :root 的 --term-* 读一次（styles.css 定义，深浅主题同值：终端恒为深底）。
// 改版前这里写死翡翠绿光标 #55d6ae 与绿调灰选区，和全局品牌蓝对不上（UI 调研 sys6）。
// 兜底值与 styles.css 同值：jsdom 测试与样式未加载的瞬间也不回到旧配色。
const FALLBACK = {
  background: "#111210",
  foreground: "rgba(255, 255, 255, 0.9)",
  cursor: "#d4f64c",
  selectionBackground: "rgba(212, 246, 76, 0.25)",
} as const;

export function terminalTheme(): ITheme {
  const css = typeof document === "undefined" ? null : getComputedStyle(document.documentElement);
  const read = (name: string, fallback: string) => css?.getPropertyValue(name).trim() || fallback;
  return {
    background: read("--term-bg", FALLBACK.background),
    foreground: read("--term-fg", FALLBACK.foreground),
    cursor: read("--term-cursor", FALLBACK.cursor),
    selectionBackground: read("--term-selection", FALLBACK.selectionBackground),
  };
}
