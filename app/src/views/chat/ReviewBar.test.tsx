// 评论栏「清空」可撤销（walkthrough P-7，2026-11-12，wk3-review-clear：误点「清空」全部评论就没了，焦点掉到 body）。
import { afterEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ReviewComment } from "./diffComments";
import { ReviewBar } from "./ReviewBar";

afterEach(cleanup);

const c = (id: string): ReviewComment => ({ id, dir: "C:/repo", path: "a.ts", side: "new", line: 1, code: "x", text: id });

function Harness({ initial, onInsert = () => {} }: { initial: ReviewComment[]; onInsert?: () => void }) {
  const [list, setList] = useState<readonly ReviewComment[]>(initial);
  return (
    <>
      <ReviewBar comments={list} onClear={() => setList([])} onRestore={setList} onInsert={onInsert} />
      <button type="button" onClick={() => setList((prev) => [...prev, c("new")])}>加一条</button>
      <output data-testid="ids">{list.map((item) => item.id).join(",")}</output>
    </>
  );
}

describe("ReviewBar", () => {
  it("清空后原位给「撤销」并拿到焦点；撤销原样放回，焦点回到「插入输入框」", () => {
    render(<Harness initial={[c("a"), c("b")]} />);
    fireEvent.click(screen.getByRole("button", { name: "清空" }));
    expect(screen.getByTestId("ids").textContent).toBe("");
    expect(screen.getByTestId("review-cleared").textContent).toContain("已清空 2 条评论");
    expect(document.activeElement).toBe(screen.getByTestId("review-undo"));
    fireEvent.click(screen.getByTestId("review-undo"));
    expect(screen.getByTestId("ids").textContent).toBe("a,b");
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "插入输入框" }));
  });

  it("清空后又写了新评论：撤销条让位，不拿旧的盖掉新的；也能手动关掉", () => {
    render(<Harness initial={[c("a")]} />);
    fireEvent.click(screen.getByRole("button", { name: "清空" }));
    fireEvent.click(screen.getByRole("button", { name: "加一条" }));
    expect(screen.queryByTestId("review-undo")).toBeNull();
    expect(screen.getByTestId("ids").textContent).toBe("new");
    fireEvent.click(screen.getByRole("button", { name: "清空" }));
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(screen.queryByTestId("review-cleared")).toBeNull();
  });

  it("「插入输入框」不留撤销条（评论已经进了输入框）", () => {
    const onInsert = vi.fn();
    render(<Harness initial={[c("a")]} onInsert={onInsert} />);
    fireEvent.click(screen.getByRole("button", { name: "插入输入框" }));
    expect(onInsert).toHaveBeenCalled();
  });
});
