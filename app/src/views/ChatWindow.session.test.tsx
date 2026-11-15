// 加载更早、侧栏切换与归档、命令面板、接管与恢复门卡、后台会话、审批卡。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke, eventListeners, terminalMounts, b64utf8, respondWithHistory } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";
import { chatUi } from "../test/agents";

describe("ChatWindow", () => {
  it("offers to load earlier messages when the first read was truncated", async () => {
    window.history.replaceState({}, "", "/?sessionId=33");
    const truncated = {
      sessionId: 33, title: "长会话", status: "running", provider: "claude", cwd: null,
      supported: true, offset: 500, reset: false, pendingReview: null,
      model: null, contextPct: null, contextWindow: null, currentActivity: null,
      hasMore: true, earliest: 120,
      items: [{ type: "user_text", id: "recent", timestamp: null, text: "最近的消息" }],
    };
    // 增量轮询恒为 hasMore:false——提示不能因此闪掉。
    const incremental = { ...truncated, items: [], hasMore: false, earliest: 0 };
    let firstRead = true;
    invoke.mockImplementation((command: string, args: { before?: number }) => {
      if (command === "get_chat_history") {
        // 向上翻页：按 before（= 上一屏的 earliest）只回更早的一屏，可一直点到文件头。
        if (args?.before === 120) {
          return Promise.resolve({
            ...truncated, hasMore: true, earliest: 40,
            items: [{ type: "user_text", id: "mid", timestamp: null, text: "中间的消息" }],
          });
        }
        if (args?.before === 40) {
          return Promise.resolve({
            ...truncated, hasMore: false, earliest: 0,
            items: [{ type: "user_text", id: "old", timestamp: null, text: "很早以前的消息" }],
          });
        }
        if (firstRead) { firstRead = false; return Promise.resolve(truncated); }
        return Promise.resolve(incremental);
      }
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findByText("最近的消息");
    const button = await screen.findByRole("button", { name: "加载更早的对话" });
    // 被裁掉的消息此刻不在 DOM 里——这正是首屏省下的成本。
    expect(screen.queryByText("很早以前的消息", { selector: ".chat-text" })).toBeNull();

    // 第一屏：增量前插，不整段重读（无 full 参数）。
    fireEvent.click(button);
    expect(await screen.findByText("中间的消息", { selector: ".chat-text" })).toBeTruthy();
    expect(invoke).toHaveBeenCalledWith("get_chat_history", expect.objectContaining({ sessionId: 33, before: 120 }));
    expect(invoke).not.toHaveBeenCalledWith("get_chat_history", expect.objectContaining({ full: true }));
    // 还没到文件头：入口保留，可以再点。
    const again = await screen.findByRole("button", { name: "加载更早的对话" });

    // 第二屏翻到文件头（hasMore=false）：提示消失，已有消息不重复插入。
    fireEvent.click(again);
    expect(await screen.findByText("很早以前的消息", { selector: ".chat-text" })).toBeTruthy();
    await waitFor(() => expect(screen.queryByRole("button", { name: "加载更早的对话" })).toBeNull());
    expect(screen.getAllByText("最近的消息", { selector: ".chat-text" })).toHaveLength(1);
    expect(screen.getAllByText("中间的消息", { selector: ".chat-text" })).toHaveLength(1);
  });

  it("delays the loading skeleton until the first read takes >150ms", async () => {
    window.history.replaceState({}, "", "/?sessionId=44");
    let resolveHistory!: (value: unknown) => void;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") {
        return new Promise((resolve) => { resolveHistory = resolve; });
      }
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      return Promise.resolve();
    });
    const { container } = render(<ChatWindow />);
    // 首帧不得有占位：快读（<150ms）不该闪加载态造成三段跳。
    expect(container.querySelector(".chat-loading")).toBeNull();
    await new Promise((resolve) => setTimeout(resolve, 200));
    await waitFor(() => expect(container.querySelector(".chat-loading")).not.toBeNull());
    // 数据到达后加载态消失、内容落地。
    resolveHistory({
      sessionId: 44, title: "慢会话", status: "running", provider: "claude", cwd: null,
      supported: true, offset: 0, reset: false, pendingReview: null,
      connected: true, hasMore: false, earliest: 0,
      items: [{ type: "user_text", id: "u1", timestamp: null, text: "慢会话内容" }],
    });
    await screen.findByText("慢会话内容");
    expect(container.querySelector(".chat-loading")).toBeNull();
  });

  it("keeps the terminal mounted across tab switches", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    respondWithHistory({
      sessionId: 7, title: "保活", status: "running", provider: "claude", cwd: null,
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    });
    terminalMounts.count = 0;
    render(<ChatWindow />);
    await screen.findByText("保活");
    // broker 报告活跃 PTY 后即在屏幕外挂载一次，以便无需切 tab 也能还原 ANSI 选择器。
    await waitFor(() => expect(terminalMounts.count).toBe(1));

    fireEvent.click(screen.getByRole("button", { name: "终端" }));
    expect(screen.getByText("PTY 7")).toBeTruthy();
    expect(terminalMounts.count).toBe(1);

    // 切回对话再切回终端：终端留在树上（隐藏），不得重建。
    // 终端是开关：再点一次回到对话。
    fireEvent.click(screen.getByRole("button", { name: "终端" }));
    fireEvent.click(screen.getByRole("button", { name: "终端" }));
    expect(terminalMounts.count).toBe(1);
  });

  /// Alt+↑/↓ 按侧栏眼前的顺序切上一条/下一条（对照 Kimi「上一条/下一条」）；到头不循环。
  it("Alt+↑/↓ 按侧栏顺序切换会话", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    invoke.mockImplementation((command: string, args: { sessionId?: number }) => {
      if (command === "get_chat_history") {
        return Promise.resolve({
          sessionId: args?.sessionId ?? 7, title: `会话 ${args?.sessionId}`, status: "running",
          provider: "claude", cwd: null, supported: true, offset: 0, reset: false,
          pendingReview: null, items: [],
        });
      }
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "get_live_sessions_page") {
        return Promise.resolve([
          { session: { id: 7, cc_session_id: "a", status: "running" }, project_name: "p", task_title: "第一条", connected: true, pending_review: null, provider: "claude", cwd: "C:/a" },
          { session: { id: 42, cc_session_id: "b", status: "running" }, project_name: "p", task_title: "第二条", connected: true, pending_review: null, provider: "claude", cwd: "C:/b" },
        ]);
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findByRole("button", { name: /第二条/ });
    const current = () => invoke.mock.calls.filter(([command]) => command === "get_chat_history").at(-1)?.[1]?.sessionId;
    fireEvent.keyDown(window, { key: "ArrowUp", altKey: true });
    expect(current()).toBe(7);
    fireEvent.keyDown(window, { key: "ArrowDown", altKey: true });
    await waitFor(() => expect(current()).toBe(42));
  });

  it("keeps terminal view when switching sessions from the sidebar", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    invoke.mockImplementation((command: string, args: { sessionId?: number }) => {
      if (command === "get_chat_history") {
        return Promise.resolve({
          sessionId: args?.sessionId ?? 7, title: `会话 ${args?.sessionId}`, status: "running",
          provider: "claude", cwd: null, supported: true, offset: 0, reset: false,
          pendingReview: null, items: [],
        });
      }
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 7, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      if (command === "get_live_sessions_page") {
        return Promise.resolve([
          { session: { id: 7, cc_session_id: "a", status: "running" }, project_name: "p", task_title: "会话 7", connected: true, pending_review: null, provider: "claude", cwd: "C:/a" },
          { session: { id: 42, cc_session_id: "b", status: "running" }, project_name: "p", task_title: "另一个会话", connected: true, pending_review: null, provider: "claude", cwd: "C:/b" },
        ]);
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    // 切到终端视图。
    fireEvent.click(await screen.findByRole("button", { name: "终端" }));
    expect(screen.getByText("PTY 7")).toBeTruthy();
    // 从侧栏切到另一个会话——视图必须仍是终端，而不是弹回对话。
    fireEvent.click(await screen.findByRole("button", { name: /另一个会话/ }));
    expect(await screen.findByText("PTY 42")).toBeTruthy();
    expect(screen.queryByRole("combobox", { name: "发送消息给 Agent" })).toBeNull();
  });

  /// 归档此前只有看板的卡片菜单能做：读完一个会话想收起它，得先切回看板、在一堆卡片里
  /// 找回同一条。入口补进标题栏后，按钮本身也是「这条已归档」的唯一提示，故点完必须当场
  /// 翻面（乐观），失败再翻回来并报错——静默失败等于骗用户「已归档」。
  it("标题栏归档当前会话：按钮当场翻面，失败回滚并报错", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    const base = {
      sessionId: 7, title: "要收起的会话", status: "ended", provider: "claude", cwd: null,
      supported: true, offset: 0, reset: false, pendingReview: null, items: [], archived: false,
    };
    let archiveFails = false;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(base);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "set_archived") return archiveFails ? Promise.reject(new Error("db busy")) : Promise.resolve();
      // 归档后要切走：这里只有它自己一条，没有可切的下一条，窗口留在原地。
      if (command === "get_live_sessions_page") return Promise.resolve([]);
      return Promise.resolve();
    });
    render(<ChatWindow />);
    // 归档入口收进标题菜单（Kimi 式）：点标题开菜单再点「归档」。
    fireEvent.click(await screen.findByRole("button", { name: "会话操作" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "标记完成" }));
    expect(invoke).toHaveBeenCalledWith("set_archived", { sessionId: 7, archived: true });
    // 翻面后菜单项变成「取消归档」——它是这条会话已归档的唯一提示（菜单点完即收，重开验证）。
    fireEvent.click(screen.getByRole("button", { name: "会话操作" }));
    expect(await screen.findByRole("menuitem", { name: "恢复进行中" })).toBeTruthy();

    archiveFails = true;
    fireEvent.click(screen.getByRole("menuitem", { name: "恢复进行中" }));
    // 失败回滚：错误可见，重开菜单仍是归档态（取消归档）。
    expect(await screen.findByText(/db busy/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "会话操作" }));
    expect(await screen.findByRole("menuitem", { name: "恢复进行中" })).toBeTruthy();
  });

  /// 「标记完成」直达钮（对照 Kimi 标题栏）：一击完成，不必进标题菜单；完成后钮随即消失
  /// （恢复进行中仍走标题菜单）。
  it("顶栏「标记完成」一击完成当前会话，完成后钮消失", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 7, title: "做完的会话", status: "ended", provider: "claude", cwd: null,
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], archived: false,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "get_live_sessions_page") return Promise.resolve([]);
      return Promise.resolve();
    });
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "标记完成" }));
    expect(invoke).toHaveBeenCalledWith("set_archived", { sessionId: 7, archived: true });
    await waitFor(() => expect(screen.queryByRole("button", { name: "标记完成" })).toBeNull());
  });

  /// 标记完成会顺带结束会话：Agent 正在干活时先确认（取消则什么都不发），停着时直接完成。
  it.each([false, true])("运行中标记完成先确认，确认=%s", async (answer) => {
    window.history.replaceState({}, "", "/?sessionId=7");
    const confirms: unknown[] = [];
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 7, title: "跑着的会话", status: "running", provider: "claude", cwd: null,
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], archived: false,
        connected: true, ptyManaged: true, screenState: "working",
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "confirm_dialog") { confirms.push(args); return Promise.resolve(answer); }
      if (command === "get_live_sessions_page") return Promise.resolve([]);
      return Promise.resolve();
    });
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "标记完成" }));
    await waitFor(() => expect(JSON.stringify(confirms)).toContain(zh.chat.markDoneRunningConfirm));
    if (answer) {
      await waitFor(() => expect(invoke).toHaveBeenCalledWith("set_archived", { sessionId: 7, archived: true }));
    } else {
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(invoke).not.toHaveBeenCalledWith("set_archived", expect.anything());
    }
  });

  /// 归档 = 收纳：侧栏里当场消失，右边却还停在它的对话上等于「收起来了还摊在桌上」。
  /// 归档后自动切到列表里的下一条会话。
  it("归档当前会话后切到下一条", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    const histories: Record<number, unknown> = {
      7: { sessionId: 7, title: "要收起的", status: "ended", provider: "claude", cwd: null, supported: true, offset: 0, reset: false, pendingReview: null, items: [], archived: false },
      9: { sessionId: 9, title: "下一条", status: "ended", provider: "claude", cwd: null, supported: true, offset: 0, reset: false, pendingReview: null, items: [], archived: false },
    };
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve(histories[args?.sessionId as number]);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "get_live_sessions_page") {
        return Promise.resolve([{ session: { id: 9, cc_session_id: "cc-9", status: "ended" }, task_title: "下一条", connected: false, provider: "claude", cwd: null }]);
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "会话操作" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "标记完成" }));
    // 看标题栏（侧栏条目也叫「下一条」，且标题现在首读前就先摆出来）。
    await waitFor(() => expect(document.querySelector(".chat-bar .chat-title")?.textContent).toBe("下一条"));
  });

  /// Ctrl/Cmd+K 命令面板：会话上百条时侧栏翻页太慢，这是键盘用户的主通道；
  /// U1-14 后同层还能搜并执行窗口命令。
  it("Ctrl+K 打开命令面板，↑↓ 选择、Enter 切到目标会话", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    const histories: Record<number, unknown> = {
      7: { sessionId: 7, title: "当前会话", status: "ended", provider: "claude", cwd: null, supported: true, offset: 0, reset: false, pendingReview: null, items: [], archived: false },
      9: { sessionId: 9, title: "目标会话", status: "ended", provider: "claude", cwd: null, supported: true, offset: 0, reset: false, pendingReview: null, items: [], archived: false },
    };
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve(histories[args?.sessionId as number]);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "get_live_sessions_page") {
        return Promise.resolve([
          { session: { id: 7, cc_session_id: "cc-7", status: "ended" }, task_title: "当前会话", project_name: "repo", connected: false, provider: "claude", cwd: null },
          { session: { id: 9, cc_session_id: "cc-9", status: "ended" }, task_title: "目标会话", project_name: "other", connected: false, provider: "claude", cwd: null },
        ]);
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    // 标题与侧栏条目都叫「当前会话」：等标题栏那颗菜单钮（chat-title-menu 内）出现即可。
    await waitFor(() => expect(screen.getAllByRole("button", { name: /当前会话/ }).length).toBeGreaterThan(0));
    fireEvent.keyDown(window, { key: "k", code: "KeyK", ctrlKey: true });
    const input = await screen.findByPlaceholderText(zh.chat.switcherPlaceholder);
    // 首屏（空查询）即列出最近会话 + 命令组全列（2 会话 + 9 命令）
    const options = await screen.findAllByRole("option");
    expect(options).toHaveLength(11);
    expect(screen.getByText(zh.chat.switcherGroupCommands)).toBeTruthy();
    // 二期 #4：空查询默认选中第一个非当前会话（目标会话），直接 Enter 切换
    await waitFor(() => expect(screen.getByRole("option", { name: /目标会话/ }).getAttribute("aria-selected")).toBe("true"));
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_chat_history", { sessionId: 9, offset: 0 }));
    // 切换器已关闭
    expect(screen.queryByPlaceholderText(zh.chat.switcherPlaceholder)).toBeNull();
  });

  /// 命令面板的另一半（U1-14）：命令条目执行窗口已有动作——这里点「收起 / 展开侧栏」，
  /// 验证 ChatWindow 的派发接线（弹层关闭 + 侧栏真收起）。
  it("命令面板执行「收起侧栏」命令：关弹层并收起侧栏", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") {
        return Promise.resolve({ sessionId: 7, title: "当前会话", status: "ended", provider: "claude", cwd: null, supported: true, offset: 0, reset: false, pendingReview: null, items: [], archived: false });
      }
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "get_live_sessions_page") return Promise.resolve([]);
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findByRole("button", { name: "收起会话列表" });
    fireEvent.keyDown(window, { key: "k", code: "KeyK", ctrlKey: true });
    const input = await screen.findByPlaceholderText(zh.chat.switcherPlaceholder);
    // 空查询时无会话：active=0 即第一条命令（新建会话）；End 跳到最后一项再往回到侧栏命令，
    // 直接点击更直给——点击路径与 Enter 共用同一条 run()。
    fireEvent.click(screen.getByRole("option", { name: new RegExp(zh.chat.shortcutSidebar) }));
    expect(screen.queryByPlaceholderText(zh.chat.switcherPlaceholder)).toBeNull();
    await waitFor(() => expect(screen.queryByRole("button", { name: "收起会话列表" })).toBeNull());
    expect(localStorage.getItem("meowo-chat-sidebar-collapsed")).toBe("1");
    expect(input).toBeTruthy();
  });

  it("collapses the sidebar into a title-bar toggle and restores it", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    respondWithHistory({
      sessionId: 7, title: "折叠", status: "ended", provider: "claude", cwd: null,
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    });
    render(<ChatWindow />);
    // 展开态：收起按钮在侧栏里，标题栏没有展开按钮。
    const collapse = await screen.findByRole("button", { name: "收起会话列表" });
    expect(screen.queryByRole("button", { name: "展开会话列表" })).toBeNull();
    fireEvent.click(collapse);
    // 收起态：侧栏整个消失，展开入口出现在标题栏，偏好落盘。
    expect(screen.queryByRole("button", { name: "收起会话列表" })).toBeNull();
    expect(localStorage.getItem("meowo-chat-sidebar-collapsed")).toBe("1");
    fireEvent.click(screen.getByRole("button", { name: "展开会话列表" }));
    expect(await screen.findByRole("button", { name: "收起会话列表" })).toBeTruthy();
    expect(localStorage.getItem("meowo-chat-sidebar-collapsed")).toBe("0");
  });

  /** 侧栏分栏柄（用户要求：侧栏可拖）：separator 落在停靠侧栏与主列之间，键盘
   *  ←/→ 步进调宽并落盘；jsdom 里 .chat-body 实测宽为 0，上限钳到下限 200，
   *  正好一并验证钳制。抽屉形态（窄窗）与收起态不渲染柄。 */
  it("侧栏分栏柄:键盘调宽并持久化,收起后消失", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    localStorage.removeItem("meowo-chat-sidebar-width");
    respondWithHistory({
      sessionId: 7, title: "拖宽", status: "ended", provider: "claude", cwd: null,
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    });
    render(<ChatWindow />);
    await screen.findByRole("button", { name: "收起会话列表" });
    const resizer = screen.getByRole("separator", { name: "拖拽调整宽度" });
    expect(resizer.getAttribute("aria-valuenow")).toBe("240");
    // aside 吃到默认宽。
    expect(document.querySelector(".chat-sidebar")?.getAttribute("style")).toContain("width: 240px");
    // → 调宽：jsdom 无实测布局，上限 = max(200, 0-360) = 200，钳到下限并落盘。
    fireEvent.keyDown(resizer, { key: "ArrowRight" });
    expect(localStorage.getItem("meowo-chat-sidebar-width")).toBe("200");
    expect(document.querySelector(".chat-sidebar")?.getAttribute("style")).toContain("width: 200px");
    // 收起侧栏：柄随布局一起消失。
    fireEvent.click(screen.getByRole("button", { name: "收起会话列表" }));
    expect(screen.queryByRole("separator", { name: "拖拽调整宽度" })).toBeNull();
  });

  it("外部占用时就地给出接管入口，确认后重放刚才那次发送", async () => {
    window.history.replaceState({}, "", "/?sessionId=15");
    const history = {
      sessionId: 15, title: "外部运行中", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    };
    let takenOver = false;
    let wrote = false;
    // 确认走应用内原生小窗(invoke confirm_dialog):按队列依次给答案,不再 mock 系统 confirm。
    const confirmAnswers: boolean[] = [false, true];
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "confirm_dialog") return Promise.resolve(confirmAnswers.shift() ?? false);
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      // 接管后的重发走写后回显验证(submitToTerminal 的 verify),write 之后补回显。
      if (command === "write_managed_terminal") {
        if (String((args as { data?: string } | undefined)?.data ?? "").includes("别起第二个")) wrote = true;
        return Promise.resolve();
      }
      if (command === "managed_terminal_snapshot") {
        // 接管前没有托管 PTY；接管后有，且已画出可见内容。
        return Promise.resolve(takenOver
          ? { sessionId: 15, active: true, managed: true, data: b64utf8(wrote ? "ready ❯ 别起第二个" : "ready"), startOffset: 0, endOffset: wrote ? 30 : 5, exited: false, exitCode: null }
          : { sessionId: 15, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      // 进程是否真活着由后端按 pid 判定——前端不再靠 status 猜，而是让这次 start 被拒。
      if (command === "start_managed_terminal") return Promise.reject("会话仍在外部终端运行，不能重复接管");
      if (command === "takeover_managed_terminal") { takenOver = true; return Promise.resolve(); }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "别起第二个" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    expect(await screen.findByText(/在外部终端运行/)).toBeTruthy();
    // 输入不能丢：失败的原文退回输入框，接管后要原样重发这条。
    await waitFor(() => expect(input.value).toBe("别起第二个"));
    expect(invoke).not.toHaveBeenCalledWith("write_managed_terminal", expect.anything());

    // 接管是破坏性的（杀掉外部进程），必须显式确认；取消（队列首个 false）则什么都不做。
    fireEvent.click(screen.getByRole("button", { name: "结束并接管" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("confirm_dialog", expect.anything()));
    expect(invoke).not.toHaveBeenCalledWith("takeover_managed_terminal", expect.anything());

    // 再点一次（队列次个 true）→ 确认接管。
    fireEvent.click(screen.getByRole("button", { name: "结束并接管" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("takeover_managed_terminal", { sessionId: 15, cols: 100, rows: 30 }));
    // 接管成功后自动重放刚才那次发送，用户不必重打一遍。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 15, data: "别起第二个" }), { timeout: 3_000 });
  });

  /** C-8：接管条的显隐挂 needsTakeover 而不是 sendError。打字会清掉 sendError（输入框
   *  onChange 的 setSendError("")），但外部进程还活着，接管入口不能跟着消失——此前
   *  一敲键盘「结束并接管」按钮就没了，needsTakeover 还悬着。 */
  it("打字清掉发送错误后,接管入口仍在(C-8)", async () => {
    window.history.replaceState({}, "", "/?sessionId=21");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 21, title: "外部运行中", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 21, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null,
      });
      // 后端按 pid 判定进程仍活着 → 拒绝起托管终端,前端给出就地接管入口。
      if (command === "start_managed_terminal") return Promise.reject("会话仍在外部终端运行，不能重复接管");
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "在吗" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    // 发送被拒 → 接管入口出现。
    expect(await screen.findByRole("button", { name: "结束并接管" })).toBeTruthy();
    // 继续打字：错误文本被清掉,但接管入口必须留在原地。
    fireEvent.change(input, { target: { value: "在吗?" } });
    expect(screen.getByRole("button", { name: "结束并接管" })).toBeTruthy();
  });

  /** 会话被外部终端占用（后端明确 ptyManaged=false）时门卡横幅给出接管动线，composer
   *  禁用而非卸载——输入框与草稿留在原地（C-15）；接管成功（ptyManaged 翻真）后解禁。
   *  （上一条用例的 mock 不带 ptyManaged 字段，覆盖的是信号缺失时的兜底旧路径。） */
  it("外部占用的会话直接显示接管门卡,接管成功后 composer 回归", async () => {
    window.history.replaceState({}, "", "/?sessionId=19");
    let takenOver = false;
    invoke.mockImplementation((command: string) => {
      if (command === "confirm_dialog") return Promise.resolve(true);
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 19, title: "外部占用", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [],
        ptyManaged: takenOver,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve(takenOver
          ? { sessionId: 19, active: true, managed: true, data: btoa("ready"), startOffset: 0, endOffset: 5, exited: false, exitCode: null }
          : { sessionId: 19, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      if (command === "takeover_managed_terminal") { takenOver = true; return Promise.resolve(); }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    expect(await screen.findByText(/在外部终端运行/)).toBeTruthy();
    // 门卡态：输入框在但禁用——草稿不藏，只是发不出去。
    const gatedBox = screen.getByRole("combobox", { name: "发送消息给 Agent" });
    expect((gatedBox as HTMLTextAreaElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "结束并接管" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("takeover_managed_terminal", { sessionId: 19, cols: 100, rows: 30 }));
    // ptyManaged 随下一轮轮询翻真 → 门卡撤下，composer 解禁。
    const box = await screen.findByRole("combobox", { name: "发送消息给 Agent" }, { timeout: 3_000 });
    await waitFor(() => expect((box as HTMLTextAreaElement).disabled).toBe(false));
  });

  /** 已结束的会话同理：composer 禁用而非卸载（草稿不藏），门卡横幅的「恢复会话」是
   *  唯一动线——只拉起托管终端，不发送任何内容，恢复完成后 composer 解禁。 */
  it("已结束的会话显示恢复门卡,点恢复只拉起终端不发内容", async () => {
    window.history.replaceState({}, "", "/?sessionId=20");
    let resumed = false;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 20, title: "已结束", status: "ended", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: false,
        ptyManaged: resumed,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve(resumed
          ? { sessionId: 20, active: true, managed: true, data: btoa("ready"), startOffset: 0, endOffset: 5, exited: false, exitCode: null }
          : { sessionId: 20, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      if (command === "start_managed_terminal") { resumed = true; return Promise.resolve(); }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    expect(await screen.findByText(zh.chat.composerGateEnded)).toBeTruthy();
    // 门卡态：输入框在但禁用——草稿不藏，只是发不出去。
    const endedBox = screen.getByRole("combobox", { name: "发送消息给 Agent" });
    expect((endedBox as HTMLTextAreaElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "恢复会话" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("start_managed_terminal", expect.objectContaining({ sessionId: 20 })));
    const resumedBox = await screen.findByRole("combobox", { name: "发送消息给 Agent" }, { timeout: 3_000 });
    await waitFor(() => expect((resumedBox as HTMLTextAreaElement).disabled).toBe(false));
    // 恢复只是拉起终端，不发送任何内容。只查本会话：前序用例的异步尾巴可能还在
    // 共享 mock 上落别的会话的写入，那不是本用例要防的。
    expect(invoke.mock.calls.some(
      (call) => call[0] === "write_managed_terminal" && (call[1] as { sessionId?: number } | undefined)?.sessionId === 20,
    )).toBe(false);
  });

  /// Agent 自己派出的后台会话（Claude Code 的 FleetView）：它不消费 stdin，往它的 PTY 写
  /// 按键石沉大海。送话要经 Agent 守护进程的控制通道——发送必须走那条路，且**不能**碰
  /// 托管终端的任何命令（起终端、接管、写终端）。
  it("后台会话发消息走守护进程通道，不碰托管终端", async () => {
    window.history.replaceState({}, "", "/?sessionId=17");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 17, title: "后台跑着", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, background: true,
        // 有对话内容 → 停在对话页（transcript 空的后台会话会被自动切到终端页，另有用例覆盖）。
        items: [{ type: "user_text", id: "u0", timestamp: null, text: "之前的" }],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "继续干活" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("send_background_prompt", { sessionId: 17, text: "继续干活" }));
    // 守护进程的 ok 只代表收下了投递，手动模式的 worker 根本不消费——**绝不能清空输入框**。
    // 实测踩过：清掉之后用户刚打的字再也找不回来，而消息其实没被处理。
    expect(await screen.findByText(zh.chat.sendBackgroundQueued)).toBeTruthy();
    expect(input.value).toBe("继续干活");
    // 托管终端那套对它无效，一条都不该发出去。
    expect(invoke).not.toHaveBeenCalledWith("start_managed_terminal", expect.anything());
    expect(invoke).not.toHaveBeenCalledWith("write_managed_terminal", expect.anything());
    expect(invoke).not.toHaveBeenCalledWith("takeover_managed_terminal", expect.anything());
  });

  /// claude 的 fork/resume 后台 worker 有不落盘的老毛病：transcript 里只剩两行元数据，
  /// 正文只活在进程内存。对话页给不出任何东西，而终端旁路拿得到完整画面——这类会话
  /// 打开就该落在终端页，而不是让用户对着「还没有对话记录」发呆。
  it("后台会话的 transcript 为空时直接落在终端页", async () => {
    window.history.replaceState({}, "", "/?sessionId=18");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 18, title: "没落盘", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], background: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 18, active: true, managed: true, data: btoa("screen"), startOffset: 0, endOffset: 6,
        exited: false, exitCode: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    // 终端页是「按下」态,且画面已接上。
    await waitFor(() => expect(screen.getByRole("button", { name: "终端" }).getAttribute("aria-pressed")).toBe("true"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("attach_background_session", { sessionId: 18 }));
    // 注：「在终端页打字会被带回对话页」那条走 xterm 的 onData，而 xterm 在 jsdom 里不渲染，
    // 这里模拟不了按键。该行为只在真机验证过，没有单测护着。
  });

  /// items 这一帧还没读到、但 hook 记着最近往来 → 内容是有的，不能甩去终端页。
  it("后台会话 items 为空但 hook 有往来时仍停在对话页", async () => {
    window.history.replaceState({}, "", "/?sessionId=20");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 20, title: "有往来", status: "ended", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], background: true,
        lastUserText: "hi", lastAiText: "你好",
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findByText("你好");
    expect(screen.getByRole("button", { name: "终端" }).getAttribute("aria-pressed")).toBe("false");
  });

  /// 有对话内容的后台会话照旧停在对话页——自动切终端只是「没东西可显示」时的兜底。
  it("后台会话有对话内容时仍停在对话页", async () => {
    window.history.replaceState({}, "", "/?sessionId=19");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 19, title: "有内容", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, background: true,
        items: [{ type: "user_text", id: "u1", timestamp: null, text: "在吗" }],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findByText("在吗");
    expect(screen.getByRole("button", { name: "终端" }).getAttribute("aria-pressed")).toBe("false");
  });

  it("外部会话其实已经死了（status 陈旧）时，直接起托管终端而不是一律拒绝", async () => {
    window.history.replaceState({}, "", "/?sessionId=16");
    // status 仍是 running/stale，但进程早没了——后端 pid 判定会放行这次 start。
    const history = {
      sessionId: 16, title: "陈旧状态", status: "stale", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    };
    let started = false;
    let wrote = false;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "start_managed_terminal") { started = true; return Promise.resolve(); }
      // 刚拉起的发送走写后回显验证(submitToTerminal 的 verify),write 之后补回显。
      if (command === "write_managed_terminal") {
        if (String((args as { data?: string } | undefined)?.data ?? "").includes("继续")) wrote = true;
        return Promise.resolve();
      }
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve(started
          ? { sessionId: 16, active: true, managed: true, data: b64utf8(wrote ? "ready ❯ 继续" : "ready"), startOffset: 0, endOffset: wrote ? 20 : 5, exited: false, exitCode: null }
          : { sessionId: 16, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "继续" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("start_managed_terminal", { sessionId: 16, cols: 100, rows: 30 }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 16, data: "继续" }), { timeout: 3_000 });
    // 等回车也落地再结束本用例:正文与回车之间隔着 SUBMIT_GAP_MS,提前收工的话这次写入会在
    // 后面某个用例执行到一半时才落进共享的 invoke.mock.calls,把那边的「零副作用」断言打翻
    // (真实现场:软拦用例偶发失败在这条 sessionId 16 的 "\r" 上)。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 16, data: "\r" }), { timeout: 3_000 });
  });

  it("does not show the terminal-wait banner during a normal startup wait", async () => {
    // T-8 的等待条(已等秒数 + 「去终端页」)只在等得够久(TERMINAL_WAIT_NOTICE_S=20s,
    // 多半卡住)时出现:正常启动 5–15 秒,期间发送钮转圈就够,逢恢复必出横幅是噪声
    // (用户实拍「能不能不要显示」)。
    window.history.replaceState({}, "", "/?sessionId=17");
    const history = {
      sessionId: 17, title: "慢启动", status: "ended", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    };
    let started = false;
    let ready = false;
    let wrote = false;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "start_managed_terminal") { started = true; return Promise.resolve(); }
      if (command === "write_managed_terminal") {
        if (String((args as { data?: string } | undefined)?.data ?? "").includes("继续")) wrote = true;
        return Promise.resolve();
      }
      if (command === "managed_terminal_snapshot") {
        if (!started) return Promise.resolve({ sessionId: 17, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
        // ready 前:活着但一直画不出可见内容,waitForTerminalReady 持续等待;
        // ready 后:可见画面 + 输出静止 → 判就绪(回显验证需要的「继续」随 wrote 补上)。
        return Promise.resolve(ready
          ? { sessionId: 17, active: true, managed: true, data: b64utf8(wrote ? "ready ❯ 继续" : "ready"), startOffset: 0, endOffset: wrote ? 20 : 5, exited: false, exitCode: null }
          : { sessionId: 17, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "继续" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(started).toBe(true));
    // 过了旧阈值(2s)仍在等:等待条不出。
    await new Promise((resolve) => setTimeout(resolve, 3_000));
    expect(screen.queryByText(/等待 Agent 终端就绪/)).toBeNull();
    // 放行就绪:等待条收起、消息送达。把在途链路走完再收工,别留轮询污染后面的用例
    // (与上一个用例的「等回车落地」同一纪律)。
    ready = true;
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 17, data: "继续" }), { timeout: 3_000 });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 17, data: "\r" }), { timeout: 3_000 });
    await waitFor(() => expect(screen.queryByText(/等待 Agent 终端就绪/)).toBeNull());
  });

  it("keeps the prompt and reports a managed terminal that exits during startup", async () => {
    window.history.replaceState({}, "", "/?sessionId=14");
    const history = {
      sessionId: 14, title: "恢复失败", status: "ended", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    };
    let snapshotCalls = 0;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_snapshot") {
        snapshotCalls += 1;
        return Promise.resolve(snapshotCalls === 1
          ? { sessionId: 14, active: false, managed: false, data: "", exited: false, exitCode: null }
          // 失败带 tail(T-8):CLI 拒绝启动的原因随快照的 exitTail 送达,不只一句退出码。
          : { sessionId: 14, active: false, managed: false, data: "launch error", exited: true, exitCode: 1, exitTail: "launch error" });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "不要丢失" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(await screen.findByText(/Agent 启动后退出（退出码 1）：launch error/)).toBeTruthy();
    expect(input.value).toBe("不要丢失");
    expect(invoke).not.toHaveBeenCalledWith("write_managed_terminal", expect.objectContaining({ sessionId: 14 }));
  });

  it("shows Claude's native command approval for an already-managed PTY", async () => {
    window.history.replaceState({}, "", "/?sessionId=45");
    const prompt = [
      "\x1b[2JBash command",
      "cargo build -p meowo-agent -p meowo-store 2>&1 | tail -20",
      "Build rust crates",
      "This command requires approval",
      "Do you want to proceed?",
      "> 1. Yes",
      "  2. Yes, and don't ask again for: cargo build *",
      "  3. No",
    ].join("\r\n");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 45, title: "托管命令审批", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: "approval", items: [],
      });
      if (command === "pending_interaction") return Promise.resolve({
        approval: null,
        question: null,
        runtimeRequests: [{
          sessionId: 45, requestId: "hook-45", provider: "claude", kind: "approval", status: "pending",
          toolName: "Bash", description: "hook 同步审批", input: "{}", permissionSuggestions: [],
          responseCapability: { type: "broker_live" },
        }],
      });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 45, active: true, managed: true, data: btoa(prompt), startOffset: 0, endOffset: prompt.length,
        exited: false, exitCode: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);

    expect(await screen.findByRole("button", { name: "允许一次" })).toBeTruthy();
    // Claude 的 hook 与 TUI 会竞速出现；RuntimeRequest 统一后仍由屏幕卡接管，不能双卡。
    expect(screen.getAllByRole("button", { name: "允许一次" })).toHaveLength(1);
    expect(screen.queryByText("hook 同步审批")).toBeNull();
    expect(screen.getByRole("button", { name: "允许并记住 · cargo build *" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "拒绝" })).toBeTruthy();
    expect(screen.getByText(/cargo build -p meowo-agent/)).toBeTruthy();
    expect(screen.getByText("Do you want to proceed?")).toBeTruthy();
    expect(screen.queryByText("该请求来自非托管会话，请在原终端中处理")).toBeNull();
  });

  /**
   * claude 的拒绝项存在长文案形态(「No, and tell Claude what to do differently (esc)」)。
   * 回归:全等匹配 /^(?:no|reject)$/ 认不出它——拒绝按钮和 Esc 快捷键无声消失,
   * 且该项被「既非拒绝也非允许的第一项」规则吸收成「允许并记住」,点持久放行实际发拒绝。
   */
  it("claude 长文案拒绝项仍归类为拒绝,不冒充「允许并记住」", async () => {
    window.history.replaceState({}, "", "/?sessionId=49");
    const prompt = [
      "\x1b[2JBash command",
      "rm -rf build",
      "Clean build output",
      "This command requires approval",
      "Do you want to proceed?",
      "> 1. Yes",
      "  2. No, and tell Claude what to do differently (esc)",
    ].join("\r\n");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 49, title: "长文案拒绝", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: "approval", items: [],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 49, active: true, managed: true, data: btoa(prompt), startOffset: 0, endOffset: prompt.length,
        exited: false, exitCode: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);

    expect(await screen.findByRole("button", { name: "允许一次" })).toBeTruthy();
    // 拒绝按钮在,且没有任何选项被误认成「允许并记住」。
    expect(screen.getByRole("button", { name: "拒绝" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /允许并记住/ })).toBeNull();
  });

  /**
   * TUI 重绘残留:审批框弹出前 claude 已经刷过一屏工具输出,上一次的审批框也还留在
   * 缓冲里。此前识别窗口从**第一处**签名起截,于是「Do you want to proceed?」在卡上
   * 显示两遍(description 一遍、question 一遍),命令区里还混着上一轮的搜索结果——
   * 真机截图为证。窗口改取最后一处签名,解析再把重复问句和铺行的框线横杠清掉。
   */
  it("重绘残留:审批问句只显示一遍,命令区不夹带上一屏输出", async () => {
    window.history.replaceState({}, "", "/?sessionId=46");
    const prompt = [
      "\x1b[2JSearched for 3 patterns, ran 5 shell commands",
      "● Web Search(\"codex tui approval prompt not displayed\")",
      "  └ Did 1 search in 8s",
      "Do you want to proceed? ──────────────────────────────",
      "> 1. Yes",
      "  2. No",
      "Tool use",
      "Bash command",
      "lark-cli attendance query --employee-type employee_id",
      "查询打卡记录",
      "This command requires approval",
      "Do you want to proceed? ──────────────────────────────",
      "> 1. Yes",
      "  2. Yes, and don't ask again for: lark-cli *",
      "  3. No",
    ].join("\r\n");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 46, title: "重绘残留", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: "approval", items: [],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 46, active: true, managed: true, data: btoa(unescape(encodeURIComponent(prompt))), startOffset: 0, endOffset: prompt.length,
        exited: false, exitCode: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);

    expect(await screen.findByRole("button", { name: "允许一次" })).toBeTruthy();
    // 问句只出现一次,且尾部铺行的框线横杠已剥掉。
    expect(screen.getAllByText("Do you want to proceed?")).toHaveLength(1);
    expect(screen.queryByText(/Do you want to proceed\? ─/)).toBeNull();
    // 命令区认得出这次的命令,不夹带上一屏的搜索输出。
    expect(screen.getByText(/lark-cli attendance query/)).toBeTruthy();
    expect(screen.queryByText(/Web Search/)).toBeNull();
    expect(screen.queryByText(/Searched for 3 patterns/)).toBeNull();
  });

  /// 2.1.304 源码里审批框标题有变体:Windows 的「PowerShell command」与沙箱外的
  /// 「… (unsandboxed)」。此前只认裸「Bash command」,框边界认不出,上一屏输出整段并进命令区。
  it("PowerShell / unsandboxed 标题也认作框边界,工具名如实显示", async () => {
    window.history.replaceState({}, "", "/?sessionId=48");
    const prompt = [
      "\x1b[2J● 上一屏的残留输出",
      "PowerShell command (unsandboxed)",
      "Get-Process -Id 62844",
      "查进程",
      "Do you want to proceed?",
      "> 1. Yes",
      "  2. No",
    ].join("\r\n");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 48, title: "PowerShell 审批", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: "approval", items: [],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 48, active: true, managed: true, data: btoa(unescape(encodeURIComponent(prompt))), startOffset: 0, endOffset: prompt.length,
        exited: false, exitCode: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);

    expect(await screen.findByRole("button", { name: "允许一次" })).toBeTruthy();
    expect(screen.getByText(/Get-Process -Id 62844/)).toBeTruthy();
    expect(screen.queryByText(/上一屏的残留输出/)).toBeNull();
    expect(document.querySelector(".chat-approval-tool code")?.textContent).toBe("PowerShell");
  });

  it("shows Kimi's approval panel as actionable GUI choices (digit keys)", async () => {
    window.history.replaceState({}, "", "/?sessionId=47");
    const prompt = [
      "\x1b[2J  ▶ Run this command?",
      "  $ echo meowo-approval-probe-47",
      "  Run the probe command",
      "  ▶ 1. Approve once",
      "    2. Approve for this session",
      "    3. Reject",
      "    4. Reject with feedback",
      "  ↑/↓ select · 1/2/3/4 choose · ↵ confirm",
    ].join("\r\n");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 47, title: "kimi 审批", status: "waiting", provider: "kimi", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: "approval", items: [],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("kimi"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 47, active: true, managed: true,
        // 面板含 ▶/↑/↓ 等非 Latin1 字符,btoa 不能直接吃 Unicode 字符串,先走 UTF-8 字节。
        data: btoa(String.fromCharCode(...new TextEncoder().encode(prompt))),
        startOffset: 0, endOffset: prompt.length, exited: false, exitCode: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);

    // 识别成可操作审批卡：三个能直接完成的选项（feedback 项不收），命令原文在详情里。
    expect(await screen.findByRole("button", { name: "允许一次" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "本会话内允许" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "拒绝" })).toBeTruthy();
    expect(screen.getByText("Run this command?")).toBeTruthy();
    expect(screen.getByText(/echo meowo-approval-probe-47/)).toBeTruthy();
    // 不再是「只能去终端处理」的降级卡。
    expect(screen.queryByText("Meowo 正在从托管终端读取 Agent 的选项…")).toBeNull();
    // 按钮 = 直接向 PTY 打数字键（kimi 面板数字直选，官方源码 selectAndSubmit）。
    fireEvent.click(screen.getByRole("button", { name: "本会话内允许" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 47, data: "2" }));
    // terminal_live 只写原生 TUI；与 broker 共用 RuntimeRequest 后也不能误入统一 broker 回答命令。
    expect(invoke).not.toHaveBeenCalledWith("respond_runtime_request", expect.anything());
  });

  /// gemini 0.65 的 ask_user 提问表单（npm bundle 源码拼出的画面，2026-11-12，未实拍）：
  /// 认成可作答的提问卡，点选项直接从 ● 焦点做相对移动 + 回车。修复前对话页不出卡，
  /// 用户只能切到终端回答。兄弟场景：手机远程页渲染的是同一个 ChatWindow 卡片。
  it("gemini 0.65 ask_user 表单在对话页出提问卡，点选项即作答", async () => {
    window.history.replaceState({}, "", "/?sessionId=48");
    const prompt = [
      "\x1b[2J\u280f Waiting for user confirmation...",
      "\u256d\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u256e",
      "\u2502 Answer Questions                         \u2502",
      "\u2502\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2502",
      "\u2502 Which database should we use?            \u2502",
      "\u2502                                          \u2502",
      "\u2502   1.  PostgreSQL                         \u2502",
      "\u2502         Relational, strong consistency   \u2502",
      "\u2502 \u25cf 2.  SQLite                             \u2502",
      "\u2502   3.  Enter a custom value               \u2502",
      "\u2502                                          \u2502",
      "\u2502 Enter to select \u00b7 \u2191/\u2193 to navigate \u00b7 Esc to cancel \u2502",
      "\u2570\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u256f",
    ].join("\r\n");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 48, title: "gemini 提问", status: "running", provider: "gemini", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("gemini"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 48, active: true, managed: true,
        data: btoa(String.fromCharCode(...new TextEncoder().encode(prompt))),
        startOffset: 0, endOffset: prompt.length, exited: false, exitCode: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);

    expect(await screen.findByText("Agent 正在等待你的回答")).toBeTruthy();
    expect(screen.getByText("Which database should we use?")).toBeTruthy();
    expect(screen.getByText("Relational, strong consistency")).toBeTruthy();
    // 自定义答案项要现场打字，不出按钮；不是命令审批卡。
    expect(screen.queryByRole("button", { name: /Enter a custom value/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "允许一次" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /PostgreSQL/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 48, data: "\x1b[A\r" }));
  });

  it("shows a managed multi-select question without requiring a Terminal tab visit", async () => {
    window.history.replaceState({}, "", "/?sessionId=46");
    const prompt = [
      "\x1b[2JWhich items should I continue with?",
      "> 1. [ ] First-screen tail reading",
      "  2. [ ] Connection pooling",
      "  3. [ ] Keep the current state",
      "  4. [ ] Type something",
      "Submit",
      "Enter to select · up/down to navigate · Esc to cancel",
    ].join("\r\n");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 46, title: "托管问答", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: "question", items: [],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 46, active: true, managed: true, data: btoa(prompt), startOffset: 0, endOffset: prompt.length,
        exited: false, exitCode: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);

    expect(await screen.findByText("Agent 正在等待你的回答")).toBeTruthy();
    expect(screen.getByText(/Which items should I continue with/)).toBeTruthy();
    const firstChoice = screen.getByRole("button", { name: "First-screen tail reading" });
    expect(firstChoice).toBeTruthy();
    expect(screen.getByPlaceholderText("输入其他回答")).toBeTruthy();
    expect(screen.getByRole("button", { name: "提交选择" })).toBeTruthy();
    fireEvent.click(firstChoice);
    expect(firstChoice.className).toContain("is-selected");
    expect(invoke).toHaveBeenCalledWith("write_managed_terminal", {
      sessionId: 46, data: "\r",
    });
    expect(screen.queryByRole("button", { name: "上一项 ↑" })).toBeNull();
    expect(screen.queryByText("Meowo 正在从托管终端读取 Agent 的选项…")).toBeNull();
  });

  /**
   * 「仅收起」:识别是启发式的,误报/过期的卡必须有不写 PTY 的零副作用出口——
   * 此前唯一的关闭方式都要发按键(\r/Esc),Esc 还会打断正在跑的回合。
   */
  it("交互卡可仅收起:不向终端写任何字节,输入框恢复", async () => {
    window.history.replaceState({}, "", "/?sessionId=47");
    const prompt = [
      "\x1b[2JWhich items should I continue with?",
      "> 1. [ ] First-screen tail reading",
      "  2. [ ] Connection pooling",
      "Submit",
      "Enter to select · up/down to navigate · Esc to cancel",
    ].join("\r\n");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 47, title: "误报收起", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: "question", items: [],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 47, active: true, managed: true, data: btoa(prompt), startOffset: 0, endOffset: prompt.length,
        exited: false, exitCode: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    expect(await screen.findByText("Agent 正在等待你的回答")).toBeTruthy();
    // 只数**本会话**的写入:invoke 的调用记录是全测试文件共享的,别的用例卸载后仍在飞的
    // 异步写入会迟到落进来(见 sessionId 16 那个用例的收尾注释)。
    const writes = () => invoke.mock.calls.filter(
      ([command, args]) => command === "write_managed_terminal" && (args as { sessionId: number }).sessionId === 47,
    ).length;
    const writesBefore = writes();
    fireEvent.click(screen.getByRole("button", { name: "仅收起" }));
    // 零副作用断言必须**同步**做(点击处理器只 setTerminalAttention(null),纯同步):
    // 放到下面的 await/waitFor 之后,慢机(macOS CI)上后台 timer 会在那段窗口里插入一次
    // 无关写入,把「零副作用」误判成失败。点击后当场查,timer 还没机会触发。
    expect(writes()).toBe(writesBefore);
    await waitFor(() => expect(screen.queryByText(/Which items should I continue with/)).toBeNull());
  });

  /**
   * hook已报告交互时，不能把识别不到题面当成可发送；正文+回车会误答原生选择器。
   */
  it("pendingReview 未识别成卡片时保留原文，不发送到未知选择器", async () => {
    window.history.replaceState({}, "", "/?sessionId=48");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 48, title: "未识别提示", status: "waiting", provider: "codex", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: "question", items: [],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("codex"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        // 屏幕上是识别不出的提示形态(比如 codex 自家的选择器),没有任何卡片。
        sessionId: 48, active: true, managed: true, data: btoa("\x1b[2Jsome unrecognized picker"), startOffset: 0, endOffset: 28,
        exited: false, exitCode: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "继续" } });

    fireEvent.keyDown(input, { key: "Enter" });
    expect(await screen.findByText(zh.chat.cardLockQuestion)).toBeTruthy();
    expect(invoke.mock.calls.some(([command, args]) => command === "write_managed_terminal" && args?.sessionId === 48)).toBe(false);
    expect((input as HTMLTextAreaElement).value).toBe("继续");
    expect(invoke.mock.calls.some(([command]) => command === "confirm_dialog")).toBe(false);
  });

  /**
   * 外部终端跑的会话:提问/审批一律留在终端处理(后端 broker 同口径整段短路)。
   * pendingReview 的降级卡对它不再渲染——弹一张只能「打开终端」的卡只是打扰,
   * 用户就坐在那个终端前(明确反馈)。
   */
  it("外部会话的 pendingReview 不弹「去终端处理」降级卡", async () => {
    window.history.replaceState({}, "", "/?sessionId=51");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 51, title: "外部提问", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: "question", items: [], ptyManaged: false,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      return Promise.resolve();
    });
    render(<ChatWindow />);
    // 正例锚:外部会话 composer 让位给接管门卡——门卡在场说明首轮渲染已完成。
    expect(await screen.findByText(/会话在外部终端运行/)).toBeTruthy();
    expect(screen.queryByText("Agent 正在等待你的回答")).toBeNull();
    expect(screen.queryByRole("button", { name: "去终端页" })).toBeNull();
  });

  /** 对照:托管会话在屏幕未识别出卡片时,降级卡仍是去终端页处理的入口,不得连带消失。 */
  it("托管会话的 pendingReview 降级卡保留", async () => {
    window.history.replaceState({}, "", "/?sessionId=52");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 52, title: "托管降级", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: "question", items: [], ptyManaged: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        // 屏幕上是识别不出的形态:没有屏幕卡,降级卡是唯一入口。
        sessionId: 52, active: true, managed: true, data: btoa("\x1b[2Junrecognized"), startOffset: 0, endOffset: 14,
        exited: false, exitCode: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    expect(await screen.findByText("Agent 正在等待你的回答")).toBeTruthy();
    expect(screen.getByRole("button", { name: "去终端页" })).toBeTruthy();
  });

  it("renders and resolves a managed permission request", async () => {
    window.history.replaceState({}, "", "/?sessionId=12");
    const history = {
      sessionId: 12, title: "审批", status: "running", provider: "codex", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: "approval", items: [],
    };
    let pending: unknown = {
      sessionId: 12, requestId: "request-1", provider: "codex", toolName: "Bash",
      description: "运行测试", input: "{\"command\":\"cargo test\"}",
      permissionSuggestions: [{
        type: "addRules", behavior: "allow", destination: "localSettings",
        rules: [{ toolName: "Bash", ruleContent: "cargo test" }],
      }],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "pending_interaction") return Promise.resolve({ approval: pending, question: null });
      if (command === "respond_runtime_request") { pending = null; return Promise.resolve(); }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    expect(await screen.findByText("运行测试")).toBeTruthy();
    // Bash 参数只摆命令本身（commandOfInput），不摆 JSON 外壳。
    expect(screen.getByText("cargo test")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /^允许并记住（此项目、本机）/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("respond_runtime_request", {
      sessionId: 12, requestId: "request-1", choice: "suggestion:0",
    }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "允许一次" })).toBeNull());
    expect(screen.queryByText("该请求来自非托管会话，请在原终端中处理")).toBeNull();
  });

  /**
   * 审批卡上的 `Esc` 徽章必须说话算话——键真绑上,落点是「拒绝」。
   *
   * 只绑这一个方向:Enter→允许**故意没绑**。输入框外随手一个回车就把执行权交出去,
   * 换不来那点快捷。焦点在输入框里时整条快捷键让开,那里的 Esc 另有主人(收补全菜单)。
   */
  it("审批卡:Esc 落到拒绝,且不抢输入框里的 Esc", async () => {
    window.history.replaceState({}, "", "/?sessionId=13");
    let pending: unknown = {
      sessionId: 13, requestId: "request-esc", provider: "claude", toolName: "Bash",
      description: "查打卡记录", input: "lark-cli attendance query", permissionSuggestions: [],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 13, title: "Esc 拒绝", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: "approval", items: [], connected: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: pending, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "respond_runtime_request") { pending = null; return Promise.resolve(); }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const deny = await screen.findByRole("button", { name: "拒绝" });
    // kbd 徽章是给眼睛的,不进可访问名——上面按 "拒绝" 精确找得到就是证据。
    expect(deny.textContent).toContain("Esc");

    // 焦点在输入框里:这一下归补全菜单/输入框,审批卡不许截走。
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    input.focus();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(invoke.mock.calls.some(([command]) => command === "respond_runtime_request")).toBe(false);

    input.blur();
    // 两段式确认:第一下只把拒绝按钮点亮为「再按 Esc 确认拒绝」,不发拒绝——
    // 空手一个 Esc 就替 agent 递出不可撤销的正式拒绝,误触面太大。
    fireEvent.keyDown(window, { key: "Escape" });
    expect(invoke.mock.calls.some(([command]) => command === "respond_runtime_request")).toBe(false);
    expect(await screen.findByRole("button", { name: "再按 Esc 确认拒绝" })).toBeTruthy();
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("respond_runtime_request", {
      sessionId: 13, requestId: "request-esc", choice: "deny",
    }));
  });

  /** 对照 Kimi 审批卡：数字键 1 = 允许一次（焦点在输入框时让开），「说明并拒绝」把理由带给 Agent。 */
  it("审批卡数字键 1 允许一次，附理由拒绝带上理由", async () => {
    window.history.replaceState({}, "", "/?sessionId=23");
    let pending: unknown = {
      sessionId: 23, requestId: "request-keys", provider: "claude", toolName: "Bash",
      description: "跑测试", input: "cargo test", permissionSuggestions: [],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 23, title: "审批快捷键", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: "approval", items: [], connected: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: pending, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findByRole("button", { name: "允许一次" });
    // 焦点在输入框：数字键属于输入，不能替用户放行。
    const input = screen.getByRole("combobox", { name: "发送消息给 Agent" });
    input.focus();
    fireEvent.keyDown(input, { key: "1" });
    expect(invoke.mock.calls.some(([command]) => command === "respond_runtime_request")).toBe(false);
    input.blur();
    fireEvent.keyDown(window, { key: "1" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("respond_runtime_request", {
      sessionId: 23, requestId: "request-keys", choice: "allow_once",
    }));
    // 换一条新请求：附理由拒绝。
    pending = { ...(pending as object), requestId: "request-reason" };
    fireEvent.click(await screen.findByRole("button", { name: "说明并拒绝" }, { timeout: 3000 }));
    fireEvent.change(screen.getByRole("textbox", { name: "说明拒绝原因…" }), { target: { value: "先别跑全量" } });
    fireEvent.click(screen.getByRole("button", { name: "提交并拒绝" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("respond_runtime_request", {
      sessionId: 23, requestId: "request-reason", choice: "deny:先别跑全量",
    }));
  });

  /** C-10：审批提交失败不再是空 catch 完全静默——原因写进卡内错误行,卡片留在原地
   *  (请求未结算,下一次轮询仍可能恢复它)。顺带钉住 C-9:危险命令(rm -rf)的
   *  「允许一次」染红(is-danger),与安全命令的深色主按钮拉开视觉权重。 */
  it("审批提交失败写卡内错误行,危险命令主按钮染红(C-9/C-10)", async () => {
    window.history.replaceState({}, "", "/?sessionId=22");
    const pending = {
      sessionId: 22, requestId: "request-fail", provider: "claude", toolName: "Bash",
      description: "清理构建产物", input: "rm -rf build", permissionSuggestions: [],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 22, title: "审批失败", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: "approval", items: [], connected: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: pending, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "respond_runtime_request") return Promise.reject("broker 已结算");
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const allow = await screen.findByRole("button", { name: "允许一次" });
    // rm -rf 与 ls 不同权重:危险命令的允许钮是红色警示主按钮。
    expect(allow.className).toContain("is-danger");
    fireEvent.click(allow);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("respond_runtime_request", {
      sessionId: 22, requestId: "request-fail", choice: "allow_once",
    }));
    // 失败原因就地可见,卡片与按钮不消失(此前点了像没点一样)。
    expect(await screen.findByText("broker 已结算")).toBeTruthy();
    expect(screen.getByRole("button", { name: "允许一次" })).toBeTruthy();
  });

  /**
   * 回归：负载缺 `permissionSuggestions` 时审批条照常渲染，不许崩整窗。
   *
   * 类型上该字段恒在（DTO 保证），但真实世界里出现过缺席：后端曾直接 emit 原始
   * `ApprovalRequest`（空列表被 `skip_serializing_if` 略去），codex 的审批一弹，
   * ChatWindow 就死在 `.map` 上（TypeError: Cannot read properties of undefined）。
   * 后端已改走 DTO；这里钉住前端的 `?? []` 防御，堵旧后端/新前端错配的同一条死路。
   */
  it("survives an approval payload that lacks permissionSuggestions", async () => {
    window.history.replaceState({}, "", "/?sessionId=12");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 12, title: "审批", status: "running", provider: "codex", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: "approval", items: [],
      });
      if (command === "pending_interaction") return Promise.resolve({
        question: null,
        approval: {
          sessionId: 12, requestId: "request-lean", provider: "codex", toolName: "Bash",
          description: "运行测试", input: "{\"command\":\"cargo test\"}",
          // 刻意没有 permissionSuggestions —— 模拟被 skip 掉字段的瘦负载。
        },
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    // 审批条正常出现：允许/拒绝都在，只是没有「记住」类按钮。
    expect(await screen.findByRole("button", { name: "允许一次" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "拒绝" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /允许并记住/ })).toBeNull();
  });

  /**
   * 回归：用户在会话 A 输入时，会话 B 的授权请求**不许**把窗口切到 B（旧行为：后端
   * emit chat-session-changed + set_focus，草稿被收走、焦点被抢）。现在后端只闪任务栏，
   * 前端的职责是：A 的画面纹丝不动，B 在侧栏亮琥珀徽标，cleared 后徽标回落状态点。
   */
  it("别的会话请求授权:不切会话不弹卡,只在侧栏亮徽标,清除后回落", async () => {
    window.history.replaceState({}, "", "/?sessionId=12");
    const liveSession = (id: number, title: string) => ({
      session: { id, cc_session_id: `cc-${id}`, status: "running" },
      project_name: "meowo", task_title: title, connected: true,
      pending_review: null, cwd: "C:/repo", provider: "claude",
    });
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 12, title: "当前会话", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 12, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      if (command === "get_live_sessions_page") return Promise.resolve({
        items: [liveSession(12, "当前会话"), liveSession(13, "另一条会话")],
        next_cursor: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const otherDot = () => screen.getByRole("button", { name: /另一条会话/ }).querySelector(".chat-sidebar-dot");
    await waitFor(() => expect(otherDot()?.className).toContain("is-running"));

    const payload = {
      sessionId: 13, requestId: "request-bg", provider: "claude", toolName: "Bash",
      description: "跑构建", input: "{}", permissionSuggestions: [],
    };
    act(() => { eventListeners.get("pending-approval")?.({ payload }); });
    // 徽标亮起、压过 running 点；当前会话的画面不受任何打扰。
    await waitFor(() => expect(otherDot()?.className).toContain("is-approval"));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText("跑构建")).toBeNull();

    act(() => { eventListeners.get("pending-approval-cleared")?.({ payload }); });
    await waitFor(() => expect(otherDot()?.className).toContain("is-running"));
  });

  /**
   * 提问与审批同策略：会话 B 的 interactive-question 不切窗不弹卡，只亮侧栏徽标。
   * 题面没有 cleared 事件，徽标在用户切到 B 时退场（题面靠轮询取回）。
   */
  it("别的会话提问:不切会话不弹卡,侧栏亮徽标", async () => {
    window.history.replaceState({}, "", "/?sessionId=12");
    const liveSession = (id: number, title: string) => ({
      session: { id, cc_session_id: `cc-${id}`, status: "running" },
      project_name: "meowo", task_title: title, connected: true,
      pending_review: null, cwd: "C:/repo", provider: "claude",
    });
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 12, title: "当前会话", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 12, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      if (command === "get_live_sessions_page") return Promise.resolve({
        items: [liveSession(12, "当前会话"), liveSession(13, "另一条会话")],
        next_cursor: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const otherDot = () => screen.getByRole("button", { name: /另一条会话/ }).querySelector(".chat-sidebar-dot");
    await waitFor(() => expect(otherDot()?.className).toContain("is-running"));

    const payload = {
      sessionId: 13, requestId: "request-q", provider: "claude", toolName: "AskUserQuestion",
      description: null, input: '{"questions":[{"question":"选哪个?","options":[{"label":"甲"}]}]}',
      permissionSuggestions: [],
    };
    act(() => { eventListeners.get("interactive-question")?.({ payload }); });
    await waitFor(() => expect(otherDot()?.className).toContain("is-approval"));
    // 当前会话不弹题面卡。
    expect(screen.queryByText("选哪个?")).toBeNull();
  });
});
