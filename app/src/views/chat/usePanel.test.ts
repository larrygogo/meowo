import { afterEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke, Channel: class {} }));
const { handlers } = vi.hoisted(() => ({ handlers: new Map<string, (event: { payload: unknown }) => void>() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, handler: (event: { payload: unknown }) => void) => {
    handlers.set(name, handler);
    return Promise.resolve(() => {});
  }),
}));

import { loadPanel, savePanel, type PanelState } from "./panelTabs";
import { usePanel } from "./usePanel";

afterEach(() => {
  invoke.mockReset();
  localStorage.clear();
});

// 会话 1 有一个浏览器页签（后端编号 5）和一个终端（后端编号 3）；会话 2 什么都没有。
const panes: Record<number, unknown[]> = {
  1: [{ id: 5, owner: 1, url: "https://example.com/", title: "Example", loading: false, agentActive: false, agentPaused: false }],
  2: [],
};
const shells: Record<number, unknown[]> = { 1: [{ id: 3, owner: 1, title: "pwsh", exited: false }], 2: [] };

describe("面板与后端进程对号", () => {
  it("切走再切回来，不会重开浏览器和终端，也不会多出页签（实拍：每切回来一次多一个）", async () => {
    const state: PanelState = {
      tabs: [
        { key: "w", type: "browser", paneId: 5, url: "https://example.com/", title: "Example" },
        { key: "t", type: "term", shellId: 3, cwd: "C:\\repo" },
      ],
      active: "w",
      visible: true,
      expanded: false,
    };
    savePanel(1, state);
    invoke.mockImplementation(async (cmd: string, args?: { owner?: number }) => {
      if (cmd === "browser_pane_list") return panes[args?.owner ?? 0] ?? [];
      if (cmd === "shell_terminal_list") return shells[args?.owner ?? 0] ?? [];
      if (cmd === "browser_pane_open") return { id: 99, owner: args?.owner, url: "about:blank", title: "", loading: false, agentActive: false, agentPaused: false };
      if (cmd === "shell_terminal_open") return { id: 98, owner: args?.owner, title: "pwsh", exited: false };
      return null;
    });
    const { result, rerender } = renderHook(({ id }) => usePanel(id, "C:\\repo", () => {}), { initialProps: { id: 1 } });
    await waitFor(() => expect(result.current.panes.has(5)).toBe(true));
    act(() => rerender({ id: 2 }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("browser_pane_list", { owner: 2 }));
    act(() => rerender({ id: 1 }));
    await waitFor(() => expect(result.current.panes.has(5)).toBe(true));
    // 切回来以后，切到终端页签也不该新起 shell。
    act(() => result.current.actions.activate("t"));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(invoke).not.toHaveBeenCalledWith("browser_pane_open", expect.anything());
    expect(invoke).not.toHaveBeenCalledWith("shell_terminal_open", expect.anything());
    expect(result.current.state.tabs).toHaveLength(2);
  });

  it("应用重启过（后端没有这个浏览器了），切到页签时按存下的网址重新打开", async () => {
    savePanel(1, { tabs: [{ key: "w", type: "browser", paneId: 5, url: "https://example.com/", title: "" }], active: "w", visible: true, expanded: false });
    invoke.mockImplementation(async (cmd: string, args?: { owner?: number; url?: string }) => {
      if (cmd === "browser_pane_list" || cmd === "shell_terminal_list") return [];
      if (cmd === "browser_pane_open") return { id: 7, owner: args?.owner, url: args?.url, title: "", loading: true, agentActive: false, agentPaused: false };
      return null;
    });
    const { result } = renderHook(() => usePanel(1, "C:\\repo", () => {}));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("browser_pane_open", { owner: 1, url: "https://example.com/" }));
    await waitFor(() => expect(result.current.state.tabs[0]).toMatchObject({ paneId: 7 }));
    expect(invoke.mock.calls.filter(([cmd]) => cmd === "browser_pane_open")).toHaveLength(1);
  });

  it("每个会话记住自己的面板开着还是关着，切来切去不互相影响", async () => {
    invoke.mockImplementation(async (cmd: string) => (cmd.endsWith("_list") ? [] : null));
    const { result, rerender } = renderHook(({ id }) => usePanel(id, "C:\\repo", () => {}), { initialProps: { id: 1 } });
    // 会话 1：打开面板（还没有页签，显示启动列表）。
    act(() => result.current.actions.show());
    expect(result.current.state.visible).toBe(true);
    // 会话 2：开文件页签，再把面板关掉。
    act(() => rerender({ id: 2 }));
    expect(result.current.state.visible).toBe(false);
    act(() => result.current.actions.openFiles());
    act(() => result.current.actions.hide());
    // 切回会话 1：面板还开着；再切回会话 2：关着，文件页签还在。
    act(() => rerender({ id: 1 }));
    expect(result.current.state.visible).toBe(true);
    act(() => rerender({ id: 2 }));
    expect(result.current.state.visible).toBe(false);
    expect(result.current.state.tabs.map((tab) => tab.type)).toEqual(["files"]);
    // 应用重启（重新挂载）后同样记得。
    const again = renderHook(() => usePanel(1, "C:\\repo", () => {}));
    expect(again.result.current.state.visible).toBe(true);
  });

  it("会话结束或被关闭后，面板页签清空，记录也删掉（别的会话的也一样）", async () => {
    invoke.mockImplementation(async (cmd: string) => (cmd.endsWith("_list") ? [] : null));
    savePanel(1, { tabs: [{ key: "f", type: "files" }], active: "f", visible: true, expanded: false });
    savePanel(2, { tabs: [{ key: "d", type: "diff" }], active: "d", visible: true, expanded: false });
    const { result } = renderHook(() => usePanel(1, "C:\\repo", () => {}));
    expect(result.current.state.tabs).toHaveLength(1);
    await waitFor(() => expect(handlers.has("panel-session-closed")).toBe(true));
    act(() => handlers.get("panel-session-closed")?.({ payload: 2 }));
    expect(loadPanel(2).tabs).toHaveLength(0);
    expect(result.current.state.tabs).toHaveLength(1);
    act(() => handlers.get("panel-session-closed")?.({ payload: 1 }));
    expect(result.current.state).toEqual({ tabs: [], active: null, visible: false, expanded: false });
    expect(loadPanel(1).tabs).toHaveLength(0);
  });
});
