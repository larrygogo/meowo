/// 标题栏的溢出规则：放不下的功能按钮收进标题 ⋯ 菜单。
///
/// 为什么单独一份：此前窄窗靠 styles.css 的 @container chat-bar 直接 display:none
/// 收掉「打开方式」「PR」「改动」，菜单里却没有对应入口，拉窄窗口就丢功能（调研 sys8/c8：
/// 620 宽开着面板时「终端」「侧边面板」还被面板盖住）。现在标题栏和 ⋯ 菜单读同一份收起结果，
/// 一项要么在栏上、要么在菜单里，不会两头都没有。纯信息（PR 状态字、分支名、项目名）
/// 仍由 CSS 容器查询收起，它们不是功能，点进改动页都看得到。
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

export function sameOverflow(a: ReadonlySet<TitleOverflowKey>, b: ReadonlySet<TitleOverflowKey>): boolean {
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
