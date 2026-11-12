// 标题栏按实测宽度收进 ⋯（验收 #24，做空审查 p24/p24c：写死门限在开面板、缩放 124%/150% 时出界）。
import { describe, expect, it } from "vitest";
import { fitTitleBar, planTitleBar, sameOverflow, TITLE_COMPACT_ORDER, TITLE_OVERFLOW_ORDER, type TitleBarLayout, type TitleBarPlanLayout, type TitleOverflowKey } from "./titleOverflow";

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

// 标题最后才省略（界面打磨第二轮 chat-1 / chat-2 / panels-14）：纯信息排在功能之前收。
// 数字取自实拍：1280 宽开面板带 PR 时标题栏内容盒 764px；改前这里一样没收，标题 69px、分支名 94px。
describe("planTitleBar", () => {
  const plan = (available: number, over: Partial<TitleBarPlanLayout> = {}): TitleBarPlanLayout => ({
    available, gap: 6, titleMin: 156, titleWant: 300,
    fixed: [72, 28],
    items: items({ open: 56, git: 190, pr: 104 }),
    savings: [
      { step: "prState", owner: "pr", saving: 46 },
      { step: "branch", owner: "git", saving: 100 },
      { step: "project", owner: "title", saving: 80 },
      { step: "endLabel", owner: "fixed", saving: 44 },
    ],
    ...over,
  });
  // 一档不收所需：300 + 72 + 28 + (28+56+190+104+28+28) + 6×8 = 882

  it("放得下就什么都不收", () => {
    const out = planTitleBar(plan(882.5), false);
    expect(out.compact.size).toBe(0);
    expect(out.hidden.size).toBe(0);
  });

  it("1280 宽开面板带 PR（内容盒 764）：收结束会话字样、PR 状态字和分支名，项目名与标题完整，功能一颗不少", () => {
    const out = planTitleBar(plan(764), false);
    expect([...out.compact]).toEqual(["endLabel", "prState", "branch"]);
    expect(out.hidden.size).toBe(0);
  });

  it("信息按顺序收：结束会话只留图标 → PR 状态字 → 分支名 → 项目名", () => {
    expect(TITLE_COMPACT_ORDER).toEqual(["endLabel", "prState", "branch", "project"]);
    expect([...planTitleBar(plan(840), false).compact]).toEqual(["endLabel"]);
    expect([...planTitleBar(plan(700), false).compact]).toEqual(["endLabel", "prState", "branch"]);
    expect([...planTitleBar(plan(640), false).compact]).toEqual(TITLE_COMPACT_ORDER);
  });

  it("信息没收完之前，功能按钮不进 ⋯；收完还不够才按保底宽收功能", () => {
    for (let w = 900; w >= 300; w -= 5) {
      const out = planTitleBar(plan(w), false);
      if (out.hidden.size > 0) expect(out.compact.size).toBe(TITLE_COMPACT_ORDER.length);
    }
    // 信息全收后其余共 28+28+28+56+90+58+28+28 + 6×8 = 392，加保底 156 = 548
    expect(planTitleBar(plan(548.5), false).hidden.size).toBe(0);
    expect([...planTitleBar(plan(540), false).hidden]).toEqual(["pr"]);
  });

  it("150% 缩放的 620 窗（内容盒 511、各项放大 1.5 倍）：结束会话收成图标，改前固定门限 400 进不去", () => {
    const zoomed = plan(511, {
      gap: 9, titleMin: 234, titleWant: 330, fixed: [107, 42],
      items: items({ archive: 42, open: 84, git: 285, terminal: 42, info: 42 }).filter((item) => item.key !== "pr"),
      savings: [{ step: "branch", owner: "git", saving: 150 }, { step: "endLabel", owner: "fixed", saving: 65 }],
    });
    const out = planTitleBar(zoomed, false);
    expect(out.compact.has("endLabel")).toBe(true);
    expect(out.compact.has("branch")).toBe(true);
  });

  it("短标题不为自己多争位置：按保底宽算", () => {
    const out = planTitleBar(plan(764, { titleWant: 90 }), false);
    expect(out.compact.size).toBe(0);
  });

  it("没量到（0 宽）时什么都不收", () => {
    const out = planTitleBar(plan(0), false);
    expect(out.compact.size + out.hidden.size).toBe(0);
  });
});
