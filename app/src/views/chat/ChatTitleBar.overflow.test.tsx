// 标题栏放不下时，收起的功能必须出现在 ⋯ 菜单里（验收 #24，调研 c8/sys8）。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { useContext } from "react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
import { ChatTitleBar } from "./ChatTitleBar";
import { titleBarOverflow, TitleOverflowContext } from "./titleOverflow";

// jsdom 没有布局：用一个假 ResizeObserver 把标题栏宽度报成指定值。
function fakeWidth(width: number) {
  class FakeObserver {
    constructor(private cb: ResizeObserverCallback) {}
    observe() { this.cb([{ contentRect: { width } } as ResizeObserverEntry], this as unknown as ResizeObserver); }
    unobserve() {}
    disconnect() {}
  }
  vi.stubGlobal("ResizeObserver", FakeObserver);
}

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

/** 代替 ChatTitleMenu：只把上下文里的溢出项列出来。 */
function OverflowList() {
  const items = useContext(TitleOverflowContext);
  return <ul data-testid="overflow">{items.map((item) => <li key={item.key}>{item.group}</li>)}</ul>;
}

const props = {
  leading: null, title: <OverflowList />, archive: { busy: false, onClick: () => {} }, end: { busy: false, onClick: () => {} },
  cwd: "C:\\repo",
  gitSummary: { gitAvailable: true, isRepo: true, branch: "main", ahead: 0, behind: 0, files: [] } as never,
  changesOpen: false, panelOpen: true, terminalOn: false, info: { open: false, onToggle: () => {} },
  onToggleChanges: () => {}, onTogglePanel: () => {}, onToggleTerminal: () => {}, onError: () => {},
};

describe("标题栏溢出进 ⋯ 菜单", () => {
  it("门限表：没量到时全留栏上；320 宽收起终端、会话信息、改动、打开方式；手机上改动不收", () => {
    expect([...titleBarOverflow(0, false)]).toEqual([]);
    expect(titleBarOverflow(900, false).size).toBe(0);
    expect([...titleBarOverflow(600, false)]).toEqual(["pr"]);
    const narrow = titleBarOverflow(320, false);
    for (const key of ["pr", "open", "git", "terminal", "info", "archive"]) expect(narrow.has(key as never)).toBe(true);
    expect(titleBarOverflow(320, true).has("git")).toBe(false);
  });

  it("620 宽开着面板（标题栏 320）：终端等不在栏上，而在菜单里；面板开关和结束会话留在栏上", () => {
    fakeWidth(320);
    render(<ChatTitleBar {...props} />);
    expect(screen.queryByRole("button", { name: "终端" })).toBeNull();
    expect(screen.queryByRole("button", { name: "显示会话信息" })).toBeNull();
    expect(document.querySelector(".chat-bar .tb-git")).toBeNull();
    const groups = [...screen.getByTestId("overflow").querySelectorAll("li")].map((li) => li.textContent);
    expect(groups).toEqual(expect.arrayContaining(["open", "git", "terminal", "info"]));
    expect(document.querySelector(".chat-panel-toggle")).not.toBeNull();
    expect(screen.getByRole("button", { name: "结束会话" })).toBeTruthy();
  });

  it("宽标题栏：功能都在栏上，菜单不多出任何项", () => {
    fakeWidth(1000);
    render(<ChatTitleBar {...props} />);
    expect(screen.getByRole("button", { name: "终端" })).toBeTruthy();
    expect(document.querySelector(".chat-bar .tb-git")).not.toBeNull();
    expect(screen.getByTestId("overflow").children.length).toBe(0);
  });
});
