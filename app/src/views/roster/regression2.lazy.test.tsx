// 做空审查第二轮：速览分包加载失败（手机页资产陈旧 / 断网）时点开一行会怎样。
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(() => Promise.resolve()), listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("./RosterGlance", () => { throw new Error("Failed to fetch dynamically imported module"); });

import { makeSession } from "../../demo/data";
import { WindowErrorBoundary as ErrorBoundary } from "../../ErrorBoundary";
import { PhoneRoster } from "./PhoneRoster";

describe("懒加载失败", () => {
  afterEach(cleanup);
  it("L1 分包取不到：点开一行后整张名单被错误页取代", async () => {
    const rows = [makeSession({ title: "甲", project: "p", lastAi: "我改完了" }), makeSession({ title: "乙", project: "p" })];
    invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "get_live_sessions_page" ? { items: rows, next_cursor: null } : cmd === "sidebar_git_badges" ? [] : undefined));
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    render(<ErrorBoundary><PhoneRoster onEnter={() => {}} /></ErrorBoundary>);
    fireEvent.click(await screen.findByText("甲"));
    await act(async () => { await new Promise((r) => setTimeout(r, 200)); });
    spy.mockRestore();
    expect(document.querySelectorAll(".roster-item").length).toBe(2);
    expect(screen.getByRole("button", { name: "进入现场" })).toBeTruthy();
  });
});
