// 标题栏放不下时，收起的功能必须出现在 ⋯ 菜单里（验收 #24，调研 c8/sys8）。
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { useContext } from "react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
import { ChatTitleBar } from "./ChatTitleBar";
import { TitleOverflowContext } from "./titleOverflow";

// jsdom 没有布局：标题栏报成指定宽，每个可收起项 40px、结束会话 72px、面板开关 28px，
// 标题区保底宽按 0 算（jsdom 读不到 styles.css 的 min-width）。
function fakeWidth(width: number) {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const w = this.classList.contains("chat-bar") ? width
      : this.dataset.overflowKey ? 40
      : this.classList.contains("chat-end") ? 72
      : this.classList.contains("chat-panel-toggle") ? 28 : 0;
    return { width: w, height: 28, top: 0, left: 0, right: w, bottom: 28, x: 0, y: 0, toJSON: () => ({}) } as DOMRect;
  });
}

const visible = (selector: string) => {
  const el = document.querySelector(selector);
  return !!el && !el.closest("[aria-hidden=true]");
};

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

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
  it("标题栏放不下（只够结束会话和面板开关）：终端等不在栏上，而在菜单里；面板开关和结束会话留在栏上", () => {
    fakeWidth(100);
    render(<ChatTitleBar {...props} />);
    expect(screen.queryByRole("button", { name: "终端" })).toBeNull();
    expect(screen.queryByRole("button", { name: "显示会话信息" })).toBeNull();
    expect(visible(".chat-bar .tb-git")).toBe(false);
    // 收起的项留在 DOM 里量宽度，但不可聚焦
    expect(document.querySelector('[data-overflow-key="terminal"]')?.hasAttribute("inert")).toBe(true);
    const groups = [...screen.getByTestId("overflow").querySelectorAll("li")].map((li) => li.textContent);
    expect(groups).toEqual(expect.arrayContaining(["open", "git", "terminal", "info"]));
    expect(document.querySelector(".chat-panel-toggle")).not.toBeNull();
    expect(screen.getByRole("button", { name: "结束会话" })).toBeTruthy();
    // 功能都开始收了，纯信息必须已经先收完：「结束会话」只留图标（名字还在读屏名里）。
    expect(document.querySelector(".chat-bar")?.getAttribute("data-compact")).toBe("endLabel prState branch project");
  });

  it("宽标题栏：功能都在栏上，菜单不多出任何项", () => {
    fakeWidth(1000);
    render(<ChatTitleBar {...props} />);
    expect(screen.getByRole("button", { name: "终端" })).toBeTruthy();
    expect(visible(".chat-bar .tb-git")).toBe(true);
    expect(screen.getByTestId("overflow").children.length).toBe(0);
    expect(document.querySelector(".chat-bar")?.hasAttribute("data-compact")).toBe(false);
  });
});
