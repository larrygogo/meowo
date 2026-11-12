// diff 行评论草稿不丢（walkthrough P-6）、「+」只占一个 Tab 停靠点（P-9）、打开文件后焦点接到「返回」（P-3），
// 文件树方向键（P-12）。2026-11-12，复现原文见 research-walkthrough.md wk3-diff-cmt / wk3d-files。
// 兄弟场景：草稿遇到换行、Esc、换文件、离开 diff 页（卸载）都存成评论；只有「取消」丢。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import type { GitDiffSummaryDto } from "../../api";
import { ChangesView, FilesView, GitDiffView } from "./GitDiffView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}));

const diffOf = (path: string) => [`diff --git a/${path} b/${path}`, `--- a/${path}`, `+++ b/${path}`, "@@ -10,2 +10,2 @@", " keep", "-old", "+new"].join("\n");
const summary: GitDiffSummaryDto = {
  gitAvailable: true, isRepo: true, branch: "main", ahead: null, behind: null,
  files: [{ path: "src/a.ts", status: "M", additions: 1, deletions: 1 }, { path: "src/b.ts", status: "M", additions: 1, deletions: 1 }],
};
const PLACEHOLDER = "写下对这一行的意见，Enter 添加";

afterEach(cleanup);

function renderPanel() {
  vi.mocked(invoke).mockImplementation(async (command: string, args?: unknown) => {
    if (command === "git_file_diff") {
      const path = (args as { path: string }).path;
      return { path, status: "M", diff: diffOf(path), truncated: false };
    }
    return [];
  });
  const onAddComment = vi.fn();
  const utils = render(
    <GitDiffView cwd="C:/repo" summary={summary} width={420} collapsed={false} maximized={false} onCollapse={() => {}} onToggleMaximize={() => {}}
      comments={[]} onAddComment={onAddComment} onRemoveComment={() => {}} touchedPaths={[]} />,
  );
  return { ...utils, onAddComment };
}

const openFile = async (name: RegExp) => {
  fireEvent.click(screen.getByRole("button", { name }));
  return screen.findAllByRole("button", { name: "评论这一行" });
};
const write = (text: string) => fireEvent.change(screen.getByPlaceholderText(PLACEHOLDER), { target: { value: text } });

describe("评论草稿不丢（P-6）", () => {
  it("写了一半去点另一行的「+」：前一条存成评论，新框是空的", async () => {
    const { onAddComment } = renderPanel();
    const adds = await openFile(/a\.ts/);
    fireEvent.click(adds[0]);
    write("这里命名再想想");
    fireEvent.click(adds[2]);
    expect(onAddComment).toHaveBeenCalledWith({ path: "src/a.ts", side: "new", line: 10, code: "keep", text: "这里命名再想想" }, "C:/repo");
    expect((screen.getByPlaceholderText(PLACEHOLDER) as HTMLTextAreaElement).value).toBe("");
  });

  it("Esc：草稿存成评论再关框；空草稿直接关", async () => {
    const { onAddComment } = renderPanel();
    const adds = await openFile(/a\.ts/);
    fireEvent.click(adds[1]);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onAddComment).not.toHaveBeenCalled();
    fireEvent.click(adds[1]);
    write("别删这行");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByPlaceholderText(PLACEHOLDER)).toBeNull();
    expect(onAddComment).toHaveBeenCalledWith(expect.objectContaining({ path: "src/a.ts", line: 11, text: "别删这行" }), "C:/repo");
  });

  it("点「返回」离开 diff 页：草稿存下", async () => {
    const { onAddComment } = renderPanel();
    const adds = await openFile(/a\.ts/);
    fireEvent.click(adds[1]);
    write("回头再看");
    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    expect(onAddComment).toHaveBeenCalledWith(expect.objectContaining({ path: "src/a.ts", text: "回头再看" }), "C:/repo");
  });

  it("换文件（外层把另一个文件交进来）：草稿记在原来那个文件上", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string, args?: unknown) => {
      if (command !== "git_file_diff") return [];
      const path = (args as { path: string }).path;
      return { path, status: "M", diff: diffOf(path), truncated: false };
    });
    const onAddComment = vi.fn();
    const view = (path: string) => (
      <ChangesView key="one" cwd="C:/repo" summary={summary} comments={[]} touchedPaths={[]} turnTouchedPaths={[]} initialPath={path}
        onAddComment={onAddComment} onRemoveComment={() => {}} escOverrideRef={{ current: null }} />
    );
    render(view("src/a.ts"));
    fireEvent.click((await screen.findAllByRole("button", { name: "评论这一行" }))[1]);
    write("a 的意见");
    // 文件页里的「打开」菜单切到同仓库别的文件时，selectedPath 变化：用列表切文件模拟。
    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    fireEvent.click(screen.getByRole("button", { name: /b\.ts/ }));
    await screen.findAllByRole("button", { name: "评论这一行" });
    expect(onAddComment).toHaveBeenCalledWith(expect.objectContaining({ path: "src/a.ts", text: "a 的意见" }));
    expect(onAddComment).toHaveBeenCalledTimes(1);
  });

  it("只有「取消」丢草稿", async () => {
    const { onAddComment } = renderPanel();
    const adds = await openFile(/a\.ts/);
    fireEvent.click(adds[1]);
    write("不要了");
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    expect(onAddComment).not.toHaveBeenCalled();
  });
});

describe("diff 里的「+」（P-9）", () => {
  it("只有一个 Tab 停靠点，↑↓ 在各行之间移动", async () => {
    renderPanel();
    const adds = await openFile(/a\.ts/);
    expect(adds.map((button) => button.tabIndex)).toEqual([0, -1, -1]);
    adds[0].focus();
    fireEvent.keyDown(adds[0], { key: "ArrowDown" });
    expect(document.activeElement).toBe(adds[1]);
    expect(adds.map((button) => button.tabIndex)).toEqual([-1, 0, -1]);
    fireEvent.keyDown(adds[1], { key: "ArrowUp" });
    expect(document.activeElement).toBe(adds[0]);
  });
});

describe("打开文件后焦点（P-3 / P-12）", () => {
  it("点开文件、列表被换掉焦点掉到 body：接到「返回」上", async () => {
    renderPanel();
    const row = screen.getByRole("button", { name: /a\.ts/ });
    row.focus();
    fireEvent.click(row);
    await screen.findAllByRole("button", { name: "评论这一行" });
    await act(async () => { await new Promise((resolve) => requestAnimationFrame(() => resolve(null))); });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "返回" }));
  });

  it("「文件」页签的树：↓ 移到下一行，→ 展开目录、← 收起；返回后焦点回到打开的那一行", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string, args?: unknown) => {
      if (command === "list_dir_entries") {
        const rel = (args as { rel: string }).rel;
        if (rel === "") return [{ name: "src", relPath: "src", isDir: true }, { name: "package.json", relPath: "package.json", isDir: false }];
        return [{ name: "a.ts", relPath: "src/a.ts", isDir: false }];
      }
      if (command === "read_file_text") return { relPath: (args as { rel: string }).rel, text: "x", truncated: false, binary: false };
      return [];
    });
    render(<FilesView cwd="C:/repo" />);
    const dir = await screen.findByRole("button", { name: "src" });
    dir.focus();
    fireEvent.keyDown(dir, { key: "ArrowDown" });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "package.json" }));
    fireEvent.keyDown(document.activeElement!, { key: "ArrowUp" });
    expect(document.activeElement).toBe(dir);
    expect(dir.getAttribute("aria-expanded")).toBe("false");
    fireEvent.keyDown(dir, { key: "ArrowRight" });
    await waitFor(() => expect(dir.getAttribute("aria-expanded")).toBe("true"));
    const child = await screen.findByRole("button", { name: "a.ts" });
    child.focus();
    fireEvent.keyDown(child, { key: "ArrowLeft" });
    expect(document.activeElement).toBe(dir);
    fireEvent.keyDown(dir, { key: "ArrowLeft" });
    expect(dir.getAttribute("aria-expanded")).toBe("false");
    const pkg = screen.getByRole("button", { name: "package.json" });
    pkg.focus();
    fireEvent.click(pkg);
    const back = await screen.findByRole("button", { name: "返回" });
    (document.activeElement as HTMLElement | null)?.blur();
    fireEvent.click(back);
    await act(async () => { await new Promise((resolve) => requestAnimationFrame(() => resolve(null))); });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "package.json" }));
  });
});
