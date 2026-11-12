/// 标题栏的溢出规则：放不下的功能按钮收进标题 ⋯ 菜单。
///
/// 为什么单独一份：此前窄窗靠 styles.css 的 @container chat-bar 直接 display:none
/// 收掉「打开方式」「PR」「改动」，菜单里却没有对应入口，拉窄窗口就丢功能（调研 sys8/c8：
/// 620 宽开着面板时「终端」「侧边面板」还被面板盖住）。现在标题栏和 ⋯ 菜单读同一份收起结果，
/// 一项要么在栏上、要么在菜单里，不会两头都没有。纯信息（PR 状态字、分支名、项目名、
/// 「结束会话」的字样）排在功能之前收，同样按实测宽度算（planTitleBar），只是不进菜单。
import { createContext, type ReactNode } from "react";

export type TitleOverflowKey = "pr" | "open" | "git" | "terminal" | "info" | "archive";

/** 放不下时先收谁：排在前面的先进 ⋯。PR、打开方式最先收（PR 在改动页看得到，打开方式菜单里
 *  整组都有）；改动数留到最后（它是改动页的入口，原 CSS 档位也是最后才收）。
 *  「标记完成」收起后不另加菜单项：菜单里本来就有「标记完成 / 恢复进行中」。
 *  「结束会话」和面板开关不在表里，永远留在栏上。 */
export const TITLE_OVERFLOW_ORDER: readonly TitleOverflowKey[] = ["pr", "open", "info", "terminal", "archive", "git"];

export type TitleBarLayout = {
  /** .chat-bar 内容盒宽（扣掉左右 padding）。≤0 表示还没量到（jsdom、首帧），一律留在栏上。 */
  available: number;
  /** .chat-bar 的 column-gap。 */
  gap: number;
  /** 标题区保底宽度（.chat-title-menu 的 min-width，CSS 里按 em 定，跟字号走）。 */
  titleMin: number;
  /** 不参与收起的项（侧栏开关、结束会话、面板开关）的实测宽度。 */
  fixed: readonly number[];
  /** 可收起项按栏上顺序的自然宽度（收起时也照常排版、只是不可见，宽度一直量得到）。 */
  items: readonly { key: TitleOverflowKey; width: number }[];
};

/** 按实测宽度决定哪些项进 ⋯：从 TITLE_OVERFLOW_ORDER 头上开始收，直到标题保底宽 + 其余各项 + 间距
 *  都放进内容盒。
 *  为什么不再用写死的门限（原 {pr:640, open:520, 其余:400}）：门限不乘 --cc-ui、不分中英文，
 *  做空审查实测 800/1100/1200 宽开面板、1440 宽缩放 124%/150% 时按钮出界压在面板下，
 *  标题被挤成一两个字（A-20261111-ui-spec #24，redteam p24/p24c）。
 *  手机上「改动」胶囊不收：它是 push/PR/合并的唯一入口，收起后只剩图标加改动数，约 40px，
 *  放得下（调研 m1：393 宽及以下原先整项被收掉，进不去「更改」）。 */
export function fitTitleBar(layout: TitleBarLayout, remote: boolean): Set<TitleOverflowKey> {
  const out = new Set<TitleOverflowKey>();
  if (!(layout.available > 0)) return out;
  const widths = new Map(layout.items.map((item) => [item.key, item.width]));
  const need = () => {
    let sum = layout.titleMin;
    let count = 1;
    for (const w of layout.fixed) { sum += w; count++; }
    for (const [key, w] of widths) if (!out.has(key)) { sum += w; count++; }
    return sum + layout.gap * (count - 1);
  };
  for (const key of TITLE_OVERFLOW_ORDER) {
    // 留 0.5px 余量：各宽度是带小数的实测值，累加误差不该让最后一颗按钮压线出界。
    if (need() <= layout.available - 0.5) break;
    if (!widths.has(key) || (remote && key === "git")) continue;
    out.add(key);
  }
  return out;
}

/** 纯信息的收纳档：放不下时先收谁，排在前面的先收。它们不是功能（点不点都不丢入口），
 *  所以排在所有功能按钮之前收，也不进 ⋯ 菜单：PR 状态字与领先/落后数、分支名在改动页都看得到，
 *  项目名在侧栏分组上，「结束会话」收成图标后名字还在提示和读屏名里。
 *  为什么从 CSS 容器查询搬到这里：原来是 @container chat-bar 的 900 / 760 / 400 三个固定门限，
 *  不看栏上实际摆了什么、也不乘界面缩放。实拍 1280 宽开面板带 PR 时标题栏 792px，刚好在 760 以上，
 *  一样没收，标题只剩 69px 而分支名占 94px；150% 缩放的 620 窗标题栏 553px 进不了 400 那档，
 *  标题只剩 3 个字而「结束会话」四个字占 71px。 */
export type TitleCompactKey = "prState" | "branch" | "project" | "endLabel";
// 顺序：「结束会话」字样最先收——收成图标后入口和名字都还在，丢的只是四个字；项目名最后收——它是
// 「这是哪个仓库的会话」的上下文，比分支名、PR 状态更常用来认会话（2026-11-12 主会话定，
// 子代理初版把项目名排在结束会话字样之前，英文 1280 开面板时 web-app 收了而 End session 还在）。
export const TITLE_COMPACT_ORDER: readonly TitleCompactKey[] = ["endLabel", "prState", "branch", "project"];

/** 标题文字最多按这么多 em 去争位置：再长的标题省略也读得出是哪个会话，
 *  不该为它在两千像素宽的栏上把分支名收掉。项目名同理，它本来就被 CSS 封顶在标题区的 30%。 */
export const TITLE_WANT_EM = 16;
export const PROJECT_WANT_EM = 8;

export type TitleCompactSaving = {
  step: TitleCompactKey;
  /** 这一档省下的宽度记在谁头上：某个可收起项、不可收起项（结束会话）、还是标题区自己（项目名）。 */
  owner: TitleOverflowKey | "fixed" | "title";
  saving: number;
};

export type TitleBarPlanLayout = TitleBarLayout & {
  /** 标题区完整显示（标题文字不省略，封顶 TITLE_WANT_EM）要的宽度；各宽度一律按「一档都没收」时的自然值给。 */
  titleWant: number;
  savings: readonly TitleCompactSaving[];
};

export type TitleBarPlan = { compact: Set<TitleCompactKey>; hidden: Set<TitleOverflowKey> };

/** 标题栏的完整收纳顺序：标题最后才省略。
 *  第一段：标题显示不全时，按 TITLE_COMPACT_ORDER 逐档收纯信息，收到标题能完整显示为止。
 *  第二段：信息全收完，标题连保底宽都不够，才按 fitTitleBar 把功能按钮收进 ⋯。 */
export function planTitleBar(layout: TitleBarPlanLayout, remote: boolean): TitleBarPlan {
  const compact = new Set<TitleCompactKey>();
  const hidden = new Set<TitleOverflowKey>();
  if (!(layout.available > 0)) return { compact, hidden };
  const others = layout.fixed.reduce((a, b) => a + b, 0) + layout.items.reduce((a, b) => a + b.width, 0)
    + layout.gap * (layout.fixed.length + layout.items.length);
  let title = Math.max(layout.titleMin, layout.titleWant);
  let rest = others;
  for (const step of TITLE_COMPACT_ORDER) {
    if (Math.max(layout.titleMin, title) + rest <= layout.available - 0.5) return { compact, hidden };
    compact.add(step);
    for (const s of layout.savings) {
      if (s.step !== step) continue;
      if (s.owner === "title") title -= s.saving; else rest -= s.saving;
    }
  }
  if (Math.max(layout.titleMin, title) + rest <= layout.available - 0.5) return { compact, hidden };
  // 功能按钮按收完信息之后的宽度算：分支胶囊只剩图标和改动数，结束会话只剩图标。
  const saved = (owner: TitleCompactSaving["owner"]) => layout.savings.reduce((a, s) => (s.owner === owner ? a + s.saving : a), 0);
  const fixedSaved = saved("fixed");
  const fixed = layout.fixed.map((w, i) => (i === 0 ? Math.max(0, w - fixedSaved) : w));
  const items = layout.items.map((item) => ({ key: item.key, width: Math.max(0, item.width - saved(item.key)) }));
  return { compact, hidden: fitTitleBar({ ...layout, fixed, items }, remote) };
}

export function sameOverflow<T>(a: ReadonlySet<T>, b: ReadonlySet<T>): boolean {
  if (a.size !== b.size) return false;
  for (const key of a) if (!b.has(key)) return false;
  return true;
}

export type TitleOverflowItem = {
  key: string;
  group: TitleOverflowKey;
  label: string;
  icon: ReactNode;
  onClick: () => void;
  /** 开关类（终端、改动、会话信息）在菜单里也标出当前开着没有。 */
  checked?: boolean;
};

/** 标题栏把收起的项放进来，标题 ⋯ 菜单（ChatTitleMenu）读出来排在最前。 */
export const TitleOverflowContext = createContext<TitleOverflowItem[]>([]);
