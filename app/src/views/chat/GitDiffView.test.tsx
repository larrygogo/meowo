import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import type { GitDiffSummaryDto, RepoChangesDto } from "../../api";
import type { ReviewComment } from "./diffComments";
import { ChangesView, FilesView, GitDiffView, PathMentionContext, RepoChanges } from "./GitDiffView";
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
    expect(onAddComment).toHaveBeenCalledWith({ path: "src/a.ts", side: "old", line: 11, code: "old", text: "别删这行" }, "C:\\repo");
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

describe("GitDiffView 列表 / 树形（对照 Kimi 改动面板）", () => {
  afterEach(() => localStorage.clear());
  it("默认列表一行一个完整路径，行头是状态方块；切到树形按目录收拢并记住", () => {
    vi.mocked(invoke).mockResolvedValue([]);
    const props = {
      cwd: "C:\repo", width: 420, collapsed: false, maximized: false, onCollapse: () => {}, onToggleMaximize: () => {},
      comments: [], onAddComment: () => {}, onRemoveComment: () => {}, touchedPaths: [],
    };
    const files = [
      { path: "app/shot.png", status: "U", additions: null, deletions: null },
      { path: "app/src/views/chat/PillSelect.tsx", status: "A", additions: 80, deletions: 0 },
      { path: "README.md", status: "D", additions: 0, deletions: 3 },
    ];
    const { unmount } = render(<GitDiffView {...props} summary={{ ...summary, files }} />);
    expect(screen.getByText("3 个文件")).toBeTruthy();
    expect(screen.getByRole("button", { name: "列表" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByText("app/src/views/chat/PillSelect.tsx")).toBeTruthy();
    // 新增 / 未跟踪是「+」，删除是「−」，读屏报状态名。
    expect(screen.getByRole("img", { name: "新文件，未跟踪" }).textContent).toBe("+");
    expect(screen.getByRole("img", { name: "已删除" }).textContent).toBe("−");

    fireEvent.click(screen.getByRole("button", { name: "树形" }));
    expect(screen.queryByText("app/src/views/chat/PillSelect.tsx")).toBeNull();
    expect(screen.getByText("PillSelect.tsx")).toBeTruthy();
    unmount();
    render(<GitDiffView {...props} summary={{ ...summary, files }} />);
    expect(screen.getByRole("button", { name: "树形" }).getAttribute("aria-pressed")).toBe("true");
  });
});

describe("GitDiffView 文件页：树与预览同时在场（对照 Kimi）", () => {
  it("点文件后树仍挂着并标出当前文件，宽面板由 CSS 并排、窄面板只显示预览", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "list_dir_entries") return [
        { name: "a.ts", relPath: "a.ts", isDir: false },
        { name: "b.ts", relPath: "b.ts", isDir: false },
      ];
      if (command === "read_file_text") return { relPath: "a.ts", text: "export const a = 1;\n", truncated: false, binary: false };
      return [];
    });
    const { container } = render(
      <GitDiffView cwd="C:\repo" summary={summary} width={420} collapsed={false} maximized={false}
        onCollapse={() => {}} onToggleMaximize={() => {}} comments={[]} onAddComment={() => {}} onRemoveComment={() => {}}
        touchedPaths={[]} activeTab="files" onTabChange={() => {}} />,
    );
    fireEvent.click(await screen.findByText("a.ts"));
    // 高亮把代码拆成多段，按预览区整段文本断言。
    await waitFor(() => expect(container.querySelector(".chat-files-preview pre")?.textContent).toContain("export const a"));
    const files = container.querySelector(".chat-files")!;
    expect(files.className).toContain("has-selection");
    // 树没卸载：并排时直接点下一个文件，不用先返回。
    const row = within(files.querySelector(".chat-files-browse") as HTMLElement).getByText("a.ts").closest("button")!;
    expect(row.getAttribute("aria-current")).toBe("true");
    expect(within(files.querySelector(".chat-files-browse") as HTMLElement).getByText("b.ts")).toBeTruthy();
  });

  it("更改：旧/新两列行号，自动换行开关记住；没有改动时说工作区干净", async () => {
    localStorage.removeItem("meowo-code-wrap");
    const { container } = render(<ChangesHarness />);
    fireEvent.click(screen.getByRole("button", { name: /a\.ts/ }));
    await waitFor(() => expect(container.querySelector(".chat-diff-row")).toBeTruthy());
    const rows = Array.from(container.querySelectorAll(".chat-diff-row"));
    const nos = rows.map((row) => [row.querySelector(".is-old")?.textContent, row.querySelector(".is-new")?.textContent]);
    expect(nos).toEqual([["10", "10"], ["11", ""], ["", "11"]]);
    const wrap = screen.getByRole("button", { name: "自动换行" });
    fireEvent.click(wrap);
    expect(container.querySelector("pre")!.className).toContain("is-wrap");
    expect(localStorage.getItem("meowo-code-wrap")).toBe("1");
    cleanup();
    render(<ChangesHarness summary={{ ...summary, files: [] }} />);
    expect(screen.getByText("工作区干净，无改动")).toBeTruthy();
  });

  it("更改：面板可见时后台发现打开的文件变了，刷新亮成橙色，点了才换", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      let diff = DIFF;
      vi.mocked(invoke).mockImplementation(async (command: string) => (command === "git_file_diff" ? { path: "src/a.ts", status: "M", diff, truncated: false } : []));
      const { container } = render(<ChangesHarness live />);
      fireEvent.click(screen.getByRole("button", { name: /a\.ts/ }));
      await waitFor(() => expect(container.querySelector("pre")?.textContent).toContain("new"));
      expect(screen.getByRole("button", { name: "刷新" }).className).not.toContain("is-stale");
      diff = DIFF.replace("+new", "+newer");
      await vi.advanceTimersByTimeAsync(5100);
      const stale = await screen.findByRole("button", { name: "文件有更新，点击刷新" });
      expect(stale.className).toContain("is-stale");
      // 没点之前眼前的内容不动。
      expect(container.querySelector("pre")?.textContent).not.toContain("newer");
      fireEvent.click(stale);
      await waitFor(() => expect(container.querySelector("pre")?.textContent).toContain("newer"));
      expect(screen.getByRole("button", { name: "刷新" }).className).not.toContain("is-stale");
    } finally {
      vi.useRealTimers();
    }
  });

  it("文件：树行「…」里有添加到对话；压成一行的 JSON 展开显示；文件树位置记住", async () => {
    localStorage.removeItem("meowo-file-tree-position");
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "list_dir_entries") return [{ name: "data.json", relPath: "data.json", isDir: false }];
      if (command === "read_file_text") return { relPath: "data.json", text: '{"a":1,"b":[2]}', truncated: false, binary: false };
      return [];
    });
    const mention = vi.fn();
    const { container } = render(
      <PathMentionContext.Provider value={mention}>
        <FilesView cwd="C:
epo" />
      </PathMentionContext.Provider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "更多操作" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /添加到对话/ }));
    expect(mention).toHaveBeenCalledWith("data.json");

    fireEvent.click(screen.getByText("data.json"));
    await waitFor(() => expect(container.querySelector(".chat-files-preview pre")?.textContent).toContain('"a": 1'));
    expect(container.querySelectorAll(".chat-files-preview pre > div").length).toBeGreaterThan(1);

    fireEvent.click(screen.getByRole("button", { name: "文件树位置" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /放在右侧/ }));
    expect(container.querySelector(".chat-files")!.className).toContain("is-tree-right");
    expect(localStorage.getItem("meowo-file-tree-position")).toBe("right");
  });

  it("更改列表和文件搜索结果的行尾也有「…」；已删除的文件没有", async () => {
    const withDeleted: GitDiffSummaryDto = { ...summary, files: [...summary.files, { path: "gone.ts", status: "D", additions: 0, deletions: 3 }] };
    render(<ChangesHarness summary={withDeleted} />);
    expect(screen.getAllByRole("button", { name: "更多操作" })).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "更多操作" }));
    expect(screen.getByRole("menuitem", { name: /复制路径/ })).toBeTruthy();
    cleanup();

    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "search_project_files") return { files: [{ relPath: "src/a.ts", isDir: false, lines: [{ line: 3, preview: "const needle = 1;" }], totalLineMatches: 1 }], truncated: false };
      return [];
    });
    render(<FilesView cwd="C:\\repo" />);
    fireEvent.change(screen.getByPlaceholderText("搜索文件名或内容…"), { target: { value: "needle" } });
    await screen.findByText("src/a.ts");
    fireEvent.click(screen.getByRole("button", { name: "更多操作" }));
    expect(screen.getByRole("menuitem", { name: /在文件管理器中显示|定位/ })).toBeTruthy();
  });

  it("文件：分隔条用键盘调宽，夹在 160–400 之间", async () => {
    localStorage.removeItem("meowo-file-tree-width");
    localStorage.removeItem("meowo-file-tree-position");
    vi.mocked(invoke).mockResolvedValue([]);
    const { container } = render(<FilesView cwd="C:
epo" />);
    const bar = screen.getByRole("separator", { name: "调整文件树宽度" });
    expect(bar.getAttribute("aria-valuenow")).toBe("224");
    fireEvent.keyDown(bar, { key: "ArrowRight", shiftKey: true });
    expect(bar.getAttribute("aria-valuenow")).toBe("272");
    for (let i = 0; i < 10; i += 1) fireEvent.keyDown(bar, { key: "ArrowRight", shiftKey: true });
    expect(bar.getAttribute("aria-valuenow")).toBe("400");
    expect((container.querySelector(".chat-files") as HTMLElement).style.getPropertyValue("--tree-w")).toBe("400px");
    expect(localStorage.getItem("meowo-file-tree-width")).toBe("400");
  });
});

function ChangesHarness({ summary: sum = summary, live = false }: { summary?: GitDiffSummaryDto; live?: boolean }) {
  if (!live) {
    vi.mocked(invoke).mockImplementation(async (command: string) => (command === "git_file_diff" ? { path: "src/a.ts", status: "M", diff: DIFF, truncated: false } : []));
  }
  return (
    <ChangesView cwd="C:
epo" summary={sum} comments={[]} touchedPaths={[]} turnTouchedPaths={[]}
      onAddComment={() => {}} onRemoveComment={() => {}} escOverrideRef={{ current: null }} live={live} />
  );
}


/// 多仓库「更改」（用户要求 2026-11-09）：会话开在父目录、agent 新开 worktree 并提交后也看得到改动。
describe("RepoChanges", () => {
  /// 做空复现：agent 刚提交、出现已提交段时，面板从单仓库切到分组形态，正在看的文件被踢回列表。
  it("轮询让形态从单仓库切到分组时，正在看的文件不被踢走", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string) => (command === "git_file_diff" ? { path: "src/a.ts", status: "M", diff: DIFF, truncated: false } : []));
    const props = { cwd: "C:/w/meowo", comments: [], touchedPaths: [], turnTouchedPaths: [], onAddComment: () => {}, onRemoveComment: () => {}, escOverrideRef: { current: null } };
    const one: RepoChangesDto = { root: "C:/w/meowo", name: "meowo", isCwdRepo: true, summary, base: null, baseLabel: null, committed: [] };
    const { rerender, container } = render(<RepoChanges {...props} repos={[one]} />);
    fireEvent.click(screen.getByText("src/a.ts"));
    await screen.findByRole("button", { name: "返回" });
    rerender(<RepoChanges {...props} repos={[{ ...one, base: "0123abcd", baseLabel: "main", committed: [{ path: "done.rs", status: "A", additions: 1, deletions: 0 }] }]} />);
    expect(await screen.findByRole("button", { name: "返回" })).toBeTruthy();
    expect(container.querySelector("[data-testid=repo-changes]")).toBeNull();
    // 返回才回到分组列表。
    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    expect(container.querySelector("[data-testid=repo-changes]")).toBeTruthy();
  });

  it("别的仓库里写的评论记在那个仓库的根下；分组列表有搜索框", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string) => (command === "git_file_diff" ? { path: "src/a.ts", status: "M", diff: DIFF, truncated: false } : []));
    const onAddComment = vi.fn();
    const other: RepoChangesDto = { root: "C:/w/other", name: "other", isCwdRepo: false, summary: { ...summary, files: [{ path: "src/a.ts", status: "M", additions: 1, deletions: 1 }, { path: "b.md", status: "M", additions: 1, deletions: 0 }] }, base: null, baseLabel: null, committed: [] };
    const mine: RepoChangesDto = { root: "C:/w/meowo", name: "meowo", isCwdRepo: true, summary: { ...summary, files: [] }, base: null, baseLabel: null, committed: [] };
    const { container } = render(<RepoChanges cwd="C:/w" repos={[mine, other]} comments={[]} touchedPaths={[]} turnTouchedPaths={[]}
      onAddComment={onAddComment} onRemoveComment={() => {}} escOverrideRef={{ current: null }} />);
    fireEvent.change(container.querySelector(".chat-diff-search")!, { target: { value: "b.md" } });
    expect(screen.queryByText("src/a.ts")).toBeNull();
    fireEvent.change(container.querySelector(".chat-diff-search")!, { target: { value: "" } });
    fireEvent.click(screen.getByText("src/a.ts"));
    const adds = await screen.findAllByRole("button", { name: "评论这一行" });
    fireEvent.click(adds[1]);
    fireEvent.change(screen.getByPlaceholderText("写下对这一行的意见，Enter 添加"), { target: { value: "看这里" } });
    fireEvent.keyDown(screen.getByPlaceholderText("写下对这一行的意见，Enter 添加"), { key: "Enter" });
    expect(onAddComment).toHaveBeenCalledWith(expect.objectContaining({ path: "src/a.ts", text: "看这里" }), "C:/w/other");
  });

  const repo = (over: Partial<RepoChangesDto>): RepoChangesDto => ({
    root: "C:/w/meowo", name: "meowo", isCwdRepo: true, summary, base: null, baseLabel: null, committed: [], ...over,
  });
  const renderRepos = (repos: RepoChangesDto[]) => {
    vi.mocked(invoke).mockImplementation(async (command: string) => (command === "git_file_diff" ? { path: "src/a.ts", status: "M", diff: DIFF, truncated: false } : []));
    return render(
      <RepoChanges cwd="C:/w/meowo" repos={repos} comments={[]} touchedPaths={[]} turnTouchedPaths={[]}
        onAddComment={() => {}} onRemoveComment={() => {}} escOverrideRef={{ current: null }} />,
    );
  };

  it("只有工作目录仓库、只有未提交时与原来一样：不出分组标题", () => {
    const { container } = renderRepos([repo({})]);
    expect(container.querySelector("[data-testid=repo-changes]")).toBeNull();
    expect(container.querySelector(".chat-repo-head")).toBeNull();
    expect(screen.getByText("src/a.ts")).toBeTruthy();
  });

  it("多仓库按「仓库名 分支」分组，分未提交 / 已提交未合并两段；点已提交段的文件带 base 拉 diff，返回回到分组", async () => {
    const worktree = repo({
      root: "C:/w/meowo-feat",
      name: "meowo-feat",
      isCwdRepo: false,
      summary: { ...summary, branch: "feat/x", files: [{ path: "wip.rs", status: "U", additions: 3, deletions: 0 }] },
      base: "0123abcd",
      baseLabel: "main",
      committed: [{ path: "done.rs", status: "A", additions: 2, deletions: 0 }],
    });
    const { container } = renderRepos([repo({ summary: { ...summary, files: [] } }), worktree]);
    const heads = [...container.querySelectorAll(".chat-repo-head")].map((el) => el.textContent);
    expect(heads).toEqual(["meowo· main", "meowo-feat· feat/x"]);
    const labels = [...container.querySelectorAll(".chat-repo-label")].map((el) => el.textContent);
    expect(labels).toEqual(["未提交", "未提交", "已提交未合并 · 相对 main"]);
    expect(screen.getByText("没有改动")).toBeTruthy();
    fireEvent.click(screen.getByText("done.rs"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_file_diff", {
      cwd: "C:/w/meowo-feat", path: "done.rs", untracked: false, base: "0123abcd", sessionCwd: "C:/w/meowo",
    }));
    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    expect(container.querySelector("[data-testid=repo-changes]")).toBeTruthy();
    fireEvent.click(screen.getByText("wip.rs"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_file_diff", {
      cwd: "C:/w/meowo-feat", path: "wip.rs", untracked: true, sessionCwd: "C:/w/meowo",
    }));
  });
});
