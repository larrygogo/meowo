import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { appendQuote, pickQuote, quoteBlock, QuoteSelection } from "./QuoteSelection";

afterEach(() => {
  cleanup();
  window.getSelection()?.removeAllRanges();
});

describe("引用块", () => {
  it("每行加 > ，空行留 >，多余空行收成一个", () => {
    expect(quoteBlock("第一行\n\n\n\n第二行\r\n第三行")).toBe("> 第一行\n>\n> 第二行\n> 第三行");
  });

  it("接在输入框已有内容后面：可以连引几段，中间夹自己的话；光标位置在末尾", () => {
    const first = appendQuote("", "顺手把全局圆角从 6 改成 8");
    expect(first).toBe("> 顺手把全局圆角从 6 改成 8\n\n");
    const second = appendQuote(`${first}这个不要动   \n`, "第三点");
    expect(second).toBe("> 顺手把全局圆角从 6 改成 8\n\n这个不要动\n\n> 第三点\n\n");
  });

  it("太长的引用截断并加省略号", () => {
    expect(quoteBlock("字".repeat(2100)).length).toBeLessThan(2010);
  });
});

describe("选中 AI 正文浮出「引用」", () => {
  beforeEach(() => {
    // jsdom 的 Range 没有布局：给个固定位置。
    Range.prototype.getBoundingClientRect = () => ({ top: 200, bottom: 220, left: 100, right: 300, width: 200, height: 20, x: 100, y: 200, toJSON: () => ({}) });
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => { cb(0); return 0; });
  });

  function mount(onQuote = vi.fn()) {
    const root = createRef<HTMLDivElement>();
    render(
      <div ref={root}>
        <article className="chat-message is-assistant"><div className="chat-text"><p id="a">另外我顺手把全局按钮圆角从 6 改成 8。</p></div></article>
        <article className="chat-message is-user"><div className="chat-text"><p id="u">我自己说的话</p></div></article>
        <article className="chat-message is-assistant"><div className="chat-text"><p id="b">另一条回复</p></div></article>
        <QuoteSelection root={root} onQuote={onQuote} />
      </div>,
    );
    return onQuote;
  }
  const select = (id: string, from = 0, to?: number) => {
    const node = document.getElementById(id)!.firstChild!;
    const range = document.createRange();
    range.setStart(node, from);
    range.setEnd(node, to ?? node.textContent!.length);
    const selection = window.getSelection()!;
    selection.removeAllRanges();
    selection.addRange(range);
    act(() => { document.dispatchEvent(new Event("selectionchange")); });
  };

  it("选中 AI 回复的一段：出现「引用」，点了把选中的字交出去并清掉选区", () => {
    const onQuote = mount();
    select("a", 3);
    const button = screen.getByRole("button", { name: "引用" });
    // 按下不清选区。
    expect(fireEvent.mouseDown(button)).toBe(false);
    fireEvent.click(button);
    expect(onQuote).toHaveBeenCalledWith("顺手把全局按钮圆角从 6 改成 8。");
    expect(window.getSelection()!.isCollapsed).toBe(true);
    expect(screen.queryByRole("button", { name: "引用" })).toBeNull();
  });

  it("选的是用户自己的消息、或选区为空：不出按钮", () => {
    mount();
    select("u");
    expect(screen.queryByRole("button", { name: "引用" })).toBeNull();
    window.getSelection()!.removeAllRanges();
    act(() => { document.dispatchEvent(new Event("selectionchange")); });
    expect(screen.queryByRole("button", { name: "引用" })).toBeNull();
  });

  it("从一条回复拖选到另一条：不出按钮（中间夹着用户消息）", () => {
    mount();
    const range = document.createRange();
    range.setStart(document.getElementById("a")!.firstChild!, 2);
    range.setEnd(document.getElementById("b")!.firstChild!, 2);
    const selection = window.getSelection()!;
    selection.removeAllRanges();
    selection.addRange(range);
    act(() => { document.dispatchEvent(new Event("selectionchange")); });
    expect(screen.queryByRole("button", { name: "引用" })).toBeNull();
  });

  it("选区滚出对话区：按钮收起", () => {
    mount();
    const root = document.getElementById("a")!.closest("div")!.parentElement!.parentElement!;
    root.getBoundingClientRect = () => ({ top: 400, bottom: 900, left: 0, right: 800, width: 800, height: 500, x: 0, y: 400, toJSON: () => ({}) });
    select("a");
    expect(screen.queryByRole("button", { name: "引用" })).toBeNull();
  });

  it("pickQuote 只认传入的根节点里的正文", () => {
    mount();
    select("a");
    expect(pickQuote(document.createElement("div"))).toBeNull();
  });
});
