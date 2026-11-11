// 标题栏按实测宽度收进 ⋯（验收 #24，做空审查 p24/p24c：写死门限在开面板、缩放 124%/150% 时出界）。
import { describe, expect, it } from "vitest";
import { fitTitleBar, sameOverflow, TITLE_OVERFLOW_ORDER, type TitleBarLayout, type TitleOverflowKey } from "./titleOverflow";

const items = (w: Partial<Record<TitleOverflowKey, number>> = {}) =>
  (["archive", "open", "git", "pr", "terminal", "info"] as const).map((key) => ({ key, width: w[key] ?? 28 }));
const layout = (available: number, over: Partial<TitleBarLayout> = {}): TitleBarLayout => ({
  available, gap: 14, titleMin: 144, fixed: [72, 28], items: items({ open: 56, git: 101, pr: 60 }), ...over,
});
// 全显示所需：144 + 72 + 28 + (28+56+101+60+28+28) + 14×8 = 657
const fits = (l: TitleBarLayout, hidden: Set<TitleOverflowKey>) => {
  const shown = l.items.filter((item) => !hidden.has(item.key));
  const n = 1 + l.fixed.length + shown.length;
  return l.titleMin + l.fixed.reduce((a, b) => a + b, 0) + shown.reduce((a, b) => a + b.width, 0) + l.gap * (n - 1) <= l.available;
};

describe("fitTitleBar", () => {
  it("没量到（0 宽）时全部留在栏上", () => {
    expect(fitTitleBar(layout(0), false).size).toBe(0);
    expect(fitTitleBar(layout(Number.NaN), false).size).toBe(0);
  });

  it("放得下就一项都不收", () => {
    expect(fitTitleBar(layout(657.5), false).size).toBe(0);
  });

  it("按优先级从 PR 开始收，只收到刚好放下为止", () => {
    expect([...fitTitleBar(layout(650), false)]).toEqual(["pr"]);
    expect([...fitTitleBar(layout(560), false)]).toEqual(["pr", "open"]);
  });

  it("每个宽度下结果都放得下，且越窄收得越多（单调）", () => {
    let prev = new Set<TitleOverflowKey>();
    for (let w = 700; w >= 300; w -= 7) {
      const l = layout(w);
      const hidden = fitTitleBar(l, false);
      if (w >= 144 + 72 + 28 + 14 * 2) expect(fits(l, hidden)).toBe(true);
      for (const key of prev) expect(hidden.has(key)).toBe(true);
      prev = hidden;
    }
  });

  it("同样栏宽，按钮按 150% 缩放变宽时要多收（原门限表不看缩放）", () => {
    const base = layout(600);
    const zoomed = layout(600, { gap: 21, fixed: [108, 42], items: items({ archive: 42, open: 84, git: 150, pr: 90, terminal: 42, info: 42 }) });
    expect(fitTitleBar(zoomed, false).size).toBeGreaterThan(fitTitleBar(base, false).size);
    expect(fits(zoomed, fitTitleBar(zoomed, false))).toBe(true);
  });

  it("英文按钮更宽时多收（原门限表不分语言）", () => {
    const zh = layout(600);
    const en = layout(600, { fixed: [110, 28], items: items({ open: 56, git: 101, pr: 120 }) });
    expect(fitTitleBar(en, false).size).toBeGreaterThanOrEqual(fitTitleBar(zh, false).size);
    expect(fits(en, fitTitleBar(en, false))).toBe(true);
  });

  it("手机上改动胶囊不收；不存在的项不占名额", () => {
    const hidden = fitTitleBar(layout(300), true);
    expect(hidden.has("git")).toBe(false);
    const noPr = fitTitleBar(layout(600, { items: items({ open: 56, git: 101 }).filter((item) => item.key !== "pr") }), false);
    expect(noPr.has("pr")).toBe(false);
  });

  it("收起顺序：改动数最后收，面板开关、结束会话不在表里", () => {
    expect(TITLE_OVERFLOW_ORDER[TITLE_OVERFLOW_ORDER.length - 1]).toBe("git");
    expect([...fitTitleBar(layout(320), false)].sort()).toEqual([...TITLE_OVERFLOW_ORDER].sort());
  });

  it("sameOverflow 判同组，供 setState 原样返回不重渲染", () => {
    expect(sameOverflow(new Set(["pr", "open"]), new Set(["open", "pr"]))).toBe(true);
    expect(sameOverflow(new Set(["pr"]), new Set(["open"]))).toBe(false);
  });
});
