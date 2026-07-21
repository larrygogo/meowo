import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { ChatSidebar } from "./ChatSidebar";
import type { LiveSession } from "../api";

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

describe("ChatSidebar", () => {
  afterEach(cleanup);
  beforeEach(() => {
    invoke.mockReset();
    localStorage.clear();
  });

  it("lists sessions and switches on click", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "get_live_sessions_page") {
        return Promise.resolve([session(163, "解决冲突", { connected: true }), session(150, "旧任务")]);
      }
      return Promise.resolve();
    });
    const onSelect = vi.fn();
    render(<ChatSidebar activeId={163} onSelect={onSelect} onCollapse={() => {}} />);
    const active = await screen.findByRole("button", { name: /解决冲突/ });
    expect(active.getAttribute("aria-current")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: /旧任务/ }));
    expect(onSelect).toHaveBeenCalledWith(150);
  });

  it("reports collapse to the parent", async () => {
    invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "get_live_sessions_page" ? [session(1, "任务A")] : undefined));
    const onCollapse = vi.fn();
    render(<ChatSidebar activeId={1} onSelect={() => {}} onCollapse={onCollapse} />);
    await screen.findByRole("button", { name: /任务A/ });
    fireEvent.click(screen.getByRole("button", { name: "收起会话列表" }));
    // 折叠状态归 ChatWindow 持有（展开入口在标题栏），侧栏只上报意图。
    expect(onCollapse).toHaveBeenCalled();
  });

  it("滚到底继续翻页，直到后端给不满一页", async () => {
    const all = Array.from({ length: 150 }, (_, i) => session(1000 - i, `会话 ${i}`));
    const limits: number[] = [];
    invoke.mockImplementation((command: string, args: { limit: number }) => {
      if (command !== "get_live_sessions_page") return Promise.resolve();
      limits.push(args.limit);
      return Promise.resolve(all.slice(0, args.limit));
    });
    render(<ChatSidebar activeId={1000} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /会话 0/ });
    expect(limits).toEqual([60]);
    expect(screen.queryByRole("button", { name: /会话 60/ })).toBeNull();

    const list = screen.getByRole("navigation");
    // jsdom 里这些尺寸恒为 0，得手动装出「已经滚到底」的几何。
    Object.defineProperty(list, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(list, "clientHeight", { value: 400, configurable: true });
    Object.defineProperty(list, "scrollTop", { value: 600, configurable: true });

    fireEvent.scroll(list);
    await screen.findByRole("button", { name: /会话 60/ });
    expect(limits).toEqual([60, 120]);

    fireEvent.scroll(list);
    await screen.findByRole("button", { name: /会话 149/ });
    expect(limits).toEqual([60, 120, 180]);

    // 150 < 180：后端已经给不满，到此为止，再滚也不应该再发请求。
    fireEvent.scroll(list);
    fireEvent.scroll(list);
    expect(limits).toEqual([60, 120, 180]);
  });

  it("survives a backend without the sessions command", async () => {
    // demo/旧后端对未知命令返回 undefined：侧栏必须静默降级为空列表，不能崩掉整个窗口。
    invoke.mockResolvedValue(undefined);
    render(<ChatSidebar activeId={1} onSelect={() => {}} onCollapse={() => {}} />);
    expect(await screen.findByText("暂无会话")).toBeTruthy();
  });
});
