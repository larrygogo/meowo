import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import type { GitDiffSummaryDto, RepoChangesDto } from "../../api";
import { pickRepo, SourceControl, stagedRows, workRows } from "./SourceControl";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}));

const DIFF = ["diff --git a/src/a.ts b/src/a.ts", "--- a/src/a.ts", "+++ b/src/a.ts", "@@ -10,2 +10,2 @@", " keep", "-old", "+new"].join("\n");
const file = (path: string, indexStatus: string | null, worktreeStatus: string | null) => ({
  path, status: indexStatus ?? worktreeStatus ?? "M", additions: 1 as number | null, deletions: 0 as number | null, indexStatus, worktreeStatus,
});
const summary = (files: ReturnType<typeof file>[], branch = "main"): GitDiffSummaryDto =>
  ({ gitAvailable: true, isRepo: true, branch, ahead: 2, behind: 1, files });
const repo = (over: Partial<RepoChangesDto>): RepoChangesDto => ({
  root: "C:/w/meowo", name: "meowo", isCwdRepo: true, summary: summary([]), base: null, baseLabel: null, committed: [], ...over,
});

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command === "git_file_diff") return { path: "src/a.ts", status: "M", diff: DIFF, truncated: false };
    if (command === "git_commit") return "abc1234";
    return [];
  });
});
afterEach(cleanup);

function renderScm(repos: RepoChangesDto[], extra: Partial<Parameters<typeof SourceControl>[0]> = {}) {
  const onSelectRoot = vi.fn();
  const onChanged = vi.fn();
  const utils = render(
    <SourceControl cwd="C:/w/meowo" repos={repos} selectedRoot={null} onSelectRoot={onSelectRoot} onChanged={onChanged}
      comments={[]} touchedPaths={[]} turnTouchedPaths={[]} onAddComment={() => {}} onRemoveComment={() => {}}
      escOverrideRef={{ current: null }} {...extra} />,
  );
  return { ...utils, onSelectRoot, onChanged };
}

describe("分组规则", () => {
  it("部分暂存的文件同时出现在两组，各自的状态字母；旧后端没有新字段时全算未暂存", () => {
    const files = [file("a.ts", "M", "M"), file("b.ts", "A", null), file("c.ts", null, "U")];
    expect(stagedRows(files).map((row) => [row.file.path, row.letter])).toEqual([["a.ts", "M"], ["b.ts", "A"]]);
    expect(workRows(files).map((row) => [row.file.path, row.letter])).toEqual([["a.ts", "M"], ["c.ts", "U"]]);
    const old = [{ path: "x.ts", status: "U", additions: null, deletions: null }];
    expect(stagedRows(old)).toEqual([]);
    expect(workRows(old).map((row) => row.letter)).toEqual(["U"]);
  });

  it("默认选工作目录所在仓库；选中的还在就用它", () => {
    const repos = [repo({ root: "C:/w/wt", name: "wt", isCwdRepo: false }), repo({})];
    expect(pickRepo(repos, null)?.name).toBe("meowo");
    expect(pickRepo(repos, "c:\\w\\wt")?.name).toBe("wt");
    expect(pickRepo(repos, "C:/gone")?.name).toBe("meowo");
  });
});

describe("存储库列表", () => {
  it("显示仓库名、分支、未提交标记与 ↑↓；点选切换并通知上层", () => {
    const repos = [repo({ summary: summary([file("a.ts", null, "M")]) }), repo({ root: "C:/w/wt", name: "wt", isCwdRepo: false, summary: summary([], "feat/x") })];
    const { container, onSelectRoot } = renderScm(repos);
    const rows = [...container.querySelectorAll(".scm-repo")];
    expect(rows.map((el) => el.querySelector(".scm-repo-name")?.textContent)).toEqual(["meowo", "wt"]);
    expect(rows[0].textContent).toContain("main*");
    expect(rows[0].textContent).toContain("↑2");
    expect(rows[0].textContent).toContain("↓1");
    expect(rows[1].textContent).toContain("feat/x");
    expect(rows[1].querySelector(".scm-dirty")).toBeNull();
    expect(rows[0].getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(rows[1]);
    expect(onSelectRoot).toHaveBeenCalledWith("C:/w/wt");
  });

  it("选中 worktree 时下面是它的更改", () => {
    const repos = [repo({ summary: summary([file("main.ts", null, "M")]) }), repo({ root: "C:/w/wt", name: "wt", isCwdRepo: false, summary: summary([file("wt.ts", null, "M")]) })];
    renderScm(repos, { selectedRoot: "C:/w/wt" });
    expect(screen.getByText("wt.ts")).toBeTruthy();
    expect(screen.queryByText("main.ts")).toBeNull();
  });
});

describe("文件行与操作", () => {
  const repos = [repo({ summary: summary([file("src/a.ts", "M", null), file("src/deep/b.ts", null, "M"), file("new.txt", null, "U")]) })];

  it("文件名 + 灰色目录 + 状态字母；「更改」里是暂存、「暂存的更改」里是取消暂存", async () => {
    const { container, onChanged } = renderScm(repos);
    const work = within(screen.getByTestId("scm-group-work"));
    const row = work.getByText("b.ts").closest(".scm-row") as HTMLElement;
    expect(row.querySelector(".scm-dir")?.textContent).toBe("src/deep");
    expect(row.querySelector(".scm-status")?.textContent).toBe("M");
    fireEvent.click(within(row).getByRole("button", { name: "暂存更改" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_stage", { cwd: "C:/w/meowo", paths: ["src/deep/b.ts"] }));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    const staged = within(screen.getByTestId("scm-group-staged"));
    const stagedRow = staged.getByText("a.ts").closest(".scm-row") as HTMLElement;
    expect(within(stagedRow).queryByRole("button", { name: "暂存更改" })).toBeNull();
    fireEvent.click(within(stagedRow).getByRole("button", { name: "取消暂存" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_unstage", { cwd: "C:/w/meowo", paths: ["src/a.ts"] }));
    expect(container.querySelector(".scm-row .scm-status.is-u")?.textContent).toBe("U");
  });

  it("全部暂存 / 全部取消暂存", async () => {
    renderScm(repos);
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "全部暂存" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_stage", { cwd: "C:/w/meowo", paths: ["src/deep/b.ts", "new.txt"] }));
    fireEvent.click(within(screen.getByTestId("scm-group-staged")).getByRole("button", { name: "全部取消暂存" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_unstage", { cwd: "C:/w/meowo", paths: ["src/a.ts"] }));
  });

  it("放弃先弹确认：列出文件、Esc 关掉不执行，确认后才调命令（单个与整组）", async () => {
    renderScm(repos);
    const row = within(screen.getByTestId("scm-group-work")).getByText("new.txt").closest(".scm-row") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: "放弃更改" }));
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByText("new.txt")).toBeTruthy();
    expect(within(dialog).getByText("放弃后无法恢复。未跟踪的文件会被删除。")).toBeTruthy();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(invoke).not.toHaveBeenCalledWith("git_discard", expect.anything());
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "全部放弃" }));
    const all = screen.getByRole("dialog");
    expect(within(all).getByText("放弃 2 个文件的更改？")).toBeTruthy();
    fireEvent.click(within(all).getByRole("button", { name: "放弃更改" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_discard", { cwd: "C:/w/meowo", paths: ["src/deep/b.ts", "new.txt"] }));
  });

  it("仓库根不是会话目录本身时，写操作带 sessionCwd（手机桥据此收口）", async () => {
    const wt = repo({ root: "C:/w/wt", name: "wt", isCwdRepo: false, summary: summary([file("x.ts", null, "M")]) });
    renderScm([repo({}), wt], { selectedRoot: "C:/w/wt" });
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "全部暂存" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_stage", { cwd: "C:/w/wt", paths: ["x.ts"], sessionCwd: "C:/w/meowo" }));
  });
});

describe("做空第 1 轮", () => {
  /// 「本会话」筛选下点「全部放弃」，此前连看不见的 12 个文件一起放弃了。
  it("整组操作只作用于当前显示的行", async () => {
    const files = [file("agent.ts", null, "M"), ...Array.from({ length: 12 }, (_, i) => file(`mine${i}.ts`, null, "M"))];
    renderScm([repo({ summary: summary(files) })], { touchedPaths: ["C:/w/meowo/agent.ts"] });
    fireEvent.click(screen.getByRole("button", { name: /本会话/ }));
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "全部放弃" }));
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByText("放弃这个文件的更改？")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "放弃更改" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_discard", { cwd: "C:/w/meowo", paths: ["agent.ts"] }));
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "全部暂存" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_stage", { cwd: "C:/w/meowo", paths: ["agent.ts"] }));
  });

  it("开着筛选时不能「提交全部」，并说明原因", () => {
    const { container } = renderScm([repo({ summary: summary([file("a.ts", null, "M"), file("secret.env", null, "U")]) })]);
    fireEvent.change(screen.getByRole("textbox", { name: /^消息/ }), { target: { value: "x" } });
    expect((screen.getByRole("button", { name: /^提交/ }) as HTMLButtonElement).disabled).toBe(false);
    fireEvent.change(container.querySelector(".chat-diff-search")!, { target: { value: "a.ts" } });
    expect((screen.getByRole("button", { name: /^提交/ }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("筛选藏起了会被一起提交的文件。清除筛选后再提交")).toBeTruthy();
  });

  /// 做空第 2 轮：筛选下点「提交」（有暂存），被筛掉的暂存文件也进了提交。
  it("筛选藏起了暂存文件时「提交」也禁用；没藏的时候可以提交", () => {
    renderScm([repo({ summary: summary([file("agent.ts", "M", null), file("secret.env", "A", null)]) })], { touchedPaths: ["C:/w/meowo/agent.ts", "C:/w/meowo/secret.env"], turnTouchedPaths: ["C:/w/meowo/agent.ts"] });
    fireEvent.change(screen.getByRole("textbox", { name: /^消息/ }), { target: { value: "x" } });
    fireEvent.click(screen.getByRole("button", { name: /本会话/ }));
    expect((screen.getByRole("button", { name: /^提交/ }) as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: /本轮/ }));
    expect((screen.getByRole("button", { name: /^提交/ }) as HTMLButtonElement).disabled).toBe(true);
  });

  /// 做空第 2 轮：筛选按钮没了（touchedPaths 变空）时还停在「本会话」，面板一片空又清不掉筛选。
  it("筛选按钮消失时退回「全部」", () => {
    const props = { cwd: "C:/w/meowo", selectedRoot: null, onSelectRoot: () => {}, onChanged: () => {}, comments: [], turnTouchedPaths: [],
      onAddComment: () => {}, onRemoveComment: () => {}, escOverrideRef: { current: null } };
    const repos = [repo({ summary: summary([file("a.ts", null, "M"), file("b.ts", null, "M")]) })];
    const { rerender } = render(<SourceControl {...props} repos={repos} touchedPaths={["C:/w/meowo/a.ts"]} />);
    fireEvent.click(screen.getByRole("button", { name: /本会话/ }));
    expect(screen.queryByText("b.ts")).toBeNull();
    rerender(<SourceControl {...props} repos={repos} touchedPaths={[]} />);
    expect(screen.getByText("b.ts")).toBeTruthy();
  });

  it("冲突文件显示 !，没有暂存 / 放弃按钮，整组操作跳过它；「提交全部」把显示的文件交给后端", async () => {
    renderScm([repo({ summary: summary([file("c.txt", null, "!"), file("a.ts", null, "M")]) })]);
    const row = screen.getByText("c.txt").closest(".scm-row") as HTMLElement;
    expect(row.querySelector(".scm-status")?.textContent).toBe("!");
    expect(within(row).queryByRole("button", { name: "暂存更改" })).toBeNull();
    expect(within(row).queryByRole("button", { name: "放弃更改" })).toBeNull();
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "全部暂存" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_stage", { cwd: "C:/w/meowo", paths: ["a.ts"] }));
    fireEvent.change(screen.getByRole("textbox", { name: /^消息/ }), { target: { value: "x" } });
    fireEvent.click(screen.getByRole("button", { name: /^提交/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_commit", { cwd: "C:/w/meowo", message: "x", all: true, paths: ["c.txt", "a.ts"] }));
  });

  /// 确认框开着时选中的 worktree 从列表消失：此前确认后放弃的是主仓库的同名文件。
  it("确认框钉住仓库：仓库从列表消失就作废，不落到别的仓库；确认框写明仓库名", async () => {
    const main = repo({ summary: summary([file("src/a.ts", null, "M")]) });
    const wt = repo({ root: "C:/w/meowo-wt", name: "meowo-wt", isCwdRepo: false, summary: summary([file("src/a.ts", null, "M")]) });
    const props = { cwd: "C:/w/meowo", selectedRoot: "C:/w/meowo-wt", onSelectRoot: () => {}, onChanged: () => {}, comments: [], touchedPaths: [], turnTouchedPaths: [],
      onAddComment: () => {}, onRemoveComment: () => {}, escOverrideRef: { current: null } };
    const { rerender } = render(<SourceControl {...props} repos={[main, wt]} />);
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "全部放弃" }));
    expect(within(screen.getByRole("dialog")).getByText("仓库：meowo-wt")).toBeTruthy();
    rerender(<SourceControl {...props} repos={[main]} />);
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "放弃更改" }));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(invoke).not.toHaveBeenCalledWith("git_discard", expect.anything());
    // 照实说没执行（做空第 2 轮：此前确认框关掉就什么都没有了）。
    expect(screen.getByRole("alert").textContent).toBe("meowo-wt 已不在列表里，没有放弃任何文件");
  });

  it("有冲突时提交报 git/conflict 的文案", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "git_commit") throw "git/conflict";
      return [];
    });
    renderScm([repo({ summary: summary([file("a.ts", "M", null)]) })]);
    fireEvent.change(screen.getByRole("textbox", { name: /^消息/ }), { target: { value: "x" } });
    fireEvent.click(screen.getByRole("button", { name: /^提交/ }));
    expect(await screen.findByRole("alert")).toHaveProperty("textContent", "有未解决的合并冲突，先在终端或编辑器里解决再提交");
  });
});

describe("先显示目录，内容异步", () => {
  /// 用户 2026-11-09：没选中的仓库后端只给文件清单，选中后补拉行数和已提交段，期间提示正在加载。
  it("选中的仓库还没拉到完整内容时：文件清单照常显示，提示加载中", () => {
    renderScm([repo({ detailed: false, summary: summary([{ ...file("a.ts", null, "M"), additions: null, deletions: null }]) })]);
    expect(screen.getByText("a.ts")).toBeTruthy();
    expect(screen.getByText(/加载/)).toBeTruthy();
  });
});

describe("提交框", () => {
  it("没暂存时是「提交全部」，有暂存时是「提交」；空信息禁用；Ctrl+Enter 提交，成功后清空", async () => {
    const { rerender } = renderScm([repo({ summary: summary([file("a.ts", null, "M")]) })]);
    const button = () => screen.getByRole("button", { name: /^提交/ });
    expect(button().textContent).toBe("提交全部");
    expect((button() as HTMLButtonElement).disabled).toBe(true);
    const input = screen.getByRole("textbox", { name: "消息（Ctrl+Enter 在“main”提交）" });
    fireEvent.change(input, { target: { value: "修好了" } });
    expect((button() as HTMLButtonElement).disabled).toBe(false);
    fireEvent.keyDown(input, { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_commit", { cwd: "C:/w/meowo", message: "修好了", all: true, paths: ["a.ts"] }));
    await waitFor(() => expect((input as HTMLTextAreaElement).value).toBe(""));
    expect(screen.getByText("已提交 abc1234")).toBeTruthy();
    rerender(
      <SourceControl cwd="C:/w/meowo" repos={[repo({ summary: summary([file("a.ts", "M", null)]) })]} selectedRoot={null} onSelectRoot={() => {}} onChanged={() => {}}
        comments={[]} touchedPaths={[]} turnTouchedPaths={[]} onAddComment={() => {}} onRemoveComment={() => {}} escOverrideRef={{ current: null }} />,
    );
    expect(button().textContent).toBe("提交");
  });

  it("失败按 git/xxx 映射成看得懂的文案", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "git_commit") throw "git/identity";
      return null;
    });
    renderScm([repo({ summary: summary([file("a.ts", "M", null)]) })]);
    fireEvent.change(screen.getByRole("textbox", { name: /^消息/ }), { target: { value: "x" } });
    fireEvent.click(screen.getByRole("button", { name: /^提交/ }));
    expect(await screen.findByRole("alert")).toHaveProperty("textContent", "还没配置 git 用户名和邮箱，先在终端里运行 git config 设置");
  });
});

describe("本分支已提交（未合入 main）", () => {
  it("默认折叠、可展开、没有内容不出现、行上没有写操作；点开带 base 拉 diff，返回回到列表", async () => {
    const withCommitted = repo({ base: "0123abcd", baseLabel: "main", committed: [{ path: "done.rs", status: "A", additions: 2, deletions: 0 }] });
    const { container, unmount } = renderScm([withCommitted]);
    const group = screen.getByTestId("scm-group-committed");
    expect(group.textContent).toContain("本分支已提交（未合入 main）");
    expect(within(group).queryByText("done.rs")).toBeNull();
    fireEvent.click(within(group).getByRole("button", { expanded: false }));
    const row = within(group).getByText("done.rs").closest(".scm-row") as HTMLElement;
    expect(within(row).queryByRole("button", { name: "放弃更改" })).toBeNull();
    expect(within(row).queryByRole("button", { name: "暂存更改" })).toBeNull();
    fireEvent.click(within(group).getByText("done.rs"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_file_diff", { cwd: "C:/w/meowo", path: "done.rs", untracked: false, base: "0123abcd" }));
    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    expect(container.querySelector("[data-testid=source-control]")).toBeTruthy();
    unmount();
    renderScm([repo({ base: "0123abcd", baseLabel: "main", committed: [] })]);
    expect(screen.queryByTestId("scm-group-committed")).toBeNull();
  });
});

describe("沿用的功能", () => {
  const repos = [repo({ summary: summary([file("src/a.ts", null, "M"), file("b.md", null, "M")]) })];

  it("搜索", () => {
    const { container } = renderScm(repos);
    fireEvent.change(container.querySelector(".chat-diff-search")!, { target: { value: "b.md" } });
    expect(screen.queryByText("a.ts")).toBeNull();
    expect(screen.getByText("b.md")).toBeTruthy();
  });

  it("本会话 / 本轮筛选", () => {
    renderScm(repos, { touchedPaths: ["C:/w/meowo/b.md", "C:/w/meowo/src/a.ts"], turnTouchedPaths: ["C:/w/meowo/b.md"] });
    fireEvent.click(screen.getByRole("button", { name: /本轮/ }));
    expect(screen.queryByText("a.ts")).toBeNull();
    expect(screen.getByText("b.md")).toBeTruthy();
  });

  it("树形：按目录折叠显示", () => {
    const { container } = renderScm(repos);
    fireEvent.click(screen.getByRole("button", { name: "树形" }));
    expect([...container.querySelectorAll(".scm-row.is-dir .scm-name")].map((el) => el.textContent)).toEqual(["src"]);
    fireEvent.click(screen.getByRole("button", { name: "列表" }));
    expect(container.querySelector(".scm-row.is-dir")).toBeNull();
  });

  it("评论计数；点文件进 diff 再返回；别的仓库里的评论记在那个仓库的根下", async () => {
    const onAddComment = vi.fn();
    const wt = repo({ root: "C:/w/wt", name: "wt", isCwdRepo: false, summary: summary([file("src/a.ts", null, "M")]) });
    renderScm([repo({}), wt], {
      selectedRoot: "C:/w/wt",
      onAddComment,
      comments: [{ id: "c1", dir: "C:/w/wt", path: "src/a.ts", line: 10, side: "new", text: "旧评论", snippet: "" } as never],
    });
    const row = screen.getByText("a.ts").closest(".scm-row") as HTMLElement;
    expect(row.querySelector(".chat-diff-cmt-count")?.textContent).toBe("1");
    fireEvent.click(screen.getByText("a.ts"));
    const adds = await screen.findAllByRole("button", { name: "评论这一行" });
    fireEvent.click(adds[1]);
    fireEvent.change(screen.getByPlaceholderText("写下对这一行的意见，Enter 添加"), { target: { value: "看这里" } });
    fireEvent.keyDown(screen.getByPlaceholderText("写下对这一行的意见，Enter 添加"), { key: "Enter" });
    expect(onAddComment).toHaveBeenCalledWith(expect.objectContaining({ path: "src/a.ts", text: "看这里" }), "C:/w/wt");
    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    expect(screen.getByTestId("source-control")).toBeTruthy();
  });

  /// 做空复现过：5 秒轮询换了对象（首屏单仓库摘要的反斜杠根换成 overview 的正斜杠根）不能把正在看的文件踢走。
  it("轮询换了根的写法，正在看的文件不被踢回列表", async () => {
    const first = repo({ root: "C:\\w\\meowo", summary: summary([file("src/a.ts", null, "M")]) });
    const { rerender } = renderScm([first]);
    fireEvent.click(screen.getByText("a.ts"));
    await screen.findByRole("button", { name: "返回" });
    rerender(
      <SourceControl cwd="C:/w/meowo" repos={[{ ...first, root: "C:/w/meowo" }]} selectedRoot={null} onSelectRoot={() => {}} onChanged={() => {}}
        comments={[]} touchedPaths={[]} turnTouchedPaths={[]} onAddComment={() => {}} onRemoveComment={() => {}} escOverrideRef={{ current: null }} />,
    );
    expect(await screen.findByRole("button", { name: "返回" })).toBeTruthy();
  });
});
