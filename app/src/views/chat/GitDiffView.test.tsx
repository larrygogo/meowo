import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import type { GitDiffSummaryDto } from "../../api";
import type { ReviewComment } from "./diffComments";
import { GitDiffView } from "./GitDiffView";
import { markRemoteUi } from "../../remoteMode";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}));

const DIFF = ["diff --git a/src/a.ts b/src/a.ts", "--- a/src/a.ts", "+++ b/src/a.ts", "@@ -10,2 +10,2 @@", " keep", "-old", "+new"].join("\n");
const summary: GitDiffSummaryDto = { gitAvailable: true, isRepo: true, branch: "main", ahead: null, behind: null, files: [{ path: "src/a.ts", status: "M", additions: 1, deletions: 1 }] };

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
  it("文件后面显示增删行数;改动范围切本会话 / 本轮只留对应碰过的文件", () => {
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
        cwd={"C:\\repo"}        summary={many} width={420} collapsed={false} maximized={false} onCollapse={() => {}} onToggleMaximize={() => {}}
        comments={[]} onAddComment={() => {}} onRemoveComment={() => {}}
        touchedPaths={["c:/repo/src/a.ts", "c:/repo/src/b.ts"]}
        turnTouchedPaths={["c:/repo/src/b.ts"]}
      />,
    );
    const rowA = screen.getByRole("button", { name: /a\.ts/ });
    expect(rowA.textContent).toContain("+12");
    expect(rowA.textContent).toContain("−3");
    // 二进制数不出行数,不显示。
    expect(screen.getByRole("button", { name: /logo\.png/ }).querySelector(".chat-diff-lines")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "本会话 2" }));
    expect(screen.queryByRole("button", { name: /logo\.png/ })).toBeNull();
    expect(screen.getByRole("button", { name: /a\.ts/ })).toBeTruthy();
    // 本轮（最后一条用户消息之后）只碰了 b.ts。
    fireEvent.click(screen.getByRole("button", { name: "本轮 1" }));
    expect(screen.queryByRole("button", { name: /a\.ts/ })).toBeNull();
    expect(screen.getByRole("button", { name: /b\.ts/ })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "全部" }));
    expect(screen.getByRole("button", { name: /logo\.png/ })).toBeTruthy();
  });
});

describe("GitDiffView 分支行", () => {
  /// 领先/落后远程（对照 Kimi）：agent 提交后没推送，分支行直接显示 ↑N；为 0 或没有上游不显示。
  it("分支名后显示领先/落后远程的提交数", () => {
    vi.mocked(invoke).mockResolvedValue([]);
    const props = {
      cwd: "C:\\repo",      width: 420, collapsed: false, maximized: false, onCollapse: () => {}, onToggleMaximize: () => {},
      comments: [], onAddComment: () => {}, onRemoveComment: () => {}, touchedPaths: [],
    };
    const { rerender } = render(<GitDiffView {...props} summary={{ ...summary, ahead: 2, behind: 0 }} />);
    expect(screen.getByText("↑2").getAttribute("data-tip")).toBe("领先远程 2 个提交，尚未推送");
    expect(screen.queryByText(/↓/)).toBeNull();
    rerender(<GitDiffView {...props} summary={{ ...summary, ahead: null, behind: null }} />);
    expect(screen.queryByText(/↑/)).toBeNull();
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

describe("GitDiffView 受控页签", () => {
  it("顶栏可直达文件树，面板内切页通过回调交给上层", async () => {
    vi.mocked(invoke).mockResolvedValue([]);
    const onTabChange = vi.fn();
    render(
      <GitDiffView
        cwd={"C:\\repo"}        summary={summary} width={420} collapsed={false} maximized={false} onCollapse={() => {}} onToggleMaximize={() => {}}
        comments={[]} onAddComment={() => {}} onRemoveComment={() => {}} touchedPaths={[]}
        activeTab="files" onTabChange={onTabChange}
      />,
    );
    expect(await screen.findByPlaceholderText("搜索文件名或内容…")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "更改" }));
    expect(onTabChange).toHaveBeenCalledWith("changes");
  });
});

describe("GitDiffView 远程头行", () => {
  afterEach(() => {
    (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = undefined;
    document.body.classList.remove("remote-ui");
  });

  // 手机上只有「更改」一页：头行给返回箭头 + 标题，不再是单个分段页签 + 收起图标（用户实拍）。
  it("手机头行是「返回 + 更改」，返回即收起面板", async () => {
    markRemoteUi();
    const { onCollapse } = renderPanel();
    expect(screen.queryByRole("button", { name: "收起面板" })).toBeNull();
    expect(screen.queryByRole("button", { name: "更改" })).toBeNull();
    expect(document.querySelector(".chat-diff-title")?.textContent).toBe("更改");
    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    expect(onCollapse).toHaveBeenCalled();
  });
});
