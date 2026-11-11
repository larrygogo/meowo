import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
// 记下订阅回调，测试里能手动发 board-urgent 驱动一轮列表刷新（它绕过节流）。
const listeners = vi.hoisted(() => new Map<string, () => void>());
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, cb: () => void) => {
    listeners.set(name, cb);
    return Promise.resolve(() => {});
  }),
}));
// 侧栏内折叠钮是 macOS 专属（非 mac 的折叠/展开统一在 ChatWindow 顶栏），按平台分测。
const isMacMock = vi.hoisted(() => vi.fn(() => false));
vi.mock("../platform", () => ({ isMac: isMacMock }));
const dialogOpen = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: dialogOpen }));

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
    isMacMock.mockReturnValue(false);
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
    render(<ChatSidebar activeId={163} approvalAwaitingIds={new Set()} onSelect={onSelect} onCollapse={() => {}} />);
    const active = await screen.findByRole("button", { name: /解决冲突/ });
    expect(active.getAttribute("aria-current")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: /旧任务/ }));
    expect(onSelect).toHaveBeenCalledWith(150);
  });

  /// 未读点：回合结束时没在看的会话挂一个小圆点；当前打开的那条不挂（正在看就是已读）。
  it("未读会话显示未读点，当前会话不显示", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "get_live_sessions_page") {
        return Promise.resolve([
          session(163, "正在看的", { connected: true, unread: true }),
          session(150, "错过回复的", { connected: true, unread: true }),
          session(140, "已读的", { connected: true }),
        ]);
      }
      return Promise.resolve();
    });
    render(<ChatSidebar activeId={163} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /错过回复的/ });
    expect(screen.getAllByRole("img", { name: "有新回复" })).toHaveLength(1);
  });

  /// 列表已加载后刷新连续失败：第三次才挂「同步中断」，一次成功即撤。
  it("刷新连续失败三次挂同步中断，恢复后撤掉", async () => {
    let fail = false;
    invoke.mockImplementation((command: string) => {
      if (command === "get_live_sessions_page") {
        return fail ? Promise.reject(new Error("remote/timeout")) : Promise.resolve([session(1, "在跑的", { connected: true })]);
      }
      return Promise.resolve();
    });
    render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /在跑的/ });
    const tick = async () => {
      await waitFor(() => expect(listeners.get("board-urgent")).toBeTruthy());
      await act(async () => { listeners.get("board-urgent")!(); });
    };
    fail = true;
    await tick();
    await tick();
    expect(screen.queryByRole("alert")).toBeNull();
    await tick();
    expect(await screen.findByRole("alert")).toHaveProperty("textContent", "同步中断，正在重连，显示的是缓存");
    // 缓存的列表还在。
    expect(screen.getByRole("button", { name: /在跑的/ })).toBeTruthy();
    fail = false;
    await tick();
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  /// 「进行中 / 已完成」标签页：已完成即 archived（对照 Kimi），常驻顶部，切过去改拉归档页。
  it("已完成标签页拉取已完成会话，空时给专属空态，切回进行中恢复", async () => {
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_live_sessions_page") {
        return Promise.resolve(args?.filter === "archived" ? [] : [session(1, "在跑的", { connected: true })]);
      }
      return Promise.resolve();
    });
    render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /在跑的/ });
    expect(screen.getByRole("tab", { name: "进行中" }).getAttribute("aria-selected")).toBe("true");
    fireEvent.click(screen.getByRole("tab", { name: "已完成" }));
    expect(await screen.findByText("还没有已完成的会话")).toBeTruthy();
    expect(invoke).toHaveBeenCalledWith("get_live_sessions_page", expect.objectContaining({ filter: "archived" }));
    fireEvent.click(screen.getByRole("tab", { name: "进行中" }));
    expect(await screen.findByRole("button", { name: /在跑的/ })).toBeTruthy();
  });

  /// 组头汇总点：未读压过等待/运行（组折起来时行上的未读点看不见），但让位于出错。
  it("组头汇总未读，出错时让位给出错", async () => {
    localStorage.setItem("meowo-chat-sidebar-grouped", "1");
    let rows = [
      session(1, "正在看的", { connected: true, session: { id: 1, cc_session_id: "cc-1", status: "waiting" } } as Partial<LiveSession>),
      session(2, "错过回复的", { connected: true, unread: true, session: { id: 2, cc_session_id: "cc-2", status: "waiting" } } as Partial<LiveSession>),
    ];
    invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "get_live_sessions_page" ? rows : undefined));
    const { container, unmount } = render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /错过回复的/ });
    expect(container.querySelector(".chat-sidebar-dot.is-unread")).toBeTruthy();
    unmount();

    rows = [...rows, session(3, "出错的", { connected: true, errored: true, session: { id: 3, cc_session_id: "cc-3", status: "waiting" } } as Partial<LiveSession>)];
    const again = render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /出错的/ });
    expect(again.container.querySelector(".chat-sidebar-dot.is-unread")).toBeNull();
    expect(again.container.querySelector(".chat-sidebar-group .chat-sidebar-dot.is-error")).toBeTruthy();
  });

  /// 一个跑了一阵的目录能堆出几十条历史会话，把真正在跑的那一两条挤出视野。
  /// 未运行的默认收起，点一下才展开——但绝不能真的丢掉。
  describe("分组里的未运行会话默认收起", () => {
    const mixed = [
      session(1, "在跑的", { connected: true }),
      session(2, "停了的"),
      session(3, "也停了"),
    ];
    const renderGrouped = async (activeId = 1) => {
      localStorage.setItem("meowo-chat-sidebar-grouped", "1");
      invoke.mockImplementation((command: string) =>
        Promise.resolve(command === "get_live_sessions_page" ? mixed : undefined));
      render(<ChatSidebar activeId={activeId} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
      await screen.findByRole("button", { name: /在跑的/ });
    };

    it("默认只显示在跑的，其余折进一个可点的入口", async () => {
      await renderGrouped();
      expect(screen.queryByRole("button", { name: /停了的/ })).toBeNull();
      fireEvent.click(screen.getByRole("button", { name: "显示 2 个未运行会话" }));
      expect(screen.getByRole("button", { name: /停了的/ })).toBeTruthy();
      expect(screen.getByRole("button", { name: /也停了/ })).toBeTruthy();
      // 再点一下收回去。
      fireEvent.click(screen.getByRole("button", { name: "收起未运行会话" }));
      expect(screen.queryByRole("button", { name: /停了的/ })).toBeNull();
    });

    /// 正开着的会话无论死活都得留在视野里，否则用户一进来就找不到自己在哪。
    it("当前打开的那条即使已断开也不收起", async () => {
      await renderGrouped(2);
      expect(screen.getByRole("button", { name: /停了的/ })).toBeTruthy();
      expect(screen.getByRole("button", { name: "显示 1 个未运行会话" })).toBeTruthy();
    });

    /// 当前打开的会话结束后落进「已结束」组：组里没有任何在跑的,不能只因它被选中
    /// 就把其余几十条折没——用户切过去一看,列表凭空缩成一条。
    it("组里只有当前打开的这条『活』时不折叠", async () => {
      localStorage.setItem("meowo-chat-sidebar-grouped", "1");
      invoke.mockImplementation((command: string) =>
        Promise.resolve(command === "get_live_sessions_page"
          ? [session(8, "旧一"), session(9, "旧二"), session(10, "旧三")]
          : undefined));
      render(<ChatSidebar activeId={8} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
      await screen.findByRole("button", { name: /旧一/ });
      expect(screen.getByRole("button", { name: /旧二/ })).toBeTruthy();
      expect(screen.getByRole("button", { name: /旧三/ })).toBeTruthy();
      expect(screen.queryByRole("button", { name: /显示 .* 个未运行会话/ })).toBeNull();
    });

    /// 一条在跑的都没有时不收——展开分组后空空如也比排得长更难用。
    it("整组都没在跑时照常全列", async () => {
      localStorage.setItem("meowo-chat-sidebar-grouped", "1");
      invoke.mockImplementation((command: string) =>
        Promise.resolve(command === "get_live_sessions_page" ? [session(8, "旧一"), session(9, "旧二")] : undefined));
      render(<ChatSidebar activeId={0} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
      await screen.findByRole("button", { name: /旧一/ });
      expect(screen.getByRole("button", { name: /旧二/ })).toBeTruthy();
      expect(screen.queryByRole("button", { name: /显示 .* 个未运行会话/ })).toBeNull();
    });
  });

  it("外部切换当前会话时把它滚回可视区", async () => {
    // jsdom 没有 scrollIntoView，补一个桩记录被滚的元素。
    const scrolled: Element[] = [];
    Object.defineProperty(Element.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(function (this: Element) { scrolled.push(this); }),
    });
    invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "get_live_sessions_page"
        ? [session(163, "在跑的", { connected: true }), session(150, "旧任务")]
        : undefined));
    const { rerender } = render(<ChatSidebar activeId={163} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /旧任务/ });
    rerender(<ChatSidebar activeId={150} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await waitFor(() => {
      expect(scrolled).toContain(screen.getByRole("button", { name: /旧任务/ }));
    });
    delete (Element.prototype as { scrollIntoView?: unknown }).scrollIntoView;
  });

  it("macOS：侧栏内折叠钮上报 collapse 给父级", async () => {
    isMacMock.mockReturnValue(true);
    invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "get_live_sessions_page" ? [session(1, "任务A")] : undefined));
    const onCollapse = vi.fn();
    render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={onCollapse} />);
    await screen.findByRole("button", { name: /任务A/ });
    fireEvent.click(screen.getByRole("button", { name: "收起会话列表" }));
    // 折叠状态归 ChatWindow 持有（展开入口在标题栏），侧栏只上报意图。
    expect(onCollapse).toHaveBeenCalled();
  });

  it("非 macOS：侧栏内不渲染折叠钮（折叠/展开统一在顶栏，归 ChatWindow）", async () => {
    invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "get_live_sessions_page" ? [session(1, "任务A")] : undefined));
    render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /任务A/ });
    expect(screen.queryByRole("button", { name: "收起会话列表" })).toBeNull();
  });

  /** jsdom 里滚动尺寸恒为 0，手动装出「已经滚到底」的几何。 */
  function fakeScrolledToBottom(list: HTMLElement) {
    Object.defineProperty(list, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(list, "clientHeight", { value: 400, configurable: true });
    Object.defineProperty(list, "scrollTop", { value: 600, configurable: true });
  }

  // 150 行 + 按无障碍名查询在全量并发跑时很慢（单跑 1s 内、全量会超默认 1s 等待），
  // 放宽查询等待与用例上限：这里测的是翻页节奏，不是渲染速度。
  it("滚到底继续翻页，直到后端给不满一页", { timeout: 15_000 }, async () => {
    const slow = { timeout: 5_000 };
    const all = Array.from({ length: 150 }, (_, i) => session(1000 - i, `会话 ${i}`));
    const limits: number[] = [];
    invoke.mockImplementation((command: string, args: { limit: number }) => {
      if (command !== "get_live_sessions_page") return Promise.resolve();
      limits.push(args.limit);
      return Promise.resolve(all.slice(0, args.limit));
    });
    render(<ChatSidebar activeId={1000} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /会话 0/ }, slow);
    expect(limits).toEqual([60]);
    expect(screen.queryByRole("button", { name: /会话 60/ })).toBeNull();

    const list = screen.getByRole("navigation");
    fakeScrolledToBottom(list);

    fireEvent.scroll(list);
    await screen.findByRole("button", { name: /会话 60/ }, slow);
    expect(limits).toEqual([60, 120]);

    fireEvent.scroll(list);
    await screen.findByRole("button", { name: /会话 149/ }, slow);
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
    render(<ChatSidebar activeId={1000} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
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
    render(<ChatSidebar activeId={1000} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /会话 0/ });
    const list = screen.getByRole("navigation");
    fakeScrolledToBottom(list);

    fireEvent.scroll(list);
    await screen.findByRole("button", { name: /新活会话/ });
    // 只看会话条目：每条右侧还有个「⋯」菜单按钮，混进来会把索引全打乱。
    const names = within(list).getAllByRole("button")
      .filter((b) => b.className.includes("chat-sidebar-item") && !b.className.includes("menu"))
      .map((b) => b.textContent ?? "");
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
    render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    const dotOf = async (name: RegExp) =>
      (await screen.findByRole("button", { name })).querySelector(".chat-sidebar-dot");
    expect((await dotOf(/在跑/))?.className).toContain("is-running");
    // pending 优先于 waiting:它有明确的动作召唤。
    expect((await dotOf(/待审批/))?.className).toContain("is-pending");
    expect((await dotOf(/在等/))?.className).toContain("is-waiting");
    // 断开/已结束:图标置灰已表达不活跃,不再叠点。
    expect(await dotOf(/已断开/)).toBeNull();
  });

  it("待授权会话亮琥珀徽标并压过状态点,授权清掉后回落", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "get_live_sessions_page") {
        return Promise.resolve([
          session(1, "要授权", { connected: true, session: { id: 1, cc_session_id: "cc-1", status: "running" } } as Partial<LiveSession>),
          session(2, "没事"),
        ]);
      }
      return Promise.resolve();
    });
    const dotOf = async (name: RegExp) =>
      (await screen.findByRole("button", { name })).querySelector(".chat-sidebar-dot");
    const { rerender } = render(<ChatSidebar activeId={2} approvalAwaitingIds={new Set([1])} onSelect={() => {}} onCollapse={() => {}} />);
    // 授权比 running 更紧急：同一个 presence 位换亮琥珀徽标。
    expect((await dotOf(/要授权/))?.className).toContain("is-approval");
    expect(await dotOf(/没事/)).toBeNull();
    rerender(<ChatSidebar activeId={2} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await waitFor(() => expect(screen.getByRole("button", { name: /要授权/ }).querySelector(".chat-sidebar-dot")?.className).toContain("is-running"));
  });

  /**
   * 目录筛选走**后端**专用 cwd 参数(斜杠归一后精确匹配,不是 search 的子串 LIKE:
   * 子串会让另一种斜杠写法的会话整批消失,还把兄弟目录/标题命中漏进来):前端过滤
   * 只能过滤已加载的这一页,筛出来的清单是残缺的。切目录还必须把翻页状态清回首页,
   * 否则新目录一上来就顶着上一个目录翻到的 limit 拉几百条。
   */
  it("目录筛选把 cwd 传进后端专用参数,并把翻页重置回首页", async () => {
    const args: { search: string | null; cwd: string | null; limit: number }[] = [];
    invoke.mockImplementation((command: string, a: { search: string | null; cwd: string | null; limit: number }) => {
      if (command === "recent_cwds") return Promise.resolve(["C:/Users/me/workspace/meowo", "D:/tmp/scratch"]);
      if (command !== "get_live_sessions_page") return Promise.resolve();
      args.push({ search: a.search, cwd: a.cwd, limit: a.limit });
      return Promise.resolve(
        a.cwd === "D:/tmp/scratch"
          ? [session(9, "临时活儿", { cwd: "D:/tmp/scratch" })]
          : Array.from({ length: 60 }, (_, i) => session(100 - i, `会话 ${i}`)),
      );
    });
    render(<ChatSidebar activeId={100} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /会话 0/ });
    expect(args).toEqual([{ search: null, cwd: null, limit: 60 }]);

    // 先翻一页，让 limit 抬到 120——切目录后它必须回到 60。
    const list = screen.getByRole("navigation");
    fakeScrolledToBottom(list);
    fireEvent.scroll(list);
    await waitFor(() => expect(args).toHaveLength(2));
    expect(args[1]).toEqual({ search: null, cwd: null, limit: 120 });

    // 筛选从「工作空间」页签进入（对照 Kimi，视图菜单里不再有工作区筛选）：⋯ → 只看这个工作区的会话。
    fireEvent.click(screen.getByRole("tab", { name: "工作空间" }));
    fireEvent.click(screen.getAllByRole("button", { name: "选项" })[1]);
    fireEvent.click(await screen.findByRole("menuitem", { name: "只看这个工作区的会话" }));
    expect(await screen.findByRole("button", { name: /临时活儿/ })).toBeTruthy();
    expect(args[args.length - 1]).toEqual({ search: null, cwd: "D:/tmp/scratch", limit: 60 });
    // 上一个目录的会话不能残留。
    expect(screen.queryByRole("button", { name: /会话 0/ })).toBeNull();
    // 筛选生效时列表上方有一枚标签，点 × 退出筛选。
    expect(screen.getByText("只看 scratch")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "取消筛选" }));
    await waitFor(() => expect(args[args.length - 1]).toEqual({ search: null, cwd: null, limit: 60 }));
    expect(screen.queryByText("只看 scratch")).toBeNull();
  });

  /**
   * 分组不另起一套排序：组按「组内第一条会话的位置」排、组内保持原顺序，于是后端的
   * connected-first + 时间倒序原样透过来——用户原来靠什么找会话，分组后还靠什么找。
   * 组折起来时，组头的汇总点是这一组唯一的状态出口，必须取组内最强的召唤。
   */
  it("分组开关:组序由已有排序派生,组头汇总最强状态,可折叠并记住,当前组切入时展开一次", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "recent_cwds") return Promise.resolve([]);
      if (command !== "get_live_sessions_page") return Promise.resolve();
      return Promise.resolve([
        // 后端已排好序：活跃的在前。scratch 的活会话排在最前 → scratch 组就该在最前，
        // 哪怕 meowo 组的会话更多。
        session(1, "临时活儿", { cwd: "D:/tmp/scratch", connected: true, session: { id: 1, cc_session_id: "cc-1", status: "waiting" } } as Partial<LiveSession>),
        session(2, "改侧栏", { cwd: "C:/Users/me/workspace/meowo", connected: true, session: { id: 2, cc_session_id: "cc-2", status: "running" } } as Partial<LiveSession>),
        // 斜杠方向与大小写不同的同一目录，必须并进同一组而不是裂成两组。
        session(3, "发版", { cwd: "C:\\Users\\me\\workspace\\Meowo" }),
      ]);
    });
    render(<ChatSidebar activeId={2} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /改侧栏/ });
    // 默认关：一个像素都不改，没有组头。
    expect(screen.queryByRole("button", { name: /^scratch/ })).toBeNull();

    // 分组入口在筛选弹层里：单面板扁平化后选项直接内联,图标钮 → 选「按目录」。
    fireEvent.click(screen.getByRole("button", { name: "视图与排序" }));
    fireEvent.click(screen.getByRole("option", { name: "按工作区分组" }));
    const list = screen.getByRole("navigation");
    const heads = within(list).getAllByRole("button").filter((b) => b.className.includes("group-head"));
    // 组头对照 Kimi：文件夹图标 + 组名，不显示计数。
    expect(heads.map((h) => h.textContent)).toEqual(["scratch", "meowo"]);
    // scratch 组只有一条 waiting，meowo 组里 running 最强（另一条已断开不参与）。
    expect(heads[0].querySelector(".chat-sidebar-dot")?.className).toContain("is-waiting");
    expect(heads[1].querySelector(".chat-sidebar-dot")?.className).toContain("is-running");

    // 折 scratch 组（当前会话不在里面）：组内条目收走，组头汇总点还在——否则
    // 「折叠即失明」，用户会错过组里在跑的东西。
    fireEvent.click(heads[0]);
    expect(screen.queryByRole("button", { name: /临时活儿/ })).toBeNull();
    expect(heads[0].querySelector(".chat-sidebar-dot")?.className).toContain("is-waiting");

    // 当前打开的会话所在的组也能折（用户反馈折不起来）：主动点就照办。
    fireEvent.click(heads[1]);
    expect(screen.queryByRole("button", { name: /改侧栏/ })).toBeNull();

    // 侧栏收起时本组件整个卸载，目录组的折叠状态与开关都得落盘，回来才不是全展开。
    // 7C-6：再打开时当前会话所在的组若折着，自动展开一次——折过的组里藏着正开着的那条
    // 会话，用户一进来就找不到自己在哪。别的组的折叠照旧保留。
    cleanup();
    render(<ChatSidebar activeId={2} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    expect(await screen.findByRole("button", { name: /改侧栏/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /临时活儿/ })).toBeNull();
  });

  /** 批量清理旧会话（对照 Kimi 会话管理页）：右键「多选」是显式入口，Shift+点击连选，
   *  分组头的勾选圈整组增删，最后一次归档。 */
  it("多选:右键进入、Shift 连选、整组勾选后批量归档", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "recent_cwds") return Promise.resolve([]);
      if (command !== "get_live_sessions_page") return Promise.resolve();
      return Promise.resolve([
        session(1, "在跑的", { connected: true, session: { id: 1, cc_session_id: "cc-1", status: "running" } } as Partial<LiveSession>),
        session(2, "旧一"),
        session(3, "旧二"),
        session(4, "旧三"),
      ]);
    });
    render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    fireEvent.contextMenu(await screen.findByRole("button", { name: /旧一/ }));
    fireEvent.click(screen.getByRole("menuitem", { name: "多选" }));
    expect(screen.getByText("已选 1 条")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /旧三/ }), { shiftKey: true });
    expect(screen.getByText("已选 3 条")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "标记完成" }));
    await waitFor(() => expect(invoke.mock.calls.filter(([c]) => c === "set_archived").map(([, a]) => a.sessionId)).toEqual([2, 3, 4]));
  });

  /// 多选态下点整行（不必点右侧小勾选圈）就是增删，不进会话（用户反馈：点行首被带进了会话）；
  /// Enter/空格同口径。退出多选走底部「取消」。
  it("多选:点整行增删而不进会话,取消后点击恢复为打开会话", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "recent_cwds") return Promise.resolve([]);
      if (command !== "get_live_sessions_page") return Promise.resolve();
      return Promise.resolve([session(2, "旧一"), session(3, "旧二")]);
    });
    const onSelect = vi.fn();
    render(<ChatSidebar activeId={0} approvalAwaitingIds={new Set()} onSelect={onSelect} onCollapse={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: /旧一/ }), { ctrlKey: true });
    expect(screen.getByText("已选 1 条")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /旧二/ }));
    expect(screen.getByText("已选 2 条")).toBeTruthy();
    fireEvent.keyDown(screen.getByRole("button", { name: /旧一/ }), { key: "Enter" });
    expect(screen.getByText("已选 1 条")).toBeTruthy();
    expect(onSelect).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "取消多选" }));
    fireEvent.click(screen.getByRole("button", { name: /旧一/ }));
    expect(onSelect).toHaveBeenCalledWith(2);
  });

  it("多选:分组头勾选圈整组增删,部分选中为 mixed", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "recent_cwds") return Promise.resolve([]);
      if (command !== "get_live_sessions_page") return Promise.resolve();
      return Promise.resolve([
        session(1, "在跑的", { cwd: "C:/w/live", connected: true, session: { id: 1, cc_session_id: "cc-1", status: "running" } } as Partial<LiveSession>),
        session(2, "结束一", { cwd: "C:/w/old" }),
        session(3, "结束二", { cwd: "C:/w/old" }),
      ]);
    });
    localStorage.setItem("meowo-chat-sidebar-group-mode", "dir");
    render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    // 不在多选态时组头不出勾选圈。
    await screen.findByRole("button", { name: /结束一/ });
    expect(screen.queryByRole("checkbox", { name: /整组/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /结束一/ }), { ctrlKey: true });
    const group = screen.getByRole("checkbox", { name: "选中「old」整组" });
    expect(group.getAttribute("aria-checked")).toBe("mixed");
    fireEvent.click(group);
    expect(screen.getByText("已选 2 条")).toBeTruthy();
    expect(group.getAttribute("aria-checked")).toBe("true");
    // 另一组不受影响；再点整组取消，选中清空即退出多选。
    expect(screen.getByRole("checkbox", { name: "选中「live」整组" }).getAttribute("aria-checked")).toBe("false");
    fireEvent.click(group);
    expect(screen.queryByText(/已选/)).toBeNull();
  });

  /** 排序入口在视图与排序弹层（对照 Kimi 只有两种）。默认「手动排序」：后端按手动键分页（没拖过 =
   *  创建时间倒序），前端原样采用，在跑的会话不因新事件换位置（用户反馈列表顶部一直乱动）；
   *  切到「按最近活动」后端换回活跃顺序，选择与分组一样落盘。 */
  it("排序:默认手动排序交给后端,可切按最近活动,选择落盘", async () => {
    const orders: unknown[] = [];
    invoke.mockImplementation((command: string, args?: { order?: string }) => {
      if (command === "recent_cwds") return Promise.resolve([]);
      if (command !== "get_live_sessions_page") return Promise.resolve();
      orders.push(args?.order);
      return Promise.resolve([
        // 后端给的手动顺序：alpha 最新建。
        session(2, "alpha", { session: { id: 2, cc_session_id: "cc-2", status: "ended", started_at: 300 } } as Partial<LiveSession>),
        session(3, "", { session: { id: 3, cc_session_id: "cc-3", status: "ended", started_at: 200 } } as Partial<LiveSession>),
        session(1, "beta", { session: { id: 1, cc_session_id: "cc-1", status: "ended", started_at: 100 } } as Partial<LiveSession>),
      ]);
    });
    render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /beta/ });
    const itemNames = () =>
      within(screen.getByRole("navigation"))
        .getAllByRole("button")
        // "chat-sidebar-item" 子串会连 ⋯ 按钮（chat-sidebar-item-menu）一起捞进来——
        // 首帧默认已改 button 模式，⋯ 常驻，按有无标题文本过滤。
        .map((b) => b.querySelector(".chat-sidebar-name-text")?.textContent)
        .filter((n): n is string => n != null);
    // 默认「手动排序」：请求带 order=manual，顺序原样采用后端的。
    expect(orders[0]).toBe("manual");
    expect(itemNames()).toEqual(["alpha", "等待首次输入", "beta"]);

    fireEvent.click(screen.getByRole("button", { name: "视图与排序" }));
    // 菜单照 Kimi：视图两种、排序两种。
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual(["平铺列表", "按工作区分组", "手动排序", "按最近活动"]);
    fireEvent.click(screen.getByRole("option", { name: "按最近活动" }));
    await waitFor(() => expect(orders.length).toBeGreaterThan(1));
    expect(orders[orders.length - 1]).toBeUndefined();
    expect(localStorage.getItem("meowo-chat-sidebar-sort-mode")).toBe("recent");
  });

  /** 「工作空间」页签（对照 Kimi Code）：列出会话用过的目录，点一行就在那里新建会话；
   *  「⋯ → 只看这个工作区的会话」切回进行中并按它筛选。顶部搜索在这个页签里过滤工作区。 */
  it("工作空间页签:列出工作区,点击新建,菜单里可按工作区筛选会话", async () => {
    const pages: unknown[] = [];
    invoke.mockImplementation((command: string, args?: unknown) => {
      if (command === "recent_cwds") return Promise.resolve(["C:/w/meowo", "C:/w/reverse-bot-gui"]);
      if (command === "get_live_sessions_page") { pages.push(args); return Promise.resolve([session(1, "甲")]); }
      return Promise.resolve();
    });
    render(<ChatSidebar activeId={0} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /甲/ });
    fireEvent.click(screen.getByRole("tab", { name: "工作空间" }));
    expect(screen.getByText("C:/w/meowo")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /甲/ })).toBeNull();
    // 搜索框在这个页签里过滤工作区。
    fireEvent.change(screen.getByRole("textbox", { name: "搜索" }), { target: { value: "reverse" } });
    expect(screen.queryByText("C:/w/meowo")).toBeNull();
    fireEvent.change(screen.getByRole("textbox", { name: "搜索" }), { target: { value: "" } });
    fireEvent.click(screen.getByText("C:/w/meowo"));
    expect(invoke).toHaveBeenCalledWith("open_new_session_window", { cwd: "C:/w/meowo", provider: undefined });
    // 「添加工作区…」（对照 Kimi）：系统目录框选一个没开过会话的目录，直接带着它去新建会话。
    dialogOpen.mockResolvedValueOnce("D:/new-project");
    fireEvent.click(screen.getByRole("button", { name: "添加工作区…" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("open_new_session_window", { cwd: "D:/new-project", provider: undefined }));
    expect(dialogOpen).toHaveBeenCalledWith({ directory: true });
    fireEvent.click(screen.getAllByRole("button", { name: "选项" })[1]);
    fireEvent.click(await screen.findByRole("menuitem", { name: "只看这个工作区的会话" }));
    expect(screen.getByRole("tab", { name: "进行中" }).getAttribute("aria-selected")).toBe("true");
    await waitFor(() => expect(pages[pages.length - 1]).toMatchObject({ cwd: "C:/w/reverse-bot-gui" }));
  });

  /** 用户要求（2026-11-09）：工作区可以「从列表中移除」——只是不显示，磁盘不动；点了立刻消失，
   *  后端记下后重拉清单。分组头上的同一个菜单不出这一项（那里移除了也看不出效果）。 */
  it("工作空间页签:从列表中移除一个工作区,立刻消失并通知后端", async () => {
    let listed = ["C:/w/meowo", "C:/w/old"];
    invoke.mockImplementation((command: string, args?: unknown) => {
      if (command === "recent_cwds") return Promise.resolve(listed);
      if (command === "get_live_sessions_page") return Promise.resolve([session(1, "甲")]);
      if (command === "hide_workspace") { listed = ["C:/w/meowo"]; return Promise.resolve(); }
      void args;
      return Promise.resolve();
    });
    render(<ChatSidebar activeId={0} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /甲/ });
    fireEvent.click(screen.getByRole("tab", { name: "工作空间" }));
    expect(screen.getByText("C:/w/old")).toBeTruthy();
    fireEvent.click(screen.getAllByRole("button", { name: "选项" })[1]);
    fireEvent.click(await screen.findByRole("menuitem", { name: "从列表中移除" }));
    expect(screen.queryByText("C:/w/old")).toBeNull();
    expect(invoke).toHaveBeenCalledWith("hide_workspace", { cwd: "C:/w/old" });
    await waitFor(() => expect(invoke.mock.calls.filter(([c]) => c === "recent_cwds").length).toBeGreaterThan(1));
    expect(screen.getByText("C:/w/meowo")).toBeTruthy();
  });

  /** 工作区右键菜单（对照 Kimi 的 onContextmenu）：工作空间页签的目录行、会话列表的分组头都能右键，
   *  菜单同一个；分组头上选「只看这个工作区的会话」即按它筛选。 */
  it("工作区右键菜单:目录行与分组头都能右键,分组头可按工作区筛选", async () => {
    localStorage.setItem("meowo-chat-sidebar-group-mode", "dir");
    const pages: unknown[] = [];
    invoke.mockImplementation((command: string, args?: unknown) => {
      if (command === "recent_cwds") return Promise.resolve(["C:/w/meowo"]);
      if (command === "get_live_sessions_page") { pages.push(args); return Promise.resolve([session(1, "甲", { cwd: "C:/w/meowo" })]); }
      return Promise.resolve();
    });
    render(<ChatSidebar activeId={0} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /甲/ });
    // 分组头右键 → 工作区菜单 → 只看这个工作区的会话。
    const head = within(screen.getByRole("navigation")).getAllByRole("button").find((b) => b.className.includes("group-head"))!;
    fireEvent.contextMenu(head, { clientX: 40, clientY: 60 });
    fireEvent.click(await screen.findByRole("menuitem", { name: "只看这个工作区的会话" }));
    await waitFor(() => expect(pages[pages.length - 1]).toMatchObject({ cwd: "C:/w/meowo" }));
    // 工作空间页签的目录行右键 → 同一个菜单，选「在此工作区新建会话」。
    fireEvent.click(screen.getByRole("tab", { name: "工作空间" }));
    fireEvent.contextMenu(screen.getByText("C:/w/meowo"), { clientX: 40, clientY: 60 });
    fireEvent.click(await screen.findByRole("menuitem", { name: "在此工作区新建会话" }));
    expect(invoke).toHaveBeenCalledWith("open_new_session_window", { cwd: "C:/w/meowo", provider: undefined });
  });

  /** 拖分组头调整工作区顺序（对照 Kimi 可拖的分组头）：设置里的 workspace_order 决定组序；
   *  拖完本地先排、整份顺序写回设置，松手那次 click 不折叠。 */
  it("工作区分组:按设置里的顺序排,拖分组头改顺序并写回设置", async () => {
    localStorage.setItem("meowo-chat-sidebar-group-mode", "dir");
    const saved: unknown[] = [];
    invoke.mockImplementation((command: string, args?: { order?: string[] }) => {
      if (command === "get_live_sessions_page") return Promise.resolve([session(1, "甲", { cwd: "C:/w/a" }), session(2, "乙", { cwd: "C:/w/b" }), session(3, "丙", { cwd: "C:/w/c" })]);
      if (command === "get_settings") return Promise.resolve({ card_menu_mode: "button", workspace_order: ["c:/w/c", "c:/w/a", "c:/w/b"] });
      if (command === "set_settings") throw new Error("组序只能经 set_workspace_order 写");
      if (command === "set_workspace_order") { saved.push(args?.order); return Promise.resolve(); }
      return Promise.resolve();
    });
    render(<ChatSidebar activeId={0} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /甲/ });
    const heads = () => within(screen.getByRole("navigation")).getAllByRole("button").filter((b) => b.className.includes("group-head"));
    await waitFor(() => expect(heads().map((h) => h.textContent)).toEqual(["c", "a", "b"]));
    // 把 c 拖到 b 下面。
    const last = heads()[2].closest<HTMLElement>("[data-group-key]")!;
    last.getBoundingClientRect = () => ({ top: 100, height: 40, bottom: 140, left: 0, right: 200, width: 200, x: 0, y: 100, toJSON: () => ({}) });
    const original = document.elementFromPoint;
    document.elementFromPoint = () => last;
    try {
      fireEvent.pointerDown(heads()[0], { button: 0, pointerType: "mouse", clientX: 10, clientY: 10 });
      fireEvent.pointerMove(window, { clientX: 10, clientY: 130 });
      await waitFor(() => expect(last.className).toContain("is-drop-after"));
      // 拖动中有一份组头的影子跟着鼠标，松手就收掉。
      expect(document.querySelectorAll(".sidebar-drag-ghost")).toHaveLength(1);
      expect(document.querySelector(".sidebar-drag-ghost")?.textContent).toBe("c");
      fireEvent.pointerUp(window, { clientX: 10, clientY: 130 });
      expect(document.querySelector(".sidebar-drag-ghost")).toBeNull();
      fireEvent.click(heads()[0]);
    } finally {
      document.elementFromPoint = original;
    }
    expect(heads().map((h) => h.textContent)).toEqual(["a", "b", "c"]);
    await waitFor(() => expect(saved).toEqual([["c:/w/a", "c:/w/b", "c:/w/c"]]));
    // 松手那次 click 没有把组折起来。
    expect(screen.getByRole("button", { name: /甲/ })).toBeTruthy();
  });

  /** 「会话」小节的全部折叠 / 展开（对照 Kimi Code）：只在分组时出现，一键折起所有组，再点全部展开。 */
  it("全部折叠:一键折起所有分组,再点全部展开", async () => {
    localStorage.setItem("meowo-chat-sidebar-group-mode", "dir");
    invoke.mockImplementation((command: string) => command === "get_live_sessions_page"
      ? Promise.resolve([session(1, "甲", { cwd: "C:/w/a" }), session(2, "乙", { cwd: "C:/w/b" })])
      : Promise.resolve());
    render(<ChatSidebar activeId={0} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByRole("button", { name: /甲/ });
    fireEvent.click(screen.getByRole("button", { name: "全部折叠" }));
    expect(screen.queryByRole("button", { name: /甲/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /乙/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "全部展开" }));
    expect(screen.getByRole("button", { name: /甲/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /乙/ })).toBeTruthy();
  });

  /** 手动排序下按住拖动调位置（对照 Kimi Code）：落下即本地先挪，再把新邻居交给后端。 */
  it("手动排序:拖到另一条下方,本地先挪并把新邻居写回后端", async () => {
    const moves: unknown[] = [];
    invoke.mockImplementation((command: string, args?: unknown) => {
      if (command === "get_live_sessions_page") return Promise.resolve([session(3, "丙"), session(2, "乙"), session(1, "甲")]);
      if (command === "move_session_rank") { moves.push(args); return Promise.resolve(); }
      return Promise.resolve();
    });
    render(<ChatSidebar activeId={3} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    const row = async (name: string) => (await screen.findByRole("button", { name: new RegExp(name) })) as HTMLElement;
    const first = await row("丙");
    const last = await row("甲");
    // jsdom 没有布局：落点行与它的位置手动给。
    last.getBoundingClientRect = () => ({ top: 100, height: 40, bottom: 140, left: 0, right: 200, width: 200, x: 0, y: 100, toJSON: () => ({}) });
    const original = document.elementFromPoint;
    document.elementFromPoint = () => last;
    try {
      fireEvent.pointerDown(first, { button: 0, pointerType: "mouse", clientX: 10, clientY: 10 });
      fireEvent.pointerMove(window, { clientX: 10, clientY: 130 });
      await waitFor(() => expect(last.className).toContain("is-drop-after"));
      // 会话行也有影子（实拍：只有分组有）；影子不带选中态，松手收掉。
      const ghost = document.querySelector(".sidebar-drag-ghost");
      expect(ghost?.textContent).toContain("丙");
      expect(ghost?.classList.contains("is-active")).toBe(false);
      fireEvent.pointerUp(window, { clientX: 10, clientY: 130 });
      expect(document.querySelector(".sidebar-drag-ghost")).toBeNull();
    } finally {
      document.elementFromPoint = original;
    }
    expect(moves).toEqual([{ sessionId: 3, aboveId: 1, belowId: null }]);
    const names = within(screen.getByRole("navigation")).getAllByRole("button")
      .map((b) => b.querySelector(".chat-sidebar-name-text")?.textContent).filter(Boolean);
    expect(names).toEqual(["乙", "甲", "丙"]);
  });

  /** 手机长按起拖：一按就拖会和列表滚动抢手势，长按到点才拖；没到点就挪了是在滚动；长按没挪松手开菜单。 */
  describe("触屏长按", () => {
    const setup = async () => {
      const moves: unknown[] = [];
      invoke.mockImplementation((command: string, args?: unknown) => {
        if (command === "get_live_sessions_page") return Promise.resolve([session(3, "丙"), session(2, "乙"), session(1, "甲")]);
        if (command === "move_session_rank") { moves.push(args); return Promise.resolve(); }
        return Promise.resolve();
      });
      render(<ChatSidebar activeId={3} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
      const first = (await screen.findByRole("button", { name: /丙/ })) as HTMLElement;
      const last = (await screen.findByRole("button", { name: /甲/ })) as HTMLElement;
      last.getBoundingClientRect = () => ({ top: 100, height: 40, bottom: 140, left: 0, right: 200, width: 200, x: 0, y: 100, toJSON: () => ({}) });
      return { moves, first, last };
    };
    const hold = () => act(() => new Promise((resolve) => setTimeout(resolve, 400)));
    let original: typeof document.elementFromPoint;
    beforeEach(() => { original = document.elementFromPoint; });
    afterEach(() => { document.elementFromPoint = original; });

    it("长按到点后拖动,照样调位置", async () => {
      const { moves, first, last } = await setup();
      document.elementFromPoint = () => last;
      fireEvent.pointerDown(first, { button: 0, pointerType: "touch", clientX: 10, clientY: 10 });
      await hold();
      expect(document.querySelector(".sidebar-drag-ghost")).not.toBeNull();
      fireEvent.pointerMove(window, { clientX: 10, clientY: 130 });
      await waitFor(() => expect(last.className).toContain("is-drop-after"));
      fireEvent.pointerUp(window, { clientX: 10, clientY: 130 });
      expect(moves).toEqual([{ sessionId: 3, aboveId: 1, belowId: null }]);
    });

    it("没到点就挪了是在滚列表,不拖", async () => {
      const { moves, first, last } = await setup();
      document.elementFromPoint = () => last;
      fireEvent.pointerDown(first, { button: 0, pointerType: "touch", clientX: 10, clientY: 10 });
      fireEvent.pointerMove(window, { clientX: 10, clientY: 60 });
      await hold();
      expect(document.querySelector(".sidebar-drag-ghost")).toBeNull();
      fireEvent.pointerUp(window, { clientX: 10, clientY: 130 });
      expect(moves).toEqual([]);
    });

    it("长按没挪就松手,弹出会话菜单;Android 补发的 contextmenu 不再开第二次", async () => {
      const { moves, first } = await setup();
      fireEvent.pointerDown(first, { button: 0, pointerType: "touch", clientX: 10, clientY: 10 });
      await hold();
      fireEvent.contextMenu(first, { clientX: 10, clientY: 10 });
      expect(screen.queryByRole("menuitem", { name: "标记完成" })).toBeNull();
      fireEvent.pointerUp(window, { clientX: 10, clientY: 10 });
      expect(await screen.findByRole("menuitem", { name: "标记完成" })).toBeTruthy();
      expect(document.querySelector(".sidebar-drag-ghost")).toBeNull();
      expect(moves).toEqual([]);
    });
  });

  /**
   * 会话菜单与看板卡片是同一个 CardContextMenu、同一批动作。侧栏条目从 <button> 改成
   * role=button 的 <div> 是为了能在里面放「⋯」按钮（button 套 button 非法）。
   */
  describe("会话菜单", () => {
    const listed = [session(1, "改侧栏", { connected: true }), session(2, "旧任务")];
    /** menuMode 跟随「卡片菜单」设置。首帧占位已与生产默认(button)对齐，右键模式必须
     *  显式注入 context 设置——本组右键用例默认注入并等设置套用。 */
    const renderList = async (activeId = 1, cardMenuMode: "button" | "context" = "context") => {
      invoke.mockImplementation((command: string) => {
        if (command === "get_live_sessions_page") return Promise.resolve(listed);
        if (command === "get_settings") return Promise.resolve({ card_menu_mode: cardMenuMode });
        return Promise.resolve();
      });
      render(<ChatSidebar activeId={activeId} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
      await screen.findByRole("button", { name: /改侧栏/ });
      // 等注入的 card_menu_mode 套用（getSettings 是异步的，右键在此之前不生效）。
      await act(async () => {});
    };

    it("右键条目弹出菜单，归档走 set_archived 并把条目乐观摘掉", async () => {
      await renderList();
      fireEvent.contextMenu(screen.getByRole("button", { name: /旧任务/ }));
      fireEvent.click(await screen.findByRole("menuitem", { name: "标记完成" }));
      expect(invoke).toHaveBeenCalledWith("set_archived", { sessionId: 2, archived: true });
      // 侧栏取的是 "all"（不含已归档）：点完当场消失，不等下一轮刷新。
      await waitFor(() => expect(screen.queryByRole("button", { name: /旧任务/ })).toBeNull());
      // 不再弹「已完成，可在侧栏『已完成』找回 · 撤销」条（用户要求）。
      await new Promise((r) => setTimeout(r, 30));
      expect(screen.queryByText(/可在侧栏「已完成」找回/)).toBeNull();
      expect(screen.queryByRole("button", { name: "撤销" })).toBeNull();
    });

    /// 已归档 = 已收纳，列表里就不该再有它——正开着它的对话也一样，且右边要跟着换走，
    /// 否则「收起来了却还摊在桌上」。
    it("归档当前会话：摘掉它并切到下一条", async () => {
      const onSelect = vi.fn();
      invoke.mockImplementation((command: string) =>
        Promise.resolve(command === "get_live_sessions_page" ? listed
          : command === "get_settings" ? { card_menu_mode: "context" } : undefined));
      render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={onSelect} onCollapse={() => {}} />);
      await screen.findByRole("button", { name: /改侧栏/ });
      await act(async () => {});
      fireEvent.contextMenu(screen.getByRole("button", { name: /改侧栏/ }));
      fireEvent.click(await screen.findByRole("menuitem", { name: "标记完成" }));
      expect(invoke).toHaveBeenCalledWith("set_archived", { sessionId: 1, archived: true });
      expect(onSelect).toHaveBeenCalledWith(2);
      await waitFor(() => expect(screen.queryByRole("button", { name: /改侧栏/ })).toBeNull());
    });

    /// 归档的不是当前会话时不该乱切——用户只是在收拾别的会话。
    it("归档别的会话不动当前对话", async () => {
      const onSelect = vi.fn();
      invoke.mockImplementation((command: string) =>
        Promise.resolve(command === "get_live_sessions_page" ? listed
          : command === "get_settings" ? { card_menu_mode: "context" } : undefined));
      render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={onSelect} onCollapse={() => {}} />);
      await screen.findByRole("button", { name: /改侧栏/ });
      await act(async () => {});
      fireEvent.contextMenu(screen.getByRole("button", { name: /旧任务/ }));
      fireEvent.click(await screen.findByRole("menuitem", { name: "标记完成" }));
      expect(onSelect).not.toHaveBeenCalled();
    });

    /// 触发方式与看板共用 card_menu_mode，且**二选一**：两个入口并存时，改了设置的人
    /// 会以为没生效（另一个还在），没改的人平白多一个不知从哪来的按钮。
    it("按钮模式下有「⋯」，右键同样弹菜单（惯例恒可用）", async () => {
      await renderList(1, "button");
      // 「⋯」嵌在 role=button 的条目里、对读屏 aria-hidden（axe nested-interactive），按标签找。
      const menus = await screen.findAllByLabelText("更多操作");
      expect(menus).toHaveLength(2);
      // 右键恒开菜单：card_menu_mode 只决定「⋯」是否常驻，不再关掉右键这条路——
      // 「右键出上下文菜单」是通用惯例，随设置失效会变成无反馈的黑洞。
      const contextEvent = fireEvent.contextMenu(screen.getByRole("button", { name: /旧任务/ }));
      expect(contextEvent).toBe(false); // 已 preventDefault（自绘菜单接管）
      expect(await screen.findByRole("menuitem", { name: "标记完成" })).toBeTruthy();
    });

    /// 菜单吃掉的 Esc 必须标记 preventDefault：对话窗有窗口级的「Esc = 拒绝审批」监听
    /// （以 defaultPrevented 让路），而 document 冒泡在 window 之前——不标记的话，
    /// 关个右键菜单的同一次按键会顺手把 agent 的审批请求拒了。
    it("Esc 关菜单时吃掉这次按键，不落到「拒绝审批」上", async () => {
      await renderList();
      fireEvent.contextMenu(screen.getByRole("button", { name: /旧任务/ }));
      expect(await screen.findByRole("menuitem", { name: "标记完成" })).toBeTruthy();
      // fireEvent 返回 !defaultPrevented：false = 这次按键已被菜单消费掉。
      expect(fireEvent.keyDown(document, { key: "Escape" })).toBe(false);
      await waitFor(() => expect(screen.queryByRole("menuitem", { name: "标记完成" })).toBeNull());
    });

    it("右键模式下不渲染「⋯」按钮", async () => {
      await renderList(1, "context");
      fireEvent.contextMenu(screen.getByRole("button", { name: /旧任务/ }));
      expect(await screen.findByRole("menuitem", { name: "标记完成" })).toBeTruthy();
      expect(screen.queryByLabelText("更多操作")).toBeNull();
    });

    it("「⋯」按钮同样打开菜单，重命名就地编辑并写回后端", async () => {
      await renderList(1, "button");
      const menus = await screen.findAllByLabelText("更多操作");
      fireEvent.click(menus[1]);
      fireEvent.click(await screen.findByRole("menuitem", { name: "重命名" }));
      const input = screen.getByPlaceholderText("输入名称，回车保存");
      fireEvent.change(input, { target: { value: "改个名" } });
      fireEvent.keyDown(input, { key: "Enter" });
      expect(invoke).toHaveBeenCalledWith("rename_session", {
        cwd: "C:/Users/me/workspace/meowo", sessionId: "cc-2", title: "改个名", provider: "claude",
      });
    });

    it("便签写完看得见，失败给出提示", async () => {
      invoke.mockImplementation((command: string) => {
        if (command === "get_live_sessions_page") return Promise.resolve([session(2, "旧任务", { note: "先跑一遍测试" })]);
        if (command === "set_session_note") return Promise.reject(new Error("db busy"));
        if (command === "get_settings") return Promise.resolve({ card_menu_mode: "context" });
        return Promise.resolve();
      });
      render(<ChatSidebar activeId={0} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
      // 已有便签直接显示在条目里——写了看不见的备忘等于没写。
      expect(await screen.findByText("先跑一遍测试")).toBeTruthy();
      await act(async () => {});
      fireEvent.contextMenu(screen.getByRole("button", { name: /旧任务/ }));
      fireEvent.click(await screen.findByRole("menuitem", { name: "编辑便签" }));
      const input = screen.getByPlaceholderText("写点备忘…");
      fireEvent.change(input, { target: { value: "改成先看日志" } });
      fireEvent.keyDown(input, { key: "Enter" });
      expect(await screen.findByText("便签保存失败，请稍后再试。")).toBeTruthy();
    });

    it("置顶与看板共用同一份存储（键沿用 meowo-starred）", async () => {
      await renderList();
      fireEvent.contextMenu(screen.getByRole("button", { name: /旧任务/ }));
      fireEvent.click(await screen.findByRole("menuitem", { name: "置顶" }));
      expect(JSON.parse(localStorage.getItem("meowo-starred") ?? "[]")).toEqual(["cc-2"]);
    });

    /// 「置顶」得真的顶上去：只亮标记不动位置，等于名不副实。
    it("置顶的会话排到最前，其余相对次序不变", async () => {
      await renderList();
      const names = () => screen.getAllByRole("button")
        .filter((b) => b.className.includes("chat-sidebar-item") && !b.className.includes("menu"))
        .map((b) => b.textContent ?? "");
      expect(names()[0]).toContain("改侧栏");
      fireEvent.contextMenu(screen.getByRole("button", { name: /旧任务/ }));
      fireEvent.click(await screen.findByRole("menuitem", { name: "置顶" }));
      await waitFor(() => expect(names()[0]).toContain("旧任务"));
      expect(names()[1]).toContain("改侧栏");
    });

    /// 置顶的会话已断开时也不该被折进「显示 N 个未运行会话」——用户刚亲手顶上来的。
    it("分组视图里置顶的会话不被折进未运行", async () => {
      localStorage.setItem("meowo-chat-sidebar-grouped", "1");
      localStorage.setItem("meowo-starred", JSON.stringify(["cc-2"]));
      invoke.mockImplementation((command: string) =>
        Promise.resolve(command === "get_live_sessions_page" ? listed : undefined));
      render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
      expect(await screen.findByRole("button", { name: /旧任务/ })).toBeTruthy();
      expect(screen.queryByRole("button", { name: /显示 .* 个未运行会话/ })).toBeNull();
    });
  });

  it("survives a backend without the sessions command", async () => {
    // demo/旧后端对未知命令返回 undefined：侧栏必须静默降级为空列表，不能崩掉整个窗口。
    invoke.mockResolvedValue(undefined);
    render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    expect(await screen.findByText("暂无会话")).toBeTruthy();
  });

  it("空列表直接给新建入口", async () => {
    invoke.mockResolvedValue(undefined);
    render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByText("暂无会话");
    // 头部也有一个「新建会话」，空态里的是第二个
    const buttons = screen.getAllByRole("button", { name: "新建会话" });
    expect(buttons.length).toBe(2);
    fireEvent.click(buttons[1]);
    expect(invoke).toHaveBeenCalledWith("open_new_session_window");
  });

  describe("可访问性:roving tabindex + 单一汇总播报(C-17/G-16)", () => {
    const renderThree = async () => {
      invoke.mockImplementation((command: string) => {
        if (command === "get_live_sessions_page") {
          return Promise.resolve([
            session(1, "甲", { connected: true, session: { id: 1, cc_session_id: "cc-1", status: "running" } } as Partial<LiveSession>),
            session(2, "乙", { connected: true, session: { id: 2, cc_session_id: "cc-2", status: "waiting" } } as Partial<LiveSession>),
            session(3, "丙"),
          ]);
        }
        return Promise.resolve();
      });
      const { container } = render(<ChatSidebar activeId={2} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
      await screen.findByRole("button", { name: /甲/ });
      return container;
    };

    it("列表只占一个 Tab 停靠点(选中条 0、其余 -1),↑↓/Home/End 在条目间搬焦点", async () => {
      await renderThree();
      const first = screen.getByRole("button", { name: /甲/ });
      const second = screen.getByRole("button", { name: /乙/ });
      const third = screen.getByRole("button", { name: /丙/ });
      expect(first.getAttribute("tabindex")).toBe("-1");
      expect(second.getAttribute("tabindex")).toBe("0"); // 当前选中的那条
      expect(third.getAttribute("tabindex")).toBe("-1");
      second.focus();
      fireEvent.keyDown(second, { key: "ArrowDown" });
      expect(document.activeElement).toBe(third);
      fireEvent.keyDown(third, { key: "ArrowDown" }); // 到底回卷
      expect(document.activeElement).toBe(first);
      fireEvent.keyDown(first, { key: "ArrowUp" });
      expect(document.activeElement).toBe(third);
      fireEvent.keyDown(third, { key: "Home" });
      expect(document.activeElement).toBe(first);
      fireEvent.keyDown(first, { key: "End" });
      expect(document.activeElement).toBe(third);
    });

    it("状态点是 role=img 而非独立 live region,全侧栏只剩一个汇总 role=status", async () => {
      const container = await renderThree();
      const dot = screen.getByRole("button", { name: /甲/ }).querySelector(".chat-sidebar-dot")!;
      expect(dot.getAttribute("role")).toBe("img");
      const liveRegions = container.querySelectorAll('[role="status"]');
      expect(liveRegions.length).toBe(1);
      expect(liveRegions[0].className).toContain("chat-sidebar-sronly");
      expect(liveRegions[0].textContent).toBe("会话状态汇总：0 个待批准，1 个等待输入，0 个出错");
    });
  });
});
