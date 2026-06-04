import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@testing-library/react";

const getLiveSessions = vi.fn();
let emitBoardChanged: () => void = () => {};
const unlisten = vi.fn();

vi.mock("./api", () => ({
  getLiveSessions: () => getLiveSessions(),
}));
// 按事件名分别路由：board-changed → emitBoardChanged；snap-changed 忽略（Tauri 吸边，无法在 jsdom 中测试）
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: () => void) => {
    if (event === "board-changed") emitBoardChanged = cb;
    return Promise.resolve(unlisten);
  },
}));

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
