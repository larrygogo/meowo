import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { contrast, deltaE, over, parseColor, type Rgba, type Vision } from "./test/colorMath";

// 2026-11-11 整体重设计：品牌色换成猫眼青柠后，文字、状态色的可读性不能靠肉眼估。
// 这里按 styles.css 里的实际取值复算，改色值时不达标会直接挂。
const css = readFileSync(resolve(__dirname, "styles.css"), "utf8");
const slice = (marker: string) => css.slice(css.indexOf(marker), css.indexOf("\n}", css.indexOf(marker)));
const BLOCKS = { dark: slice(":root {"), light: slice(':root[data-theme="light"] {') } as const;
const KIMI = slice("--kt-dark-1").length ? css.slice(css.indexOf("--kt-dark-1"), css.indexOf("--focus-ring")) : "";

function raw(src: string, name: string): string {
  const m = new RegExp(`(?<![\\w-])${name}:\\s*([^;]+);`).exec(src);
  if (!m) throw new Error(`没找到 ${name}`);
  return m[1].trim();
}
const tok = (theme: keyof typeof BLOCKS, name: string): Rgba => parseColor(raw(BLOCKS[theme], name));

describe.each(["dark", "light"] as const)("%s 主题的可读性", (theme) => {
  const l0 = tok(theme, "--cc-window-bg");
  const sheet = tok(theme, "--cc-panel-bg");
  const raised = tok(theme, "--cc-panel-raised");
  const selected = over(tok(theme, "--fill-selected"), l0);
  const on = (fg: Rgba, bg: Rgba) => contrast(over(fg, bg), bg);

  it("正文对内容面与卡片 ≥7:1，次要文字两档 ≥4.5:1", () => {
    for (const bg of [sheet, raised]) {
      expect(on(tok(theme, "--cc-text"), bg)).toBeGreaterThanOrEqual(7);
      expect(on(tok(theme, "--cc-chat-text"), bg)).toBeGreaterThanOrEqual(7);
      expect(on(tok(theme, "--cc-text-dim"), bg)).toBeGreaterThanOrEqual(4.5);
      expect(on(tok(theme, "--cc-text-faint"), bg)).toBeGreaterThanOrEqual(4.5);
      // 对话窗用的文字档（--kt-*）另成一组，同样要过
      for (const n of [2, 3]) expect(on(parseColor(raw(KIMI, `--kt-${theme}-${n}`)), bg)).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("主按钮文字对按钮底 ≥4.5:1，强调文字对内容面 ≥4.5:1", () => {
    expect(contrast(tok(theme, "--cc-on-accent"), tok(theme, "--cc-accent"))).toBeGreaterThanOrEqual(4.5);
    expect(contrast(tok(theme, "--cc-on-accent"), tok(theme, "--cc-accent-dim"))).toBeGreaterThanOrEqual(4.5);
    for (const bg of [sheet, raised]) expect(contrast(tok(theme, "--cc-accent-text"), bg)).toBeGreaterThanOrEqual(4.5);
  });

  it("状态文字对内容面与卡片 ≥4.5:1", () => {
    for (const name of ["--cc-ok-text", "--cc-warn-text", "--cc-fail-text"]) {
      for (const bg of [sheet, raised]) expect(contrast(tok(theme, name), bg), `${name}`).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("状态圆点对窗口底、内容面、选中行 ≥3:1", () => {
    for (const name of ["--cc-ok-vivid", "--cc-warn-vivid", "--cc-err-vivid"]) {
      for (const bg of [l0, sheet, selected]) expect(contrast(tok(theme, name), bg), `${name}`).toBeGreaterThanOrEqual(3);
    }
  });

  it("运行、等待、出错三色在正常与红绿色盲视觉下的可分度不低于换品牌前", () => {
    // 换品牌前实测的两两最小 ΔE2000（Machado 模拟）：深色 34.6 / 3.1 / 5.9，浅色 34.7 / 13.5 / 14.7。
    const before: Record<string, Record<Vision, number>> = {
      dark: { normal: 34.6, deuteranopia: 3.1, protanopia: 5.9 },
      light: { normal: 34.7, deuteranopia: 13.5, protanopia: 14.7 },
    };
    const [ok, warn, err] = ["--cc-ok-vivid", "--cc-warn-vivid", "--cc-err-vivid"].map((n) => tok(theme, n));
    for (const v of ["normal", "deuteranopia", "protanopia"] as Vision[]) {
      const min = Math.min(deltaE(ok, warn, v), deltaE(ok, err, v), deltaE(warn, err, v));
      expect(min, `${theme} ${v}`).toBeGreaterThanOrEqual(before[theme][v]);
    }
  });
});
