import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// 名单行内的按键兜底（checkScreen）：人看到的画面和终端此刻的画面不一样时，这一下不发。
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { RemoteTerminalKeys } from "./RemoteTerminalKeys";

let current: string[] = [];
const writes = () => invoke.mock.calls.filter(([cmd]) => cmd === "write_managed_terminal");
const flush = () => act(async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); });

describe("RemoteTerminalKeys · 发键前比对画面", () => {
  afterEach(cleanup);
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "managed_terminal_screen" ? current : undefined));
  });

  it("画面没变：按键照常发出", async () => {
    current = ["1. Yes", "2. No"];
    render(<RemoteTerminalKeys sessionId={7} checkScreen />);
    await screen.findByText(/1\. Yes/);
    fireEvent.click(screen.getByRole("button", { name: "2" }));
    await flush();
    expect(writes()).toHaveLength(1);
    expect(writes()[0][1]).toMatchObject({ sessionId: 7, data: "2" });
  });

  it("画面变了：这一下不发，换上新画面并说明", async () => {
    current = ["1. Yes", "2. No"];
    render(<RemoteTerminalKeys sessionId={7} checkScreen />);
    await screen.findByText(/1\. Yes/);
    current = ["1. Delete everything", "2. Cancel"];
    fireEvent.click(screen.getByRole("button", { name: "1" }));
    await flush();
    expect(writes()).toHaveLength(0);
    expect(screen.getByText(/Delete everything/)).toBeTruthy();
    expect(screen.getByRole("status").textContent).toContain("这一下没有发出");
    // 看清之后再按一次，才发。
    fireEvent.click(screen.getByRole("button", { name: "2" }));
    await flush();
    expect(writes()).toHaveLength(1);
  });

  it("不开 checkScreen（手机页）：不多取一次画面，直接发", async () => {
    current = ["1. Yes"];
    render(<RemoteTerminalKeys sessionId={7} />);
    await screen.findByText(/1\. Yes/);
    const before = invoke.mock.calls.filter(([cmd]) => cmd === "managed_terminal_screen").length;
    fireEvent.click(screen.getByRole("button", { name: "1" }));
    await flush();
    expect(writes()).toHaveLength(1);
    expect(invoke.mock.calls.filter(([cmd]) => cmd === "managed_terminal_screen").length).toBe(before);
  });
});
