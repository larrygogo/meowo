import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { terminalTheme } from "./terminalTheme";

const src = (p: string) => readFileSync(resolve(__dirname, p), "utf8");

describe("终端配色走品牌 token（UI 优化 #50）", () => {
  it("xterm 主题从 :root 的 --term-* 读取，读不到时回落到同值的兜底", () => {
    document.documentElement.style.setProperty("--term-cursor", "#d4f64c");
    document.documentElement.style.setProperty("--term-selection", "rgba(212, 246, 76, 0.25)");
    const theme = terminalTheme();
    expect(theme.cursor).toBe("#d4f64c");
    expect(theme.selectionBackground).toBe("rgba(212, 246, 76, 0.25)");
    document.documentElement.style.removeProperty("--term-cursor");
    document.documentElement.style.removeProperty("--term-selection");
    // jsdom 不加载 styles.css：兜底值必须是品牌蓝，不能回到旧翡翠绿。
    expect(terminalTheme().cursor).toBe("#d4f64c");
  });

  it("两个终端组件与终端浮层样式里不再写死旧翡翠绿和绿调灰", () => {
    const files = [src("ManagedTerminal.tsx"), src("chat/ShellTerminal.tsx")];
    for (const f of files) expect(f).not.toMatch(/#55d6ae|#31403a|#e7e9e8/i);
    const css = src("../styles.css");
    expect(css).not.toMatch(/#(55d6ae|31403a|e7e9e8|a7adaa|9ba19e|7d827f|c8cdca|d9a03f)\b/i);
    expect(css).not.toMatch(/rgba\(21, ?22, ?23/);
    expect(css).not.toContain("翡翠");
  });
});
