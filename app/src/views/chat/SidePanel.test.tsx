import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve([])) }));
// xterm 和原生 webview 在 jsdom 里都起不来；这里只关心页签怎么排、怎么切、怎么关。
vi.mock("./ShellTerminal", () => ({
  ShellTerminal: ({ id, visible }: { id: number; visible: boolean }) => (
    <div className="shell-term" data-testid={`shell-${id}`} data-visible={String(visible)}><textarea aria-label={`shell-input-${id}`} /></div>
  ),
}));
vi.mock("./BrowserPane", () => ({
  BrowserPane: ({ info, visible }: { info: { id: number }; visible: boolean }) => <div data-testid={`web-${info.id}`} data-visible={String(visible)} />,
}));
// SidePanel / SourceControl 经 lazyDiff 按需取 GitDiffView 的两个大组件（拆 chunk，见 lazyDiff.tsx）。
vi.mock("./lazyDiff", () => ({
  LazyChangesView: () => <div>改动内容</div>,
  LazyFilesView: ({ cwd }: { cwd: string }) => <div data-testid="files" data-cwd={cwd}>文件内容</div>,
}));

import type { BrowserPaneInfo, GitDiffSummaryDto, ShellTerminalInfo } from "../../api";
import type { PanelState } from "./panelTabs";
import { SidePanel } from "./SidePanel";
import type { PanelActions } from "./usePanel";

afterEach(cleanup);

const summary: GitDiffSummaryDto = { gitAvailable: true, isRepo: true, branch: "main", ahead: null, behind: null, files: [] };
const shells = new Map<number, ShellTerminalInfo>([[3, { id: 3, owner: 1, title: "pwsh", exited: false }]]);
const panes = new Map<number, BrowserPaneInfo>([[5, { id: 5, owner: 1, url: "http://localhost:5173/", title: "Vite App", loading: false, agentActive: true, agentPaused: false, zoom: 1, downloads: [], permission: null, blocked: null }]]);
const full: PanelState = {
  tabs: [
    { key: "d", type: "diff" },
    { key: "t", type: "term", shellId: 3, cwd: "C:\\repo" },
    { key: "w", type: "browser", paneId: 5, url: "http://localhost:5173/", title: "" },
  ],
  active: "t",
  visible: true,
  expanded: false,
};

function renderPanel(state: PanelState = full) {
  const actions = {
    activate: vi.fn(), move: vi.fn(), close: vi.fn(), openDiff: vi.fn(), openFiles: vi.fn(), newTerminal: vi.fn(), newBrowser: vi.fn(),
    show: vi.fn(), hide: vi.fn(), toggle: vi.fn(), toggleExpanded: vi.fn(), cycle: vi.fn(), toggleFiles: vi.fn(), toggleTerminal: vi.fn(), openUrl: vi.fn(),
  } satisfies PanelActions;
  render(
    <SidePanel
      state={state} actions={actions} shells={shells} panes={panes} cwd={"C:\\repo"} summary={summary} width={420}
      comments={[]} onAddComment={() => {}} onRemoveComment={() => {}} touchedPaths={[]} turnTouchedPaths={[]} browserSuggestions={[]}
      selectedRoot={null} onSelectRoot={() => {}} onChanged={() => {}}
    />,
  );
  return actions;
}

describe("右侧面板", () => {
  it("改动、终端、浏览器同一排页签；终端和浏览器常驻挂载只切显隐", () => {
    renderPanel();
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((tab) => tab.textContent)).toEqual(["更改", "pwsh", "Vite App"]);
    expect(tabs[1].getAttribute("aria-selected")).toBe("true");
    expect(screen.getByTestId("shell-3").dataset.visible).toBe("true");
    expect(screen.getByTestId("web-5").dataset.visible).toBe("false");
    expect(screen.queryByText("改动内容")).toBeNull();
    // agent 正在操作的浏览器页签上亮一个点。
    expect(tabs[2].querySelector(".side-panel-tab-agent")).not.toBeNull();
    expect(tabs[1].querySelector(".side-panel-tab-agent")).toBeNull();
  });

  it("页签里的网页请求权限时，页签上换成琥珀点（页签藏着也看得到）", () => {
    panes.set(5, { ...panes.get(5)!, permission: { id: 1, kind: "downloads", host: "httpbin.org" } });
    try {
      renderPanel();
      const tab = screen.getAllByRole("tab")[2];
      expect(tab.querySelector(".side-panel-tab-waiting")?.getAttribute("aria-label")).toBe("网页请求权限");
      expect(tab.querySelector(".side-panel-tab-agent")).toBeNull();
    } finally {
      panes.set(5, { ...panes.get(5)!, permission: null });
    }
  });

  it("点页签切换，叉和中键关闭，键盘 → 切到下一个、Alt+→ 挪位置", () => {
    const actions = renderPanel();
    const tabs = screen.getAllByRole("tab");
    fireEvent.click(tabs[0]);
    expect(actions.activate).toHaveBeenCalledWith("d");
    fireEvent.click(screen.getAllByLabelText("关闭")[2]);
    expect(actions.close).toHaveBeenCalledWith("w");
    fireEvent(tabs[1], new MouseEvent("auxclick", { bubbles: true, button: 1 }));
    expect(actions.close).toHaveBeenCalledWith("t");
    fireEvent.keyDown(tabs[1], { key: "ArrowRight" });
    expect(actions.activate).toHaveBeenCalledWith("w");
    fireEvent.keyDown(tabs[1], { key: "ArrowRight", altKey: true });
    expect(actions.move).toHaveBeenCalledWith("t", 2);
  });

  it("右键菜单：关闭其他 / 关闭右侧 / 关闭所有", () => {
    const actions = renderPanel();
    fireEvent.contextMenu(screen.getAllByRole("tab")[0], { clientX: 10, clientY: 10 });
    fireEvent.click(screen.getByRole("menuitem", { name: "关闭右侧" }));
    expect(actions.close).toHaveBeenCalledWith("d", "right");
  });

  it("「+」菜单不再列已经开着的改动；文件、浏览器、终端都在，带快捷键提示", () => {
    const actions = renderPanel();
    fireEvent.click(screen.getByRole("button", { name: "新建页签" }));
    const items = screen.getAllByRole("menuitem").map((item) => item.textContent);
    expect(items).toEqual(["新建浏览器Ctrl+T", "新建终端Ctrl+`", "文件Ctrl+Shift+E"]);
    fireEvent.click(screen.getByRole("menuitem", { name: /文件/ }));
    expect(actions.openFiles).toHaveBeenCalledTimes(1);
  });

  /// 做空复现：工作目录不是仓库（会话开在 workspace）时，「更改」入口此前全没了。
  it("工作目录不是仓库、但会话改过别的仓库时，启动列表里有「更改」", () => {
    const actions = {
      activate: vi.fn(), move: vi.fn(), close: vi.fn(), openDiff: vi.fn(), openFiles: vi.fn(), newTerminal: vi.fn(), newBrowser: vi.fn(),
      show: vi.fn(), hide: vi.fn(), toggle: vi.fn(), toggleExpanded: vi.fn(), cycle: vi.fn(), toggleFiles: vi.fn(), toggleTerminal: vi.fn(), openUrl: vi.fn(),
    } satisfies PanelActions;
    render(
      <SidePanel
        state={{ tabs: [], active: null, visible: true, expanded: false }} actions={actions} shells={shells} panes={panes} cwd={"C:\\w"}
        summary={{ ...summary, isRepo: false, branch: null }} width={420}
        repos={[{ root: "C:/w/meowo-feat", name: "meowo-feat", isCwdRepo: false, summary, base: null, baseLabel: null, committed: [] }]}
        comments={[]} onAddComment={() => {}} onRemoveComment={() => {}} touchedPaths={[]} turnTouchedPaths={[]} browserSuggestions={[]}
      selectedRoot={null} onSelectRoot={() => {}} onChanged={() => {}}
      />,
    );
    expect(screen.getByRole("navigation", { name: "可打开" }).textContent).toContain("更改");
  });

  /// 用户要求（2026-11-09）：会话开在主目录、改动在 worktree 里时，「文件」跟着「更改」选中的仓库走。
  it("「文件」浏览「更改」里选中的 worktree；「更改」里点别的仓库会通知上层切换", () => {
    const actions = {
      activate: vi.fn(), move: vi.fn(), close: vi.fn(), openDiff: vi.fn(), openFiles: vi.fn(), newTerminal: vi.fn(), newBrowser: vi.fn(),
      show: vi.fn(), hide: vi.fn(), toggle: vi.fn(), toggleExpanded: vi.fn(), cycle: vi.fn(), toggleFiles: vi.fn(), toggleTerminal: vi.fn(), openUrl: vi.fn(),
    } satisfies PanelActions;
    const repos = [
      { root: "C:/repo", name: "repo", isCwdRepo: true, summary, base: null, baseLabel: null, committed: [] },
      { root: "C:/repo-wt", name: "repo-wt", isCwdRepo: false, summary, base: null, baseLabel: null, committed: [] },
    ];
    const onSelectRoot = vi.fn();
    const props = { actions, shells, panes, cwd: "C:\\repo", summary, width: 420, repos, comments: [], onAddComment: () => {}, onRemoveComment: () => {},
      touchedPaths: [], turnTouchedPaths: [], browserSuggestions: [], onSelectRoot, onChanged: () => {} };
    const files: PanelState = { tabs: [{ key: "f", type: "files" }, { key: "d", type: "diff" }], active: "f", visible: true, expanded: false };
    const { rerender } = render(<SidePanel {...props} state={files} selectedRoot="C:/repo-wt" />);
    expect(screen.getByTestId("files").getAttribute("data-cwd")).toBe("C:/repo-wt");
    // 选回工作目录所在仓库：浏览工作目录本身（它可能是仓库的子目录）。
    rerender(<SidePanel {...props} state={files} selectedRoot="C:/repo" />);
    expect(screen.getByTestId("files").getAttribute("data-cwd")).toBe("C:\\repo");
    // 「文件」顶上可切：点 worktree 通知上层（与「更改」共用），点「工作目录」切回工作目录本身。
    fireEvent.click(screen.getByRole("button", { name: "repo-wt" }));
    expect(onSelectRoot).toHaveBeenLastCalledWith("C:/repo-wt");
    fireEvent.click(screen.getByRole("button", { name: "工作目录" }));
    expect(onSelectRoot).toHaveBeenLastCalledWith("C:\\repo");
    // 做空复现：工作目录不是仓库（会话开在 workspace）时，选过子仓库后也能回到工作目录本身。
    const notRepo = { ...props, cwd: "C:/w", summary: { ...summary, isRepo: false }, repos: [{ ...repos[1], root: "C:/w/projA", name: "projA" }] };
    rerender(<SidePanel {...notRepo} state={files} selectedRoot={null} />);
    expect(screen.getByTestId("files").getAttribute("data-cwd")).toBe("C:/w");
    rerender(<SidePanel {...notRepo} state={files} selectedRoot="C:/w/projA" />);
    expect(screen.getByTestId("files").getAttribute("data-cwd")).toBe("C:/w/projA");
    rerender(<SidePanel {...notRepo} state={files} selectedRoot="C:/w" />);
    expect(screen.getByTestId("files").getAttribute("data-cwd")).toBe("C:/w");
    rerender(<SidePanel {...props} state={{ ...files, active: "d" }} selectedRoot="C:/repo" />);
    fireEvent.click(screen.getByRole("button", { name: /repo-wt/ }));
    expect(onSelectRoot).toHaveBeenCalledWith("C:/repo-wt");
  });

  it("一个页签都没有时显示启动列表", () => {
    const actions = renderPanel({ tabs: [], active: null, visible: true, expanded: false });
    const launcher = screen.getByRole("navigation", { name: "可打开" });
    expect(launcher.textContent).toContain("更改");
    fireEvent.click(screen.getByRole("button", { name: /更改/ }));
    expect(actions.openDiff).toHaveBeenCalledTimes(1);
  });

  it("Esc：终端里交给 shell，菜单开着只关菜单，其余收起面板", () => {
    const actions = renderPanel();
    fireEvent.keyDown(screen.getByLabelText("shell-input-3"), { key: "Escape" });
    expect(actions.hide).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "新建页签" }));
    fireEvent.keyDown(screen.getAllByRole("menuitem")[0], { key: "Escape" });
    expect(actions.hide).not.toHaveBeenCalled();
    expect(screen.queryByRole("menu")).toBeNull();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(actions.hide).toHaveBeenCalledTimes(1);
  });

  it("页签放不下时：滚轮横向滚动，出现「全部页签」按钮可以跳到任意页签", () => {
    const widths = { scrollWidth: 600, clientWidth: 200 };
    const restore = (["scrollWidth", "clientWidth"] as const).map((key) => {
      const original = Object.getOwnPropertyDescriptor(HTMLElement.prototype, key);
      Object.defineProperty(HTMLElement.prototype, key, { configurable: true, get() { return this.classList?.contains("side-panel-tabs") ? widths[key] : 0; } });
      return () => original && Object.defineProperty(HTMLElement.prototype, key, original);
    });
    try {
      const actions = renderPanel();
      const bar = screen.getByRole("tablist");
      expect(bar.className).toContain("has-right");
      fireEvent.wheel(bar, { deltaY: 120 });
      expect(bar.scrollLeft).toBe(120);
      fireEvent.click(screen.getByRole("button", { name: "全部页签" }));
      expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["更改", "pwsh当前", "Vite App"]);
      fireEvent.click(screen.getByRole("menuitem", { name: /Vite App/ }));
      expect(actions.activate).toHaveBeenCalledWith("w");
    } finally {
      restore.forEach((undo) => undo());
    }
  });
});
