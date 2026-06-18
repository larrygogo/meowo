import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@testing-library/react";

const getLiveSessions = vi.fn();
let emitBoardChanged: () => void = () => {};
const unlisten = vi.fn();

vi.mock("./api", () => ({
  getLiveSessions: () => getLiveSessions(),
  getSettings: () =>
    Promise.resolve({ archive_hide_days: 0, notifications_enabled: true, theme: "dark", opacity: 94, ui_scale: 100 }),
  getAccount: () => Promise.resolve({ account: null, daily: null, usage: null }),
  refreshUsage: () => Promise.reject(new Error("USAGE_UNSUPPORTED")),
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
});
