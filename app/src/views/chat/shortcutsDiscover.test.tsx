import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

// UI 调研 i17：速查表漏了 Ctrl+Shift+G/E、Ctrl+Tab/Ctrl+Shift+Tab（ChatWindow 早已实现），ShortcutList 用
// flatMap 把「窗口 / 输入框」分组标题丢了；标题栏按钮 tooltip 不带快捷键。m12：手机提交框占位符写 Ctrl+Enter。
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("../../platform", async (o) => ({ ...(await o<typeof import("../../platform")>()), isMac: () => false }));

import { ShortcutList } from "./WindowModals";
import { ChatTitleBar } from "./ChatTitleBar";
import { SourceControl } from "./SourceControl";
import { zh } from "../../i18n/zh";

afterEach(() => { cleanup(); delete (globalThis as Record<string, unknown>).__MEOWO_REMOTE__; });

describe("快捷键可发现", () => {
  it("速查表有面板的改动/文件/切页签，并保留分组标题", () => {
    const { container } = render(<ShortcutList />);
    const keys = [...container.querySelectorAll(".chat-shortcuts-row dt")].map((dt) => dt.textContent);
    expect(keys).toEqual(expect.arrayContaining(["Ctrl+Shift+G", "Ctrl+Shift+E", "Ctrl+Tab / Ctrl+Shift+Tab"]));
    const titles = [...container.querySelectorAll(".chat-shortcuts-group-title")].map((h) => h.textContent);
    expect(titles).toEqual([zh.chat.shortcutGroupWindow, zh.chat.shortcutGroupInput]);
  });

  it("标题栏终端与面板开关的 tooltip 带快捷键，读屏名不变", () => {
    render(<ChatTitleBar leading={null} title={<span>t</span>} archive={null} end={null} cwd="C:/repo" gitSummary={null}
      changesOpen={false} panelOpen={false} terminalOn={false}
      onToggleChanges={() => {}} onTogglePanel={() => {}} onToggleTerminal={() => {}} onError={() => {}} />);
    expect(screen.getByRole("button", { name: zh.chat.terminal }).getAttribute("data-tip")).toBe(`${zh.chat.terminal} Ctrl+2`);
    expect(screen.getByRole("button", { name: zh.chat.togglePanel }).getAttribute("data-tip")).toBe(`${zh.chat.togglePanel} Ctrl+Shift+B`);
  });

  it("手机提交框占位符不含快捷键，桌面照旧提示 Ctrl+Enter", () => {
    const repo = { root: "C:/w/m", name: "m", isCwdRepo: true, base: null, baseLabel: null, committed: [],
      summary: { gitAvailable: true, isRepo: true, branch: "feature/very-long-branch-name", ahead: 0, behind: 0, files: [] } };
    const renderScm = () => render(
      <SourceControl cwd="C:/w/m" repos={[repo]} selectedRoot={null} onSelectRoot={() => {}} onChanged={() => {}}
        comments={[]} touchedPaths={[]} turnTouchedPaths={[]} onAddComment={() => {}} onRemoveComment={() => {}}
        escOverrideRef={{ current: null }} />);
    renderScm();
    expect(document.querySelector<HTMLTextAreaElement>(".scm-commit-input")!.placeholder).toMatch(/Ctrl\+Enter/);
    cleanup();
    (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = true;
    renderScm();
    const placeholder = document.querySelector<HTMLTextAreaElement>(".scm-commit-input")!.placeholder;
    expect(placeholder).toBe(zh.chat.scmCommitPlaceholderRemote);
    expect(placeholder).not.toMatch(/Ctrl|⌘/);
  });
});
