import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@testing-library/react";

const getLiveSessions = vi.fn();
let emitBoardChanged: () => void = () => {};
const unlisten = vi.fn();

vi.mock("./api", () => ({
  getLiveSessions: () => getLiveSessions(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (_event: string, cb: () => void) => {
    emitBoardChanged = cb;
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
