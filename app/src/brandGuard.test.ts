import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// 2026-11-11 整体重设计定下的品牌规矩，用源码扫描钉住：
// 青柠永远压在墨色上（浅色主题强调色是墨），选中不用彩色竖条，旧品牌蓝不许回来。
const files = ["styles.css", "mobile/mobile.css"].map((f) => ({ f, lines: readFileSync(resolve(__dirname, f), "utf8").split(/\r?\n/) }));
const hits = (re: RegExp, allow: (line: string) => boolean = () => false) =>
  files.flatMap(({ f, lines }) => lines.map((l, i) => ({ at: `${f}:${i + 1}`, l })).filter(({ l }) => re.test(l) && !allow(l)).map(({ at, l }) => `${at} ${l.trim()}`));

describe("品牌色守护", () => {
  it("样式里不再出现旧品牌蓝（含注释）", () => {
    expect(hits(/#1a88ff|#258eff|#3d9bff|#1783ff|#167ff7|#1266cc|#e8f3ff|rgba\(\s*26,\s*136,\s*255|rgba\(\s*23,\s*131,\s*255/i)).toEqual([]);
  });

  it("青柠只以 token 定义的形式出现，规则里不写死青柠色值", () => {
    // 浅色主题的强调色是墨：规则里一旦写死青柠，就会把它铺到白底上（leader 否掉过浅色铺青柠）。
    // 要用青柠只能经 --cc-accent / --cc-on-accent / --cc-brand-line，或灵动岛自己的 --isl-*（岛恒为黑底）。
    const isTokenDef = (l: string) => /^\s*--[\w-]+:/.test(l);
    expect(hits(/#d4f64c|#c3e63a|#a9cc1f|212,\s*246,\s*76/i, isTokenDef)).toEqual([]);
  });

  it("选中与强调不用左侧彩色竖条", () => {
    // leader 2026-11-11：「浅色下会话选中的时候为什么有个高亮边」；此前跨会话消息也定过不要左侧强调色条。
    const colored = "(--cc-accent|--cc-brand-line|--cc-on-accent|--isl-lime)";
    expect(hits(new RegExp(`border-(left|inline-start)[^;]*var\(${colored}`))).toEqual([]);
    expect(hits(new RegExp(`inset\s+-?[1-9][\d.]*px\s+0\s+0[^;,]*var\(${colored}`))).toEqual([]);
  });
});
