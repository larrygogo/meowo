import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { appendQuote, SelectionQuote } from "./SelectionQuote";

function Harness({ onQuote }: { onQuote: (text: string) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  return (
    <>
      <div ref={ref}><p data-testid="reply">先查日志，再看配置。</p></div>
      <p data-testid="outside">对话区外的文字</p>
      <textarea data-testid="box" defaultValue="" />
      <SelectionQuote container={ref} label="引用" onQuote={onQuote} />
    </>
  );
}

function select(node: Node) {
  const range = document.createRange();
  range.selectNodeContents(node);
  const selection = window.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
}

describe("SelectionQuote", () => {
  afterEach(() => {
    cleanup();
    window.getSelection()?.removeAllRanges();
  });

  it("对话区里选中文字松手后浮出「引用」，点它交出原文并收起选区", () => {
    const onQuote = vi.fn();
    render(<Harness onQuote={onQuote} />);
    expect(screen.queryByRole("button", { name: "引用" })).toBeNull();
    select(screen.getByTestId("reply").firstChild!);
    act(() => { document.dispatchEvent(new Event("pointerup")); });
    fireEvent.click(screen.getByRole("button", { name: "引用" }));
    expect(onQuote).toHaveBeenCalledWith("先查日志，再看配置。");
    expect(window.getSelection()!.isCollapsed).toBe(true);
    expect(screen.queryByRole("button", { name: "引用" })).toBeNull();
  });

  it("对话区外的选区不弹；选区收起后浮条消失", () => {
    render(<Harness onQuote={vi.fn()} />);
    select(screen.getByTestId("outside").firstChild!);
    act(() => { document.dispatchEvent(new Event("pointerup")); });
    expect(screen.queryByRole("button", { name: "引用" })).toBeNull();

    select(screen.getByTestId("reply").firstChild!);
    act(() => { document.dispatchEvent(new Event("pointerup")); });
    expect(screen.getByRole("button", { name: "引用" })).toBeTruthy();
    act(() => {
      window.getSelection()!.removeAllRanges();
      document.dispatchEvent(new Event("selectionchange"));
    });
    expect(screen.queryByRole("button", { name: "引用" })).toBeNull();
  });

  it("引用块逐行加 > ，接在已有草稿后空一行，末尾留空行给追问", () => {
    expect(appendQuote("", "第一行\n\n第二行")).toBe("> 第一行\n>\n> 第二行\n\n");
    expect(appendQuote("顺便问下  \n", "原话")).toBe("顺便问下\n\n> 原话\n\n");
  });
});
