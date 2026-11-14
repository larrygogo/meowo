// 弹层关闭后把焦点还回去（walkthrough P-2 / P-11，2026-11-12）。
//
// 两个坑，弹窗和菜单都踩过：
// 1. 记「打开前的焦点」不能在 useEffect 里读 document.activeElement：子元素的 autoFocus 在提交阶段就生效了，
//    effect 跑的时候读到的是弹层自己的按钮（放弃确认框关掉后焦点落到 body，wk3e-modal-restore 实测）。
//    要在渲染阶段读，见 useFocusOrigin。
// 2. 面板行上的操作按钮平时 display:none，靠行的 :hover / :focus-within 才显示（styles.css .scm-row-actions、
//    .chat-diff-tree-more）。弹层开着时焦点不在行里，按钮是隐藏的，直接 focus() 无效。先把焦点给同一行里
//    看得见的控件，行进入 focus-within、按钮显示出来，再还给它（wk3b-rowmore：「更多操作」菜单 Esc 后落到 body）。
import { useState } from "react";

export const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** 渲染阶段记下此刻的焦点（只在挂载那一次）：早于子树 autoFocus 生效。 */
export function useFocusOrigin(): HTMLElement | null {
  const [origin] = useState<HTMLElement | null>(() =>
    typeof document !== "undefined" && document.activeElement instanceof HTMLElement && document.activeElement !== document.body
      ? document.activeElement
      : null,
  );
  return origin;
}

const tryFocus = (el: HTMLElement) => {
  el.focus({ preventScroll: true });
  return document.activeElement === el;
};

/** 把焦点还给 target。target 已不在文档里返回 false（由调用方另找落点）。往上最多找三层祖先，
 *  用其中第一个能拿到焦点的控件把行「点亮」，再还给 target；target 仍拿不到焦点就留在那个控件上
 *  （同一行，总比 body 强）。 */
export function restoreFocus(target: HTMLElement | null): boolean {
  if (!target || !target.isConnected) return false;
  if (tryFocus(target)) return true;
  let el = target.parentElement;
  for (let depth = 0; el && el !== document.body && depth < 3; depth += 1, el = el.parentElement) {
    const stand = Array.from(el.querySelectorAll<HTMLElement>(FOCUSABLE)).find((item) => item !== target && tryFocus(item));
    if (stand) {
      tryFocus(target);
      return true;
    }
  }
  return false;
}

/** 把焦点交给 el 之前（文档顺序）最近一个拿得到焦点的控件，相当于替用户按一次 Shift+Tab。
 *  用于提示条之类即将消失的元素：焦点在它里面时先挪出去，不然元素一卸载焦点就掉到 body
 *  （做空第一轮 #3，2026-11-12：「更改」面板提示条点「撤销」「×」「重试」后焦点落到 body）。 */
export function focusBefore(el: HTMLElement): boolean {
  const all = Array.from(document.querySelectorAll<HTMLElement>(FOCUSABLE));
  for (let i = all.length - 1; i >= 0; i -= 1) {
    const item = all[i];
    if (el.contains(item) || !(item.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING)) continue;
    // 平时隐藏的行内按钮（display:none）拿不到焦点，跳过接着往前找。
    if (tryFocus(item)) return true;
  }
  return false;
}
