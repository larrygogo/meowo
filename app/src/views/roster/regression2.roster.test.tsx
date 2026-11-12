// 做空审查第二轮的复现（修后留作回归）。
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatShellBridge } from "../ChatWindow";

const invoke = vi.hoisted(() => vi.fn());
const boardHandlers = vi.hoisted(() => new Map<string, (event: { payload: unknown }) => void>());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ isFocused: () => Promise.resolve(true), onFocusChanged: () => Promise.resolve(() => {}) }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(() => Promise.resolve()),
  listen: vi.fn((name: string, handler: (event: { payload: unknown }) => void) => { boardHandlers.set(name, handler); return Promise.resolve(() => {}); }),
}));
vi.mock("../ChatWindow", () => ({
  ChatWindow: ({ shell }: { shell?: ChatShellBridge }) => (
    <div data-testid="chat" data-new-session={shell?.newSession?.message ?? ""}>
      {shell?.overlay}
      <button type="button" onClick={() => shell?.onReveal()}>reveal</button>
      <button type="button" onClick={() => shell?.onPageBack()}>page-back</button>
      <button type="button" onClick={() => shell?.onSession(7)}>pick-session</button>
    </div>
  ),
}));

import { makeSession } from "../../demo/data";
import type { Item } from "../sticker/types";
import { RosterShell } from "./RosterShell";

let sessions: Item[] = [];
let failPending = false;
const flush = () => act(async () => { await Promise.resolve(); await Promise.resolve(); });
const wait = (ms: number) => act(async () => { await new Promise((r) => setTimeout(r, ms)); });

describe("名单", () => {
  afterEach(() => { cleanup(); vi.restoreAllMocks(); });
  beforeEach(() => {
    localStorage.clear();
    boardHandlers.clear();
    invoke.mockReset();
    failPending = false;
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "get_live_sessions_page") return Promise.resolve({ items: sessions.map((s) => JSON.parse(JSON.stringify(s))), next_cursor: null });
      if (cmd === "sidebar_git_badges") return Promise.resolve([]);
      if (cmd === "pending_interaction") return failPending ? Promise.reject(new Error("网络抖动")) : Promise.resolve({ approval: null, question: null });
      if (cmd === "get_chat_history") return Promise.resolve({ items: [] });
      return Promise.resolve(undefined);
    });
    window.history.replaceState({}, "", "/?roster=1&sessionId=1");
  });

  it("R1 等着你的行：别的行在刷新时，它的「已持续时间」不走", async () => {
    const t0 = Date.now();
    const quiet = makeSession({ title: "等审批的", project: "p", agoMin: 0 });
    quiet.pending_review = "approval";
    const busy = makeSession({ title: "在跑的", project: "p", activity: "Bash: a" });
    sessions = [quiet, busy];
    render(<RosterShell />);
    await screen.findByText("等审批的");
    const timeOf = (name: string) => [...document.querySelectorAll(".roster-item")].find((el) => el.textContent?.includes(name))?.querySelector(".roster-time")?.textContent;
    const before = timeOf("等审批的");
    // 过了 10 分钟；在跑的那行一直有事件，等审批的那行内容没变。
    vi.spyOn(Date, "now").mockReturnValue(t0 + 10 * 60_000);
    sessions = [quiet, { ...busy, current_activity: "Bash: b", session: { ...busy.session, last_event_at: t0 + 10 * 60_000 } }];
    await act(async () => { boardHandlers.get("board-urgent")?.({ payload: null }); });
    await flush();
    expect(timeOf("等审批的")).toBe("10m");
  });

  it("R2 首次查询失败：托管会话的待批准行被说成「审批已回到终端」并摆出终端按键", async () => {
    const row = makeSession({ title: "等审批的", project: "p" });
    row.pending_review = "approval";
    row.pty_managed = true;
    sessions = [row, makeSession({ title: "乙", project: "p" })];
    failPending = true;
    render(<RosterShell />);
    fireEvent.click(await screen.findByText("等审批的"));
    await flush();
    const sheet = document.querySelector(".roster-sheet");
    expect(sheet?.textContent ?? "").not.toMatch(/回到了?终端/);
  });

  it("R3 从名单进过管理页之后：之后任何一次管理页「返回」都被带回名单", async () => {
    sessions = [makeSession({ title: "甲", project: "p" }), makeSession({ title: "乙", project: "p" })];
    render(<RosterShell />);
    await screen.findByText("甲");
    fireEvent.click(screen.getByRole("button", { name: "会话管理" }));
    fireEvent.click(screen.getByRole("button", { name: "reveal" })); // 管理页打开，名单让位
    // 人在管理页里点了一条会话进现场（对话窗 selectSession，不经过 onPageBack），之后在现场里
    // 从侧栏菜单再开管理页、点「返回」：
    fireEvent.click(screen.getByRole("button", { name: "pick-session" }));
    fireEvent.click(screen.getByRole("button", { name: "page-back" }));
    expect(document.querySelector(".roster")).toBeNull();
  });
});
