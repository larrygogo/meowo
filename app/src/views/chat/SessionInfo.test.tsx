import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
import type { GitDiffSummaryDto } from "../../api";
import type { PanelState, PanelTab } from "./panelTabs";
import { SessionInfo } from "./SessionInfo";
import type { PanelActions } from "./usePanel";

afterEach(cleanup);

const actions = () => ({
  activate: vi.fn(), move: vi.fn(), close: vi.fn(), openDiff: vi.fn(), openFiles: vi.fn(), newTerminal: vi.fn(), restartTerminal: vi.fn(), newBrowser: vi.fn(),
  show: vi.fn(), hide: vi.fn(), toggle: vi.fn(), toggleExpanded: vi.fn(), cycle: vi.fn(), toggleFiles: vi.fn(), toggleTerminal: vi.fn(), openUrl: vi.fn(),
}) satisfies PanelActions;
const summary: GitDiffSummaryDto = {
  gitAvailable: true, isRepo: true, branch: "main", ahead: null, behind: null,
  files: [{ path: "a.ts", status: "M", additions: 12, deletions: 3 }, { path: "b.ts", status: "M", additions: 1, deletions: 0 }],
};
const terms = (n: number): PanelTab[] => Array.from({ length: n }, (_, i) => ({ key: `t${i}`, type: "term", shellId: null, cwd: "C:/repo" }));
const state = (tabs: PanelTab[]): PanelState => ({ tabs, active: tabs[0]?.key ?? null, visible: false, expanded: false });

describe("会话信息栏", () => {
  it("改动数、已打开的页签（最多 5 个）、可打开的项", () => {
    const act = actions();
    render(<SessionInfo mode="docked" state={state(terms(7))} actions={act} shells={new Map()} panes={new Map()} summary={summary} />);
    const diff = screen.getByRole("button", { name: /2 个文件有未提交的改动/ });
    expect(diff.textContent).toContain("+13");
    expect(diff.textContent).toContain("−3");
    fireEvent.click(diff);
    expect(act.openDiff).toHaveBeenCalled();
    // 7 个终端只列 5 个，余下的「查看更多」打开面板。
    expect(screen.getAllByRole("button", { name: /^终端$/ })).toHaveLength(5);
    fireEvent.click(screen.getByRole("button", { name: "查看更多（2+）" }));
    expect(act.show).toHaveBeenCalled();
    fireEvent.click(screen.getAllByRole("button", { name: /^终端$/ })[1]);
    expect(act.activate).toHaveBeenCalledWith("t1");
    fireEvent.click(screen.getByRole("button", { name: /新建浏览器/ }));
    expect(act.newBrowser).toHaveBeenCalled();
  });

  it("不是 git 仓库时没有改动一栏；没开页签时没有「已打开标签页」", () => {
    render(<SessionInfo mode="docked" state={state([])} actions={actions()} shells={new Map()} panes={new Map()} summary={null} />);
    expect(screen.queryByText("更改")).toBeNull();
    expect(screen.queryByText("已打开标签页")).toBeNull();
    expect(screen.getByText("可打开")).toBeTruthy();
  });

  it("浮层：选了一项就关", () => {
    const onClose = vi.fn();
    const act = actions();
    render(<SessionInfo mode="floating" state={state([])} actions={act} shells={new Map()} panes={new Map()} summary={null} onClose={onClose} />);
    expect(screen.getByRole("dialog", { name: "会话信息" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /文件/ }));
    expect(act.openFiles).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });
});
