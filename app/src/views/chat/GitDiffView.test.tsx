import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import type { GitDiffSummaryDto } from "../../api";
import type { ReviewComment } from "./diffComments";
import { GitDiffView } from "./GitDiffView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}));

const DIFF = ["diff --git a/src/a.ts b/src/a.ts", "--- a/src/a.ts", "+++ b/src/a.ts", "@@ -10,2 +10,2 @@", " keep", "-old", "+new"].join("\n");
const summary: GitDiffSummaryDto = { gitAvailable: true, isRepo: true, branch: "main", files: [{ path: "src/a.ts", status: "M", additions: 1, deletions: 1 }] };

afterEach(cleanup);

function renderPanel(comments: ReviewComment[] = [], touchedPaths: string[] = []) {
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command === "git_file_diff") return { path: "src/a.ts", status: "M", diff: DIFF, truncated: false };
    return [];
  });
  const onAddComment = vi.fn();
  const onRemoveComment = vi.fn();
  const onCollapse = vi.fn();
  render(
    <GitDiffView
      cwd={"C:\\repo"}
      dirs={["C:\\repo"]}
      onDirChange={() => {}}
      onAddDir={null}
      onRemoveDir={() => {}}
      summary={summary}
      width={420}
      collapsed={false}
      maximized={false}
      onCollapse={onCollapse}
      onToggleMaximize={() => {}}
      comments={comments}
      onAddComment={onAddComment}
      onRemoveComment={onRemoveComment}
      touchedPaths={touchedPaths}
    />,
  );
  return { onAddComment, onRemoveComment, onCollapse };
}

describe("GitDiffView 行数与本会话过滤", () => {
  it("文件后面显示增删行数;勾上「只看本会话改的」只留会话碰过的文件", () => {
    const many: GitDiffSummaryDto = {
      ...summary,
      files: [
        { path: "src/a.ts", status: "M", additions: 12, deletions: 3 },
        { path: "src/b.ts", status: "M", additions: 1, deletions: 0 },
        { path: "logo.png", status: "M", additions: null, deletions: null },
      ],
    };
    vi.mocked(invoke).mockResolvedValue([]);
    render(
      <GitDiffView
        cwd={"C:\\repo"} dirs={["C:\\repo"]} onDirChange={() => {}} onAddDir={null} onRemoveDir={() => {}}
        summary={many} width={420} collapsed={false} maximized={false} onCollapse={() => {}} onToggleMaximize={() => {}}
        comments={[]} onAddComment={() => {}} onRemoveComment={() => {}}
        touchedPaths={["c:/repo/src/a.ts"]}
      />,
    );
    const rowA = screen.getByRole("button", { name: /a\.ts/ });
    expect(rowA.textContent).toContain("+12");
    expect(rowA.textContent).toContain("−3");
    // 二进制数不出行数,不显示。
    expect(screen.getByRole("button", { name: /logo\.png/ }).querySelector(".chat-diff-lines")).toBeNull();

    fireEvent.click(screen.getByLabelText("只看本会话改的（1）"));
    expect(screen.queryByRole("button", { name: /b\.ts/ })).toBeNull();
    expect(screen.getByRole("button", { name: /a\.ts/ })).toBeTruthy();
  });
});

describe("GitDiffView 行评论", () => {
  it("点行左缘的 + 写评论,Enter 交给上层,带上路径、行号与那行代码", async () => {
    const { onAddComment, onCollapse } = renderPanel();
    fireEvent.click(screen.getByRole("button", { name: /a\.ts/ }));
    const adds = await screen.findAllByRole("button", { name: "评论这一行" });
    // keep / old / new 三行可评论,头行与 @@ 不可。
    expect(adds).toHaveLength(3);
    fireEvent.click(adds[1]);
    const box = screen.getByPlaceholderText("写下对这一行的意见，Enter 添加");
    // Esc 只关评论框,不收起整个面板。
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onCollapse).not.toHaveBeenCalled();
    expect(screen.queryByPlaceholderText("写下对这一行的意见，Enter 添加")).toBeNull();

    fireEvent.click(adds[1]);
    fireEvent.change(screen.getByPlaceholderText("写下对这一行的意见，Enter 添加"), { target: { value: "别删这行" } });
    fireEvent.keyDown(screen.getByPlaceholderText("写下对这一行的意见，Enter 添加"), { key: "Enter" });
    expect(onAddComment).toHaveBeenCalledWith({ path: "src/a.ts", side: "old", line: 11, code: "old", text: "别删这行" });
    expect(box.isConnected).toBe(false);

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onCollapse).toHaveBeenCalledTimes(1);
  });

  it("已有评论显示在对应行下面,文件树上标条数,× 删除", async () => {
    const comment: ReviewComment = { id: "c1", dir: "C:\\repo", path: "src/a.ts", side: "new", line: 11, code: "new", text: "命名再想想" };
    const { onRemoveComment } = renderPanel([comment, { ...comment, id: "c2", dir: "D:\\other" }]);
    // 别的仓的评论不算进来。
    expect(screen.getByLabelText("1 条改动评论").textContent).toBe("1");
    fireEvent.click(screen.getByRole("button", { name: /a\.ts/ }));
    const note = await screen.findByText("命名再想想");
    fireEvent.click(within(note.parentElement!).getByRole("button", { name: "删除评论" }));
    expect(onRemoveComment).toHaveBeenCalledWith("c1");
  });
});
