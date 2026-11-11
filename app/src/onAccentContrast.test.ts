import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// leader 关卡 Q-20261111-onaccent：浅色主题主按钮改深色字，强调色填充上的字必须过 AA（4.5:1）。
const css = readFileSync(resolve(__dirname, "styles.css"), "utf8");
const lum = (hex: string) => {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255)
    .map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const ratio = (a: string, b: string) => {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};
const block = (marker: string) => css.slice(css.indexOf(marker), css.indexOf("\n}", css.indexOf(marker)));
const val = (src: string, name: string) => {
  const at = src.indexOf(`${name}:`);
  if (at < 0) return "";
  return src.slice(at + name.length + 1).trimStart().slice(0, 7);
};

describe("强调色填充上的文字对比度", () => {
  it("深浅两套主题下 --cc-on-accent 压 --cc-accent 与 --cc-accent-dim 都 ≥4.5:1", () => {
    for (const src of [block(":root {"), block(':root[data-theme="light"] {')]) {
      const fg = val(src, "--cc-on-accent");
      expect(fg).not.toBe("");
      for (const bg of [val(src, "--cc-accent"), val(src, "--cc-accent-dim")]) {
        expect(ratio(fg, bg), `${fg} on ${bg}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });
});
