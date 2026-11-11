/// 标题栏的溢出规则：放不下的功能按钮收进标题 ⋯ 菜单。
///
/// 为什么单独一份表：此前窄窗靠 styles.css 的 @container chat-bar 直接 display:none
/// 收掉「打开方式」「PR」「改动」，菜单里却没有对应入口，拉窄窗口就丢功能（调研 sys8/c8：
/// 620 宽开着面板时「终端」「侧边面板」还被面板盖住）。现在标题栏和 ⋯ 菜单读同一张门限表，
/// 一项要么在栏上、要么在菜单里，不会两头都没有。纯信息（PR 状态字、分支名、项目名）
/// 仍由 CSS 容器查询收起，它们不是功能，点进改动页都看得到。
import { createContext, type ReactNode } from "react";

export type TitleOverflowKey = "pr" | "open" | "git" | "terminal" | "info" | "archive";

/** .chat-bar 内容宽 ≤ 门限时该项收进 ⋯。PR/打开方式沿用原 CSS 档位（640/520，测试页逐档截图定的）。
 *  400 以下（620 宽窗口开着面板时标题栏只剩 320）把终端、会话信息、改动一起收走：
 *  实测 320 宽时这两颗按钮落到 332～386px，被面板盖住（A-20261111-ui-spec #24）。
 *  「标记完成」同档收起，但菜单里本来就有「标记完成 / 恢复进行中」，不另加一项。 */
export const TITLE_OVERFLOW_AT: Record<TitleOverflowKey, number> = { pr: 640, open: 520, git: 400, terminal: 400, info: 400, archive: 400 };

/** 当前宽度下要收进菜单的项。width 为 0 表示还没量到（jsdom、首帧），一律留在栏上。
 *  手机上「改动」胶囊不收：它是 push/PR/合并的唯一入口，收起后只剩图标加改动数，约 40px，
 *  放得下（调研 m1：393 宽及以下原先整项被收掉，进不去「更改」）。 */
export function titleBarOverflow(width: number, remote: boolean): Set<TitleOverflowKey> {
  const out = new Set<TitleOverflowKey>();
  if (width <= 0) return out;
  for (const key of Object.keys(TITLE_OVERFLOW_AT) as TitleOverflowKey[]) {
    if (remote && key === "git") continue;
    if (width <= TITLE_OVERFLOW_AT[key]) out.add(key);
  }
  return out;
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
