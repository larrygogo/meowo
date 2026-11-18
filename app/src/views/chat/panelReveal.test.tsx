import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve([])) }));
vi.mock("./ShellTerminal", () => ({ ShellTerminal: () => null }));
vi.mock("./BrowserPane", () => ({ BrowserPane: () => null }));
// 把单文件 diff / 文件页收到的参数露出来：这里只关心面板把哪个文件交给了它们。
vi.mock("./lazyDiff", () => ({
  LazyChangesView: ({ cwd, initialPath, diffBase }: { cwd: string; initialPath: string | null; diffBase: string | null }) => (
    <div data-testid="diff" data-cwd={cwd} data-path={initialPath ?? ""} data-base={diffBase ?? ""} />
  ),
  LazyFilesView: ({ cwd, reveal, onRevealed }: { cwd: string; reveal: { rel: string; nonce: number } | null; onRevealed?: (nonce: number) => void }) => (
    <div data-testid="files" data-cwd={cwd} data-rel={reveal?.rel ?? ""}>
      {reveal && <button type="button" onClick={() => onRevealed?.(reveal.nonce)}>opened</button>}
    </div>
  ),
}));

import type { ChatItem, GitDiffSummaryDto, RepoChangesDto } from "../../api";
import { ChatCwdContext } from "./ToolActivity";
import { absolutePath, filesTarget, locateChange, RevealChangeContext } from "./panelReveal";
import { consumeReveal, EMPTY_PANEL, loadPanel, revealIn, savePanel, type PanelState } from "./panelTabs";
import { SidePanel } from "./SidePanel";
import { TurnChanges } from "./TurnChanges";
import { EditTool } from "./tools/EditTool";
import type { PanelActions } from "./usePanel";
import { zh } from "../../i18n/zh";

afterEach(cleanup);

const file = (path: string, extra: Record<string, unknown> = {}) => ({ path, status: "M", additions: 1, deletions: 0, indexStatus: null, worktreeStatus: "M", ...extra });
const summary = (files: ReturnType<typeof file>[]): GitDiffSummaryDto => ({ gitAvailable: true, isRepo: true, branch: "main", ahead: null, behind: null, files });
const repo = (root: string, files: ReturnType<typeof file>[], extra: Partial<RepoChangesDto> = {}): RepoChangesDto =>
  ({ root, name: root.split("/").pop()!, isCwdRepo: false, summary: summary(files), base: null, baseLabel: null, committed: [], ...extra });

describe("对话里的路径换算到「更改」面板（locateChange / filesTarget）", () => {
  const repos = [
    repo("C:/repo", [file("src/a.ts"), file("src/staged.ts", { indexStatus: "M", worktreeStatus: null }), file("out/", { status: "U", worktreeStatus: "U", untrackedFiles: 40 })],
      { isCwdRepo: true, base: "abc", committed: [file("src/done.ts")] }),
    repo("C:/repo/.wt/feat", [file("lib/x.ts")]),
  ];

  it("按仓库根最长匹配，大小写与斜杠不敏感；分清未暂存、暂存、本分支已提交", () => {
    expect(locateChange(repos, "C:\\Repo\\src\\a.ts")).toMatchObject({ found: true, root: "C:/repo", group: "work", rel: "src/a.ts" });
    expect(locateChange(repos, "C:/repo/src/staged.ts")).toMatchObject({ found: true, group: "staged", rel: "src/staged.ts" });
    expect(locateChange(repos, "C:/repo/src/done.ts")).toMatchObject({ found: true, group: "committed" });
    // worktree 嵌在主仓库目录里：归到更深的那个仓库。
    expect(locateChange(repos, "C:/repo/.wt/feat/lib/x.ts")).toMatchObject({ found: true, root: "C:/repo/.wt/feat", rel: "lib/x.ts" });
  });

  it("折叠的未跟踪目录里的文件补成一条未跟踪文件", () => {
    expect(locateChange(repos, "C:/repo/out/deep/b.js")).toMatchObject({ found: true, group: "work", rel: "out/deep/b.js", file: { status: "U", worktreeStatus: "U" } });
  });

  it("仓库里没有改动、或不在任何仓库里时报告没找到，并说明在哪个仓库", () => {
    expect(locateChange(repos, "C:/repo/README.md")).toEqual({ found: false, root: "C:/repo" });
    expect(locateChange(repos, "D:/elsewhere/x.ts")).toEqual({ found: false, root: null });
  });

  it("「文件」页的退路：会话目录下、或别的仓库（worktree）里；都不在就没有", () => {
    expect(filesTarget("C:/repo/README.md", "C:\\repo", repos)).toEqual({ root: "C:\\repo", rel: "README.md" });
    expect(filesTarget("C:/w/feat/a.ts", "C:/repo", [repo("C:/w/feat", [])])).toEqual({ root: "C:/w/feat", rel: "a.ts" });
    expect(filesTarget("D:/elsewhere/x.ts", "C:/repo", repos)).toBeNull();
  });

  it("相对路径按会话目录补全", () => {
    expect(absolutePath("src\\a.ts", "C:\\repo\\")).toBe("C:/repo/src/a.ts");
    expect(absolutePath("./a.ts", "/home/u/p")).toBe("/home/u/p/a.ts");
    expect(absolutePath("C:\\x\\a.ts", "C:/repo")).toBe("C:/x/a.ts");
  });
});

describe("面板状态（revealIn / consumeReveal）", () => {
  it("收起的面板：点了以后展开、切到「更改」页签并带上目标文件；不进 localStorage", () => {
    const collapsed: PanelState = { tabs: [{ key: "t", type: "term", shellId: null, cwd: "C:/repo" }], active: "t", visible: false, expanded: false };
    const next = revealIn(collapsed, "diff", "C:/repo/src/a.ts");
    expect(next.visible).toBe(true);
    expect(next.tabs.find((tab) => tab.key === next.active)?.type).toBe("diff");
    expect(next.reveal).toMatchObject({ tab: "diff", path: "C:/repo/src/a.ts" });
    // 已经开着「更改」：切过去，不再多开一个。
    const again = revealIn(next, "diff", "C:/repo/b.ts");
    expect(again.tabs.filter((tab) => tab.type === "diff")).toHaveLength(1);
    expect(again.reveal!.nonce).not.toBe(next.reveal!.nonce);
    // 只清自己处理的那一次请求。
    expect(consumeReveal(again, next.reveal!.nonce)).toBe(again);
    expect(consumeReveal(again, again.reveal!.nonce).reveal).toBeNull();

    const store = new Map<string, string>();
    const storage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v), removeItem: (k: string) => void store.delete(k), key: (i: number) => [...store.keys()][i] ?? null, get length() { return store.size; }, clear: () => store.clear() } as Storage;
    savePanel(7, again, storage);
    expect([...store.values()][0]).not.toContain("reveal");
    expect(loadPanel(7, storage).reveal).toBeUndefined();
    expect(revealIn(EMPTY_PANEL, "files", "x").tabs[0].type).toBe("files");
  });
});

function edit(id: string, path: string): ChatItem[] {
  return [
    { type: "tool_use", id, timestamp: null, name: "Edit", kind: "edit", summary: path, detail: { kind: "edit", path, old: "a", new: "b", replace_all: false, truncated: false } },
    { type: "tool_result", id: `r-${id}`, timestamp: null, tool_use_id: id, text: "ok", is_error: false },
  ];
}

describe("对话里的入口", () => {
  it("改动卡：行尾按钮把原路径交给面板，点行照旧展开修改记录", () => {
    const reveal = vi.fn();
    render(
      <ChatCwdContext.Provider value="C:/repo">
        <RevealChangeContext.Provider value={reveal}><TurnChanges items={edit("e1", "C:/repo/src/a.ts")} /></RevealChangeContext.Provider>
      </ChatCwdContext.Provider>,
    );
    const open = screen.getByRole("button", { name: `${zh.chat.revealInChanges} src/a.ts` });
    expect(open.getAttribute("data-tip")).toBe(zh.chat.revealInChanges);
    fireEvent.click(open);
    expect(reveal).toHaveBeenCalledWith("C:/repo/src/a.ts");
    // 点行本身仍是展开 / 收起，不跳面板。
    const row = document.querySelector(".chat-turn-changes-row")!;
    fireEvent.click(row);
    expect(row.getAttribute("aria-expanded")).toBe("true");
    expect(reveal).toHaveBeenCalledTimes(1);
  });

  it("工具行展开卡头的路径是按钮，点了交给面板", () => {
    const reveal = vi.fn();
    const [use, result] = edit("e2", "C:/repo/src/b.ts");
    render(
      <ChatCwdContext.Provider value="C:/repo">
        <RevealChangeContext.Provider value={reveal}>
          <EditTool item={use as never} result={result as never} />
        </RevealChangeContext.Provider>
      </ChatCwdContext.Provider>,
    );
    fireEvent.click(screen.getByRole("button", { name: `${zh.chat.revealInChanges} src/ b.ts` }));
    expect(reveal).toHaveBeenCalledWith("C:/repo/src/b.ts");
  });

  it("手机远程页（不提供面板入口）：改动卡和工具行都不出可点的入口", () => {
    const [use, result] = edit("e3", "C:/repo/src/c.ts");
    render(
      <ChatCwdContext.Provider value="C:/repo">
        <TurnChanges items={edit("e4", "C:/repo/src/d.ts")} />
        <EditTool item={use as never} result={result as never} />
      </ChatCwdContext.Provider>,
    );
    expect(screen.queryByRole("button", { name: new RegExp(zh.chat.revealInChanges) })).toBeNull();
    expect(document.querySelector(".chat-turn-changes-open")).toBeNull();
    expect(document.querySelector(".chat-edit-card-path.is-link")).toBeNull();
    expect(document.querySelector(".chat-edit-card-path")?.textContent).toBe("src/ c.ts");
  });
});

describe("面板处理定位请求", () => {
  function actionsMock() {
    return {
      activate: vi.fn(), move: vi.fn(), close: vi.fn(), openDiff: vi.fn(), openFiles: vi.fn(), newTerminal: vi.fn(), restartTerminal: vi.fn(), newBrowser: vi.fn(),
      show: vi.fn(), hide: vi.fn(), toggle: vi.fn(), toggleExpanded: vi.fn(), cycle: vi.fn(), toggleFiles: vi.fn(), toggleTerminal: vi.fn(), openUrl: vi.fn(),
      revealDiff: vi.fn(), revealFile: vi.fn(), consumeReveal: vi.fn(),
    } satisfies PanelActions;
  }
  const repos = [
    repo("C:/repo", [file("src/a.ts")], { isCwdRepo: true }),
    repo("C:/repo-wt", [file("lib/x.ts")]),
  ];
  const base = { shells: new Map(), panes: new Map(), cwd: "C:/repo", summary: repos[0].summary, repos, width: 420, comments: [], onAddComment: () => {}, onRemoveComment: () => {},
    touchedPaths: [], turnTouchedPaths: [], browserSuggestions: [], onChanged: () => {} };
  const diffState = (path: string): PanelState => ({ tabs: [{ key: "d", type: "diff" }], active: "d", visible: true, expanded: false, reveal: { tab: "diff", path, nonce: 41 } });

  it("文件在别的仓库里有改动：切过去并打开它的 diff，处理完清掉请求", () => {
    const actions = actionsMock();
    const onSelectRoot = vi.fn();
    const { rerender } = render(<SidePanel {...base} actions={actions} state={diffState("C:/repo-wt/lib/x.ts")} selectedRoot={null} onSelectRoot={onSelectRoot} />);
    expect(onSelectRoot).toHaveBeenCalledWith("C:/repo-wt");
    expect(actions.consumeReveal).toHaveBeenCalledWith(41);
    // 上层换好选中的仓库、清掉请求后，显示的是这个文件的 diff。
    rerender(<SidePanel {...base} actions={actions} state={{ ...diffState("x"), reveal: null }} selectedRoot="C:/repo-wt" onSelectRoot={onSelectRoot} />);
    expect(screen.getByTestId("diff").dataset).toMatchObject({ cwd: "C:/repo-wt", path: "lib/x.ts" });
  });

  it("文件已经没有未提交的改动：留在「更改」列表并说明，给「在文件中打开」", () => {
    const actions = actionsMock();
    render(<SidePanel {...base} actions={actions} state={diffState("C:/repo/README.md")} selectedRoot={null} onSelectRoot={() => {}} />);
    expect(screen.queryByTestId("diff")).toBeNull();
    expect(screen.getByTestId("scm-notice").textContent).toContain(zh.chat.revealNoChange);
    fireEvent.click(screen.getByRole("button", { name: zh.chat.revealOpenInFiles }));
    expect(actions.revealFile).toHaveBeenCalledWith("C:/repo/README.md");
  });

  it("不在任何仓库、也不在会话目录下：说明原因，不给打不开的按钮", () => {
    render(<SidePanel {...base} actions={actionsMock()} state={diffState("D:/tmp/x.ts")} selectedRoot={null} onSelectRoot={() => {}} />);
    expect(screen.getByTestId("scm-notice").textContent).toContain(zh.chat.revealNoChangeOutside);
    expect(screen.queryByRole("button", { name: zh.chat.revealOpenInFiles })).toBeNull();
  });

  it("退到「文件」页：交给文件页打开相对路径", () => {
    const actions = actionsMock();
    const state: PanelState = { tabs: [{ key: "f", type: "files" }], active: "f", visible: true, expanded: false, reveal: { tab: "files", path: "C:/repo/docs/README.md", nonce: 9 } };
    render(<SidePanel {...base} actions={actions} state={state} selectedRoot={null} onSelectRoot={() => {}} />);
    expect(screen.getByTestId("files").dataset).toMatchObject({ cwd: "C:/repo", rel: "docs/README.md" });
    fireEvent.click(screen.getByRole("button", { name: "opened" }));
    expect(actions.consumeReveal).toHaveBeenCalledWith(9);
  });
});
