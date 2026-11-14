import { afterEach, describe, expect, it } from "vitest";
import { restoreFocus } from "./focusRestore";

afterEach(() => {
  document.body.innerHTML = "";
});

/** jsdom 不算样式：用「行里没有焦点时 focus() 不生效」模拟 .scm-row-actions / .chat-diff-tree-more 的
 *  display:none（只在 :hover / :focus-within 时显示，styles.css 4778、4780）。 */
function hiddenUntilRowFocused(row: HTMLElement, button: HTMLElement) {
  const real = HTMLElement.prototype.focus;
  button.focus = function focus(options?: FocusOptions) {
    if (row.contains(document.activeElement)) real.call(this, options);
  };
}

describe("restoreFocus（walkthrough P-11：「更多操作」菜单 Esc 后焦点落到 body）", () => {
  it("看得见的目标直接还", () => {
    document.body.innerHTML = `<div><button id="a">a</button></div>`;
    const a = document.getElementById("a")!;
    expect(restoreFocus(a)).toBe(true);
    expect(document.activeElement).toBe(a);
  });

  it("目标只在行有焦点时显示：先点亮同一行，再还给目标", () => {
    document.body.innerHTML = `<div class="scm-row"><button id="main">login.ts</button><button id="more">更多</button></div>`;
    const row = document.querySelector<HTMLElement>(".scm-row")!;
    const more = document.getElementById("more")!;
    hiddenUntilRowFocused(row, more);
    more.focus();
    expect(document.activeElement).toBe(document.body);
    expect(restoreFocus(more)).toBe(true);
    expect(document.activeElement).toBe(more);
  });

  it("目标已不在文档里：返回 false，由调用方另找落点", () => {
    const gone = document.createElement("button");
    expect(restoreFocus(gone)).toBe(false);
    expect(restoreFocus(null)).toBe(false);
  });
});
