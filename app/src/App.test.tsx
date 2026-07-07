import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";

const getLiveSessions = vi.fn();
let emitBoardChanged: () => void = () => {};
const unlisten = vi.fn();

vi.mock("./api", () => ({
  getLiveSessions: () => getLiveSessions(),
  getSettings: () =>
    Promise.resolve({ archive_hide_days: 0, notifications_enabled: true, theme: "dark", opacity: 94, ui_scale: 100 }),
  getAccounts: () => Promise.resolve([]),
  refreshUsage: (_provider: string) => Promise.reject(new Error("USAGE_UNSUPPORTED")),
  availableAgents: () => Promise.resolve([]),
}));
// 按事件名分别路由：board-changed → emitBoardChanged；snap-changed 忽略（Tauri 吸边，无法在 jsdom 中测试）
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: () => void) => {
    if (event === "board-changed") emitBoardChanged = cb;
    return Promise.resolve(unlisten);
  },
  emit: vi.fn(() => Promise.resolve()),
}));
// 吸边相关的 Tauri 命令/窗口 API：jsdom 无后端，给空实现避免报错。
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    setAlwaysOnTop: vi.fn(() => Promise.resolve()),
    outerSize: vi.fn(() => Promise.resolve({ width: 340, height: 440 })),
    scaleFactor: vi.fn(() => Promise.resolve(1)),
  }),
}));
// 更新检查相关插件：jsdom 无后端，给空实现（check 返回无更新）。
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn(() => Promise.resolve()) }));

import { App } from "./App";

beforeEach(() => {
  getLiveSessions.mockReset();
  getLiveSessions.mockResolvedValue([]);
  unlisten.mockReset();
  vi.mocked(invoke).mockClear();
  localStorage.clear();
});
afterEach(() => cleanup());

describe("App", () => {
  it("挂载时拉取一次活跃会话", async () => {
    render(<App />);
    await waitFor(() => expect(getLiveSessions).toHaveBeenCalledTimes(1));
  });

  it("收到 board-changed 后再次拉取", async () => {
    render(<App />);
    await waitFor(() => expect(getLiveSessions).toHaveBeenCalledTimes(1));
    emitBoardChanged();
    await waitFor(() => expect(getLiveSessions).toHaveBeenCalledTimes(2));
  });

  // 单一真相源：window-state 不再恢复尺寸(lib.rs)，main 窗口尺寸由 SIZE_KEY 持有。非吸附态启动
  // 无条件按 SIZE_KEY(默认 {360,440}) snap_restore 还原正常尺寸，且不走折叠分支。
  it("非吸附态启动按 SIZE_KEY 还原正常尺寸(snap_restore)，不折叠", async () => {
    render(<App />);
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "snap_restore",
        expect.objectContaining({ width: 360, height: 440 })
      )
    );
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("snap_collapse", expect.anything());
  });

  it("吸附态启动(SNAP_KEY 有边)走折叠分支，不触发尺寸还原", async () => {
    localStorage.setItem("cc-kanban-snap-edge", "left");
    render(<App />);
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("snap_collapse", expect.anything())
    );
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("snap_restore", expect.anything());
  });

  // 回归：SIZE_KEY 曾被「吸附态拖角缩成细条」的尺寸毒化(实测 {80,240}/{136,20})。loadSize 须把低于
  // 最小尺寸的值回落默认 {360,440}，否则还原会用毒化的细条尺寸、把窗口缩成细条。
  it("SIZE_KEY 被细条尺寸毒化时，loadSize 回落默认 {360,440} 再 snap_restore", async () => {
    localStorage.setItem("cc-kanban-normal-size", JSON.stringify({ w: 80, h: 37 }));
    render(<App />);
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "snap_restore",
        expect.objectContaining({ width: 360, height: 440 })
      )
    );
  });

  // 回归：SIZE_KEY 异常大值/非有限数(localStorage 被改坏)不能直接喂给 set_size，否则设出极端窗口。
  // loadSize 须校验上界(<=20000)与有限数，超界则回落默认 {360,440}。
  it("SIZE_KEY 异常大值时，loadSize 回落默认 {360,440}，不设出极端窗口", async () => {
    localStorage.setItem("cc-kanban-normal-size", JSON.stringify({ w: 999999, h: 999999 }));
    render(<App />);
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "snap_restore",
        expect.objectContaining({ width: 360, height: 440 })
      )
    );
  });
});
