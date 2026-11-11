// 「更改」面板的大目录折叠与虚拟列表（用户实测：没进 .gitignore 的构建目录 15612 个文件，面板卡死）。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import type { GitDiffSummaryDto, RepoChangesDto } from "../../api";
import { SourceControl } from "./SourceControl";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}));
let remote = false;
vi.mock("../../remoteMode", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../remoteMode")>()),
  remoteUi: () => remote,
}));

type F = { path: string; status: string; additions: number | null; deletions: number | null; indexStatus: string | null; worktreeStatus: string | null; untrackedFiles?: number | null };
const work = (path: string, letter = "M"): F => ({ path, status: letter, additions: null, deletions: null, indexStatus: null, worktreeStatus: letter });
const folded = (path: string, n: number): F => ({ ...work(path, "U"), untrackedFiles: n });
const summary = (files: F[]): GitDiffSummaryDto => ({ gitAvailable: true, isRepo: true, branch: "main", ahead: null, behind: null, files });
const repo = (files: F[]): RepoChangesDto =>
  ({ root: "C:/w/meowo", name: "meowo", isCwdRepo: true, summary: summary(files), base: null, baseLabel: null, committed: [] });
const kids = (dir: string, n: number) => Array.from({ length: n }, (_, i) => work(`${dir}d${Math.floor(i / 10)}/f${i}.js`, "U"));

let dirListing: F[] = [];
beforeEach(() => {
  remote = false;
  try { localStorage.removeItem("meowo-diff-layout"); } catch { /* 隐私模式 */ }
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command === "git_untracked_dir_files") return dirListing;
    if (command === "git_file_diff") return { path: "x", status: "U", diff: "--- /dev/null\n+++ b/x\n+x\n", truncated: false };
    return [];
  });
});
afterEach(cleanup);

function renderScm(repos: RepoChangesDto[]) {
  const onChanged = vi.fn();
  const props = {
    cwd: "C:/w/meowo", selectedRoot: null, onSelectRoot: () => {}, onChanged, comments: [], touchedPaths: [], turnTouchedPaths: [],
    onAddComment: () => {}, onRemoveComment: () => {}, escOverrideRef: { current: null },
  };
  const utils = render(<SourceControl {...props} repos={repos} />);
  return { ...utils, onChanged, rerender: (next: RepoChangesDto[]) => utils.rerender(<SourceControl {...props} repos={next} />) };
}
const folderRow = (name: string) => within(screen.getByTestId("scm-group-work")).getByText(name).closest(".scm-row") as HTMLElement;
const calls = (command: string) => vi.mocked(invoke).mock.calls.filter(([c]) => c === command);

describe("折叠的未跟踪目录", () => {
  it("显示「目录名/ · N 个文件」，组计数按文件数；点开才拉其中的文件，轮询重渲染不重拉", async () => {
    dirListing = kids("build/out/", 3);
    const { rerender } = renderScm([repo([work("a.ts"), folded("build/out/", 15000)])]);
    const row = folderRow("out/");
    expect(row.textContent).toContain("· 15000 个文件");
    expect(row.querySelector(".scm-dir")?.textContent).toBe("build");
    expect(within(screen.getByTestId("scm-group-work")).getByText("15001")).toBeTruthy();
    expect(calls("git_untracked_dir_files")).toHaveLength(0);
    fireEvent.click(within(row).getByRole("button", { name: /out\// }));
    await waitFor(() => expect(screen.getByText("f2.js")).toBeTruthy());
    expect(invoke).toHaveBeenCalledWith("git_untracked_dir_files", { cwd: "C:/w/meowo", path: "build/out/" });
    // 目录内的相对位置（上级已在折叠行上）。
    expect((screen.getByText("f2.js").closest(".scm-row") as HTMLElement).querySelector(".scm-dir")?.textContent).toBe("d0");
    // 5 秒轮询换了 repos 对象：不重拉。
    rerender([repo([work("a.ts"), folded("build/out/", 15000)])]);
    rerender([repo([work("a.ts"), folded("build/out/", 15001)])]);
    expect(calls("git_untracked_dir_files")).toHaveLength(1);
    // 收起再点开：现拉。
    fireEvent.click(within(folderRow("out/")).getByRole("button", { name: /out\// }));
    expect(screen.queryByText("f2.js")).toBeNull();
    fireEvent.click(within(folderRow("out/")).getByRole("button", { name: /out\// }));
    await waitFor(() => expect(calls("git_untracked_dir_files")).toHaveLength(2));
  });

  it("点开后的文件逐个暂存 / 放弃，路径是目录里的文件本身；点文件进 diff 按未跟踪读", async () => {
    dirListing = kids("out/", 3);
    renderScm([repo([folded("out/", 60)])]);
    fireEvent.click(within(folderRow("out/")).getByRole("button", { name: /out\// }));
    const kid = await waitFor(() => screen.getByText("f1.js").closest(".scm-row") as HTMLElement);
    fireEvent.click(within(kid).getByRole("button", { name: "暂存更改" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_stage", { cwd: "C:/w/meowo", paths: ["out/d0/f1.js"] }));
    // 写操作后点开的目录重拉一次（文件可能刚被暂存走）。
    await waitFor(() => expect(calls("git_untracked_dir_files")).toHaveLength(2));
    const again = await waitFor(() => screen.getByText("f2.js").closest(".scm-row") as HTMLElement);
    fireEvent.click(within(again).getByRole("button", { name: "放弃更改" }));
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByText("out/d0/f2.js")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "放弃更改" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_discard", { cwd: "C:/w/meowo", paths: ["out/d0/f2.js"] }));
    fireEvent.click(await waitFor(() => screen.getByText("f0.js")));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_file_diff", { cwd: "C:/w/meowo", path: "out/d0/f0.js", untracked: true }));
  });

  it("整目录暂存与放弃：确认框按文件数说、写明整个目录进回收站", async () => {
    renderScm([repo([folded("target-x/", 15000)])]);
    fireEvent.click(within(folderRow("target-x/")).getByRole("button", { name: "暂存更改" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_stage", { cwd: "C:/w/meowo", paths: ["target-x/"] }));
    fireEvent.click(within(folderRow("target-x/")).getByRole("button", { name: "放弃更改" }));
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByText("放弃 15000 个文件的更改？")).toBeTruthy();
    expect(within(dialog).getByText("目录整个移到回收站，含其中被忽略的文件。")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "放弃更改" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_discard", { cwd: "C:/w/meowo", paths: ["target-x/"] }));
  });

  it("目录里有链接时放弃被拒，提示看得懂", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "git_discard") throw "git/dir_has_links";
      return [];
    });
    renderScm([repo([folded("target-x/", 60)])]);
    fireEvent.click(within(folderRow("target-x/")).getByRole("button", { name: "放弃更改" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "放弃更改" }));
    expect((await screen.findByRole("alert")).textContent).toBe("目录里有链接，没有放弃");
  });

  it("加入 .gitignore：调命令、刷新后目录从列表消失", async () => {
    const { onChanged, rerender } = renderScm([repo([work("a.ts"), folded("target-x/", 15000)])]);
    fireEvent.click(within(folderRow("target-x/")).getByRole("button", { name: "加入 .gitignore" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_ignore_dir", { cwd: "C:/w/meowo", path: "target-x/" }));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    rerender([repo([work("a.ts"), work(".gitignore")])]);
    expect(screen.queryByText("target-x/")).toBeNull();
  });

  it("手机端没有「加入 .gitignore」，暂存 / 放弃照常", () => {
    remote = true;
    renderScm([repo([folded("target-x/", 15000)])]);
    const row = folderRow("target-x/");
    expect(within(row).queryByRole("button", { name: "加入 .gitignore" })).toBeNull();
    expect(within(row).getByRole("button", { name: "暂存更改" })).toBeTruthy();
    expect(within(row).getByRole("button", { name: "放弃更改" })).toBeTruthy();
  });

  it("树形：折叠目录是一片叶子，点开同样列文件", async () => {
    dirListing = kids("app/target-x/", 2);
    renderScm([repo([work("app/src/a.ts"), folded("app/target-x/", 70)])]);
    fireEvent.click(screen.getByRole("button", { name: "树形" }));
    const row = folderRow("target-x/");
    expect(row.textContent).toContain("· 70 个文件");
    expect(row.querySelector(".scm-name")?.textContent).toBe("target-x/");
    fireEvent.click(within(row).getByRole("button", { name: /target-x\// }));
    await waitFor(() => expect(screen.getByText("f1.js")).toBeTruthy());
  });

  it("搜索命中折叠目录里点开的文件时，目录行留着、只显示命中的", async () => {
    dirListing = [work("out/keep.js", "U"), work("out/drop.js", "U")];
    renderScm([repo([work("a.ts"), folded("out/", 60)])]);
    fireEvent.click(within(folderRow("out/")).getByRole("button", { name: /out\// }));
    await waitFor(() => expect(screen.getByText("keep.js")).toBeTruthy());
    fireEvent.change(screen.getByPlaceholderText("按名称搜索文件…"), { target: { value: "keep" } });
    expect(screen.getByText("keep.js")).toBeTruthy();
    expect(screen.queryByText("drop.js")).toBeNull();
    expect(screen.queryByText("a.ts")).toBeNull();
    expect(screen.getByText("out/")).toBeTruthy();
  });
});

describe("虚拟列表", () => {
  const many = Array.from({ length: 3000 }, (_, i) => work(`src/m${String(i).padStart(4, "0")}/file${i}.ts`));
  const scrollToBottom = (container: HTMLElement) => {
    const scroller = container.querySelector(".scm") as HTMLElement;
    act(() => {
      scroller.scrollTop = 3000 * 28 * 2;
      fireEvent.scroll(scroller);
    });
  };

  it("平铺：3000 条变更只挂可视区附近的行，滚到底能看到最后一条", async () => {
    const { container } = renderScm([repo(many)]);
    const mounted = container.querySelectorAll('[data-testid="scm-row-work"]').length;
    expect(mounted).toBeGreaterThan(0);
    expect(mounted).toBeLessThan(200);
    expect(screen.getByText("file0.ts")).toBeTruthy();
    expect(screen.queryByText("file2999.ts")).toBeNull();
    scrollToBottom(container);
    await waitFor(() => expect(screen.getByText("file2999.ts")).toBeTruthy());
    expect(container.querySelectorAll('[data-testid="scm-row-work"]').length).toBeLessThan(200);
    // 行上的操作照常：最后一条也能暂存。
    fireEvent.click(within(screen.getByText("file2999.ts").closest(".scm-row") as HTMLElement).getByRole("button", { name: "暂存更改" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_stage", { cwd: "C:/w/meowo", paths: ["src/m2999/file2999.ts"] }));
  });

  it("树形：同样只挂一部分，滚到底能看到最后一个目录里的文件", async () => {
    const { container } = renderScm([repo(many)]);
    fireEvent.click(screen.getByRole("button", { name: "树形" }));
    expect(container.querySelectorAll(".scm-row").length).toBeLessThan(200);
    scrollToBottom(container);
    await waitFor(() => expect(screen.getByText("file2999.ts")).toBeTruthy());
  });

  it("点开一个一万五千个文件的折叠目录：同样只挂一部分", async () => {
    dirListing = kids("target-x/", 15000);
    const { container } = renderScm([repo([folded("target-x/", 15000)])]);
    fireEvent.click(within(folderRow("target-x/")).getByRole("button", { name: /target-x\// }));
    await waitFor(() => expect(screen.getByText("f0.js")).toBeTruthy());
    expect(container.querySelectorAll(".scm-row").length).toBeLessThan(200);
    act(() => {
      const scroller = container.querySelector(".scm") as HTMLElement;
      scroller.scrollTop = 15001 * 28;
      fireEvent.scroll(scroller);
    });
    await waitFor(() => expect(screen.getByText("f14999.js")).toBeTruthy());
  });

  it("右键菜单与选中态在虚拟行上照常", async () => {
    const { container } = renderScm([repo(many)]);
    const row = screen.getByText("file3.ts").closest(".scm-row") as HTMLElement;
    fireEvent.contextMenu(within(row).getByRole("button", { name: /file3\.ts/ }));
    expect(container.ownerDocument.querySelector('[role="menu"]')).toBeTruthy();
  });
});

// 做空第 3 轮复现：筛选下折叠目录只露出一部分，「提交全部」和整组操作却把整个目录交给后端，
// 目录里看不见的文件被一起提交或放弃（绕过了「筛选藏起任何一个就不许提交」）。
describe("筛选下只露出一部分的折叠目录", () => {
  const renderWithTouched = (repos: RepoChangesDto[], touched: string[]) => render(
    <SourceControl cwd="C:/w/meowo" selectedRoot={null} onSelectRoot={() => {}} onChanged={vi.fn()} comments={[]}
      touchedPaths={touched} turnTouchedPaths={touched} onAddComment={() => {}} onRemoveComment={() => {}}
      escOverrideRef={{ current: null }} repos={repos} />,
  );
  const commitButton = () => [...document.querySelectorAll("button")].find((b) => /提交全部/.test(b.textContent ?? "")) as HTMLButtonElement;
  const openOut = async () => {
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: /out\// }));
    await waitFor(() => expect(screen.getByText("keep.js")).toBeTruthy());
  };
  beforeEach(() => { dirListing = [work("out/keep.js", "U"), work("out/drop.js", "U")]; });

  it("搜索只命中目录里的一个文件：不许「提交全部」", async () => {
    renderWithTouched([repo([folded("out/", 60)])], []);
    await openOut();
    fireEvent.change(screen.getByPlaceholderText("按名称搜索文件…"), { target: { value: "keep" } });
    fireEvent.change(document.querySelector("textarea") as HTMLTextAreaElement, { target: { value: "msg" } });
    expect(commitButton().disabled).toBe(true);
    fireEvent.click(commitButton());
    expect(calls("git_commit")).toHaveLength(0);
  });

  it("搜索只命中目录里的一个文件：「全部放弃」只交看得见的文件，不交整个目录", async () => {
    renderWithTouched([repo([work("a.ts"), folded("out/", 60)])], []);
    await openOut();
    fireEvent.change(screen.getByPlaceholderText("按名称搜索文件…"), { target: { value: "keep" } });
    const head = screen.getByTestId("scm-group-work").querySelector(".scm-group-head") as HTMLElement;
    fireEvent.click(within(head).getByRole("button", { name: "全部放弃" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "放弃更改" }));
    await waitFor(() => expect(calls("git_discard")).toHaveLength(1));
    expect((calls("git_discard")[0][1] as { paths: string[] }).paths).toEqual(["out/keep.js"]);
  });

  it("「本会话」筛选、会话只写过目录里的一个文件：不许「提交全部」", async () => {
    renderWithTouched([repo([folded("out/", 60)])], ["C:/w/meowo/out/keep.js"]);
    fireEvent.click([...document.querySelectorAll("button")].find((b) => /^本会话/.test(b.textContent ?? "")) as HTMLButtonElement);
    fireEvent.change(document.querySelector("textarea") as HTMLTextAreaElement, { target: { value: "msg" } });
    expect(commitButton().disabled).toBe(true);
    fireEvent.click(commitButton());
    expect(calls("git_commit")).toHaveLength(0);
  });

  it("没有筛选时，折叠目录照常整体提交", async () => {
    renderWithTouched([repo([folded("out/", 60)])], []);
    fireEvent.change(document.querySelector("textarea") as HTMLTextAreaElement, { target: { value: "msg" } });
    expect(commitButton().disabled).toBe(false);
    fireEvent.click(commitButton());
    await waitFor(() => expect(calls("git_commit")).toHaveLength(1));
    expect((calls("git_commit")[0][1] as { paths: string[] }).paths).toEqual(["out/"]);
  });
});
