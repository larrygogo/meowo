import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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

  /** jsdom 里滚动尺寸恒为 0，手动装出「已经滚到底」的几何。 */
  function fakeScrolledToBottom(list: HTMLElement) {
    Object.defineProperty(list, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(list, "clientHeight", { value: 400, configurable: true });
    Object.defineProperty(list, "scrollTop", { value: 600, configurable: true });
  }

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
    fakeScrolledToBottom(list);

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

  it("翻页请求失败后回退，不留死 loading 行，且重滚可重试", async () => {
    const all = Array.from({ length: 150 }, (_, i) => session(1000 - i, `会话 ${i}`));
    const limits: number[] = [];
    invoke.mockImplementation((command: string, args: { limit: number }) => {
      if (command !== "get_live_sessions_page") return Promise.resolve();
      limits.push(args.limit);
      // 第二次（首个翻页请求）失败，其余成功。
      if (limits.length === 2) return Promise.reject(new Error("db busy"));
      return Promise.resolve(all.slice(0, args.limit));
    });
    render(<ChatSidebar activeId={1000} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /会话 0/ });
    const list = screen.getByRole("navigation");
    fakeScrolledToBottom(list);

    fireEvent.scroll(list);
    await waitFor(() => expect(limits).toEqual([60, 120]));
    // 失败后 loading 行必须消失（曾经的 bug：limit 卡在高位，loading 行永挂、滚动失效）。
    await waitFor(() => expect(screen.queryByText("正在加载会话…")).toBeNull());
    expect(screen.queryByRole("button", { name: /会话 60/ })).toBeNull();

    // 再滚：limit 已回退，应当能重新发起同样的翻页请求并成功。
    fireEvent.scroll(list);
    await screen.findByRole("button", { name: /会话 60/ });
    expect(limits).toEqual([60, 120, 120]);
  });

  it("翻页只在尾部追加，不重排用户正看着的前缀", async () => {
    const all = Array.from({ length: 120 }, (_, i) => session(1000 - i, `会话 ${i}`));
    let calls = 0;
    invoke.mockImplementation((command: string, args: { limit: number }) => {
      if (command !== "get_live_sessions_page") return Promise.resolve();
      calls += 1;
      if (calls === 1) return Promise.resolve(all.slice(0, args.limit));
      // 翻页响应（新返回形状）：后端把一条更深处的活会话排到了整页最前——
      // 侧栏不能照单全收把它插到用户视口上方，只能把新条目续在尾部。
      return Promise.resolve({
        items: [session(1, "新活会话", { connected: true }), ...all.slice(0, args.limit - 1)],
        next_cursor: { last_event_at: 1, id: 1 },
      });
    });
    render(<ChatSidebar activeId={1000} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /会话 0/ });
    const list = screen.getByRole("navigation");
    fakeScrolledToBottom(list);

    fireEvent.scroll(list);
    await screen.findByRole("button", { name: /新活会话/ });
    const names = within(list).getAllByRole("button").map((b) => b.textContent ?? "");
    expect(names[0]).toContain("会话 0");
    expect(names[59]).toContain("会话 59");
    // 新条目（含被后端顶到最前的活会话）只允许出现在原有 60 条之后。
    expect(names.findIndex((n) => n.includes("新活会话"))).toBe(60);
  });

  it("按 sessionTone 渲染状态点:running 脉冲、pending 召唤、断开/已结束不加点", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "get_live_sessions_page") {
        return Promise.resolve([
          session(1, "在跑", { connected: true, session: { id: 1, cc_session_id: "cc-1", status: "running" } } as Partial<LiveSession>),
          session(2, "待审批", { connected: true, pending_review: "approval", session: { id: 2, cc_session_id: "cc-2", status: "waiting" } } as Partial<LiveSession>),
          session(3, "在等", { connected: true, session: { id: 3, cc_session_id: "cc-3", status: "waiting" } } as Partial<LiveSession>),
          session(4, "已断开"),
        ]);
      }
      return Promise.resolve();
    });
    render(<ChatSidebar activeId={1} onSelect={() => {}} onCollapse={() => {}} />);
    const dotOf = async (name: RegExp) =>
      (await screen.findByRole("button", { name })).querySelector(".chat-sidebar-dot");
    expect((await dotOf(/在跑/))?.className).toContain("is-running");
    // pending 优先于 waiting:它有明确的动作召唤。
    expect((await dotOf(/待审批/))?.className).toContain("is-pending");
    expect((await dotOf(/在等/))?.className).toContain("is-waiting");
    // 断开/已结束:图标置灰已表达不活跃,不再叠点。
    expect(await dotOf(/已断开/)).toBeNull();
  });

  it("survives a backend without the sessions command", async () => {
    // demo/旧后端对未知命令返回 undefined：侧栏必须静默降级为空列表，不能崩掉整个窗口。
    invoke.mockResolvedValue(undefined);
    render(<ChatSidebar activeId={1} onSelect={() => {}} onCollapse={() => {}} />);
    expect(await screen.findByText("暂无会话")).toBeTruthy();
  });
});
