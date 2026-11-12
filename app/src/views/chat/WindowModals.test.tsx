import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
const platform = vi.hoisted(() => ({ mac: false }));
vi.mock("../../platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../platform")>()),
  isMac: () => platform.mac,
}));

import { QuickSwitcher, ShortcutSheet, type PaletteCommand } from "./WindowModals";
import type { LiveSession } from "../../api";
import { zh } from "../../i18n/zh";

function session(id: number, title: string, extra: Partial<LiveSession> = {}): LiveSession {
  return {
    session: { id, cc_session_id: `cc-${id}`, status: "ended" },
    project_name: "meowo",
    task_title: title,
    connected: false,
    pending_review: null,
    cwd: "C:/Users/me/workspace/meowo",
    provider: "claude",
    ...extra,
  } as unknown as LiveSession;
}

/// 与 ChatWindow 实际下发的命令清单同构（标签直接取字典，防止两张皮）。
const COMMANDS: PaletteCommand[] = [
  { id: "new-session", label: zh.chat.shortcutNewSession, keys: "Ctrl+N" },
  { id: "view-chat", label: zh.chat.cmdViewChat, keys: "Ctrl+1" },
  { id: "toggle-sidebar", label: zh.chat.shortcutSidebar, keys: "Ctrl+B" },
  { id: "focus-search", label: zh.chat.shortcutSearch, keys: "Ctrl+F" },
  { id: "open-settings", label: zh.sticker.openSettings },
];

function mockSessions(items: LiveSession[]) {
  invoke.mockImplementation((command: string) => {
    if (command === "get_live_sessions_page") return Promise.resolve({ items, next_cursor: null });
    return Promise.resolve();
  });
}

function renderPalette(overrides: Partial<Parameters<typeof QuickSwitcher>[0]> = {}) {
  const props = {
    activeId: 0,
    commands: COMMANDS,
    onPick: vi.fn(),
    onCommand: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  };
  render(<QuickSwitcher {...props} />);
  return props;
}

describe("QuickSwitcher 命令面板（U1-14）", () => {
  beforeEach(() => invoke.mockReset());
  afterEach(cleanup);

  it("空查询时会话组与命令组都全列（组头小标签 + 命令右侧快捷键提示）", async () => {
    mockSessions([session(1, "修 bug"), session(2, "写文档")]);
    renderPalette();
    expect(await screen.findByRole("option", { name: /修 bug/ })).toBeTruthy();
    // 两组组头都在，会话组在前、命令组在后。
    const listbox = screen.getByRole("listbox");
    expect(listbox.textContent).toContain(zh.chat.switcherGroupSessions);
    expect(listbox.textContent).toContain(zh.chat.switcherGroupCommands);
    expect(listbox.textContent!.indexOf(zh.chat.switcherGroupSessions))
      .toBeLessThan(listbox.textContent!.indexOf(zh.chat.switcherGroupCommands));
    // 命令全列 + 快捷键提示渲染。
    expect(screen.getByRole("option", { name: /新建会话/ }).textContent).toContain("Ctrl+N");
    expect(screen.getByRole("option", { name: /打开设置/ })).toBeTruthy();
  });

  it("↑↓ 跨两组导航：从最后一个会话 ArrowDown 落到第一条命令，Enter 执行并关闭", async () => {
    mockSessions([session(1, "修 bug")]);
    const props = renderPalette();
    const input = await screen.findByRole("textbox");
    // 1 条会话 + 5 条命令：两下 ↓ 到第二条命令（view-chat）。
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(screen.getByRole("option", { name: new RegExp(zh.chat.cmdViewChat) }).getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onClose).toHaveBeenCalled();
    expect(props.onCommand).toHaveBeenCalledWith("view-chat");
    expect(props.onPick).not.toHaveBeenCalled();
  });

  it("Home / End 跨两组跳首尾", async () => {
    mockSessions([session(1, "修 bug")]);
    renderPalette();
    const input = await screen.findByRole("textbox");
    fireEvent.keyDown(input, { key: "End" });
    expect(screen.getByRole("option", { name: new RegExp(zh.sticker.openSettings) }).getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(input, { key: "Home" });
    expect(screen.getByRole("option", { name: /修 bug/ }).getAttribute("aria-selected")).toBe("true");
  });

  it("输入查询同时过滤会话（下沉后端 search）与命令（客户端子串）", async () => {
    mockSessions([session(1, "修 bug")]);
    renderPalette();
    const input = await screen.findByRole("textbox");
    // 命令过滤即时生效：只剩标签含「视图」的命令。
    fireEvent.change(input, { target: { value: "视图" } });
    expect(screen.queryByRole("option", { name: /新建会话/ })).toBeNull();
    expect(screen.getByRole("option", { name: new RegExp(zh.chat.cmdViewChat) })).toBeTruthy();
    // 会话查询带 200ms 防抖下沉后端，search 参数原样传下去。
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith(
        "get_live_sessions_page",
        expect.objectContaining({ search: "视图" }),
      );
    });
  });

  it("两组都落空时显示统一空态", async () => {
    mockSessions([]);
    renderPalette();
    const input = await screen.findByRole("textbox");
    fireEvent.change(input, { target: { value: "不存在的东西" } });
    await waitFor(() => {
      expect(screen.getByText(zh.chat.switcherEmpty)).toBeTruthy();
    });
    expect(screen.queryByRole("option")).toBeNull();
  });

  it("命令条目点击执行并关闭弹层", async () => {
    mockSessions([session(1, "修 bug")]);
    const props = renderPalette();
    const cmd = await screen.findByRole("option", { name: /打开设置/ });
    fireEvent.click(cmd);
    expect(props.onClose).toHaveBeenCalled();
    expect(props.onCommand).toHaveBeenCalledWith("open-settings");
  });

  // 二期 #4（2026-11-10 定）：空查询默认选中第 0 条，而第 0 条多半就是当前会话（最近活跃），
  // 打开就回车等于没切。改为默认选中第一个不是当前会话的条目。
  it("空查询默认选中第一个非当前会话，回车切到它", async () => {
    mockSessions([session(7, "当前会话"), session(8, "别的会话")]);
    const props = renderPalette({ activeId: 7 });
    const input = await screen.findByRole("textbox");
    await waitFor(() => expect(screen.getByRole("option", { name: /别的会话/ }).getAttribute("aria-selected")).toBe("true"));
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onClose).toHaveBeenCalled();
    expect(props.onPick).toHaveBeenCalledWith(8);
  });

  it("点到当前会话只关弹层，不重复跳转", async () => {
    mockSessions([session(7, "当前会话"), session(8, "别的会话")]);
    const props = renderPalette({ activeId: 7 });
    fireEvent.click(await screen.findByRole("option", { name: /当前会话/ }));
    expect(props.onClose).toHaveBeenCalled();
    expect(props.onPick).not.toHaveBeenCalled();
  });

  it("行尾显示未读点；当前会话正在看，不显示", async () => {
    mockSessions([
      session(7, "当前会话", { unread: true } as Partial<LiveSession>),
      session(8, "没看过的", { unread: true } as Partial<LiveSession>),
      session(9, "看过的"),
    ]);
    renderPalette({ activeId: 7 });
    const unreadOf = async (name: RegExp) => (await screen.findByRole("option", { name })).querySelector(".chat-switcher-unread");
    expect(await unreadOf(/没看过的/)).toBeTruthy();
    expect(await unreadOf(/^看过的/)).toBeNull();
    expect(await unreadOf(/当前会话/)).toBeNull();
  });
});

describe("ShortcutSheet 快捷键速查表", () => {
  afterEach(cleanup);

  it("复用完整快捷键清单并可按 Esc 关闭", () => {
    const onClose = vi.fn();
    render(<ShortcutSheet onClose={onClose} />);
    const dialog = screen.getByRole("dialog", { name: zh.chat.shortcutsTitle });
    // 18 + 面板的改动 / 文件 / 切页签三行（UI 调研 i17）+ 审批数字键、空框 Esc 中断两行（走查 C-5、C-9）。
    expect(dialog.querySelectorAll(".chat-shortcuts-row")).toHaveLength(23);
    expect(dialog.textContent).toContain(zh.chat.shortcutApproveDigit);
    expect(dialog.textContent).toContain(zh.chat.shortcutInterrupt);
    expect(dialog.textContent).toContain("Ctrl+Shift+F");
    expect(dialog.textContent).toContain("Ctrl+`");
    expect(dialog.textContent).toContain(zh.chat.shortcutTerminalNote("Ctrl"));
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  // 二期 #5/#8：Ctrl+F 统一为找字（Ctrl+Shift+F 别名同行），新增跳下一个待处理会话。
  it("Ctrl+F 一行写找字并带别名；有 Alt+Shift+↓ 跳转行", () => {
    render(<ShortcutSheet onClose={vi.fn()} />);
    const rows = [...document.querySelectorAll(".chat-shortcuts-row")];
    const rowOf = (desc: string) => rows.find((row) => row.querySelector("dd")?.textContent === desc);
    expect(rowOf(zh.chat.shortcutFind)?.querySelector("dt")?.textContent).toBe("Ctrl+F / Ctrl+Shift+F");
    expect(rowOf(zh.chat.shortcutNextPending)?.querySelector("dt")?.textContent).toBe("Alt+Shift+↓");
    // 搜会话不再占 Ctrl+F。
    expect(rowOf(zh.chat.shortcutSearch)).toBeUndefined();
  });

  it("mac 上速查表显示 ⌘ 与 ⌥", () => {
    platform.mac = true;
    try {
      render(<ShortcutSheet onClose={vi.fn()} />);
      const dialog = screen.getByRole("dialog", { name: zh.chat.shortcutsTitle });
      expect(dialog.textContent).toContain("⌘+K");
      expect(dialog.textContent).toContain("⌘+F / ⌘+Shift+F");
      expect(dialog.textContent).toContain("⌥+Shift+↓");
      expect(dialog.textContent).not.toContain("Ctrl");
      expect(dialog.textContent).toContain(zh.chat.shortcutTerminalNote("⌘", "⌃"));
    } finally {
      platform.mac = false;
    }
  });
});
