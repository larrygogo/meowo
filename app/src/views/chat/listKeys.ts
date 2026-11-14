// 面板里的文件列表 / 树用方向键移动（walkthrough P-12，2026-11-12）：以前 ↓ 按了焦点不动，只能一格一格 Tab，
// 「更改」里一行五个停靠点（3 个文件就有 27 个，wk3-tabstops）。和页签条（←→/Home/End）一个手感：
// ↑↓ 上下一行，Home/End 首尾；目录行（带 aria-expanded）→ 展开、← 收起，已收起再按 ← 跳到上一级目录行。
import type { KeyboardEvent as ReactKeyboardEvent } from "react";

/** 挂在列表容器的 onKeyDown 上；rowSelector 选出每一行的主按钮（容器内按 DOM 顺序）。 */
export function moveAmongRows(event: ReactKeyboardEvent<HTMLElement>, rowSelector: string): void {
  if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
  const target = event.target instanceof HTMLElement ? event.target : null;
  if (!target?.matches(rowSelector)) return;
  const rows = Array.from(event.currentTarget.querySelectorAll<HTMLElement>(rowSelector));
  const index = rows.indexOf(target);
  if (index < 0) return;
  const expanded = target.getAttribute("aria-expanded");
  let next: number;
  switch (event.key) {
    case "ArrowDown": next = Math.min(rows.length - 1, index + 1); break;
    case "ArrowUp": next = Math.max(0, index - 1); break;
    case "Home": next = 0; break;
    case "End": next = rows.length - 1; break;
    case "ArrowRight":
      if (expanded !== "false") return;
      event.preventDefault();
      target.click();
      return;
    case "ArrowLeft": {
      if (expanded === "true") {
        event.preventDefault();
        target.click();
        return;
      }
      // 跳到上一级目录：往上找第一个层级比自己浅的目录行（行按钮上的 data-depth）。
      const depth = Number(target.dataset.depth ?? 0);
      for (let i = index - 1; i >= 0; i -= 1) {
        const row = rows[i];
        if (row.hasAttribute("aria-expanded") && Number(row.dataset.depth ?? 0) < depth) {
          event.preventDefault();
          row.focus();
          return;
        }
      }
      return;
    }
    default: return;
  }
  event.preventDefault();
  rows[next]?.focus();
  rows[next]?.scrollIntoView?.({ block: "nearest" });
}
