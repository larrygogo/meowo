import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// 手机上的名单：和桌面同一种行与决定单；停在名单上时持有名单租约并按时续约；
// 两端都能答同一条审批，后答的一端看到「已在别处处理」而不是报错。
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})), emit: vi.fn(() => Promise.resolve()) }));
vi.mock("../../remoteMode", async (original) => ({ ...(await original<typeof import("../../remoteMode")>()), remoteUi: () => true }));

import { makeSession } from "../../demo/data";
import type { Item } from "../sticker/types";
import { PhoneRoster } from "./PhoneRoster";

let sessions: Item[] = [];
let approval: unknown = null;
const mk = (title: string, patch: (s: Item) => void = () => {}) => {
  const s = makeSession({ title, project: "p" });
  patch(s);
  return s;
};
const registers = () => invoke.mock.calls.filter(([cmd]) => cmd === "register_approval_consumer");

describe("手机上的名单", () => {
  afterEach(() => { cleanup(); vi.useRealTimers(); });
  beforeEach(() => {
    localStorage.clear();
    approval = null;
    invoke.mockReset();
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "get_live_sessions_page") return Promise.resolve({ items: sessions, next_cursor: null });
      if (cmd === "sidebar_git_badges") return Promise.resolve([]);
      if (cmd === "pending_interaction") return Promise.resolve({ approval, question: null });
      return Promise.resolve(undefined);
    });
  });

  it("停在名单上持有名单租约（会话号 0）并每 20 秒续一次；离开时注销", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    sessions = [mk("甲"), mk("乙")];
    const view = render(<PhoneRoster onEnter={() => {}} />);
    await screen.findByText("甲");
    expect(registers()).toHaveLength(1);
    expect(registers()[0][1]).toMatchObject({ sessionId: 0 });
    await act(async () => { vi.advanceTimersByTime(20_000); });
    expect(registers()).toHaveLength(2);
    view.unmount();
    expect(invoke).toHaveBeenCalledWith("unregister_approval_consumer", { consumerId: (registers()[0][1] as { consumerId: string }).consumerId });
  });

  it("点一行展开并预取它的首屏；「进入现场」才进会话", async () => {
    sessions = [mk("甲"), mk("乙")];
    const onEnter = vi.fn();
    const onPrefetch = vi.fn();
    render(<PhoneRoster onEnter={onEnter} onPrefetch={onPrefetch} />);
    fireEvent.click(await screen.findByText("乙"));
    expect(onPrefetch).toHaveBeenCalledWith(sessions[1].session.id);
    expect(onEnter).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "进入现场" }));
    expect(onEnter).toHaveBeenCalledWith(sessions[1].session.id);
  });

  it("这条审批刚被另一端答掉：显示「已在别处处理」，不报错", async () => {
    sessions = [mk("甲", (s) => { s.pending_review = "approval"; }), mk("乙")];
    const id = sessions[0].session.id;
    approval = { sessionId: id, requestId: "req-1", provider: "claude", toolName: "Bash", description: null, input: "{}", permissionSuggestions: [], answerable: false };
    const base = invoke.getMockImplementation()!;
    invoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === "resolve_pending_approval") {
        // 桌面先答了：这条请求已经不在表里。
        approval = null;
        return Promise.reject("审批请求已失效");
      }
      return base(cmd, args);
    });
    render(<PhoneRoster onEnter={() => {}} />);
    fireEvent.click(await screen.findByText("甲"));
    fireEvent.click(await screen.findByRole("button", { name: "允许一次" }));
    await screen.findByText("已在别处处理");
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("button", { name: "允许一次" })).toBeNull();
  });
});
