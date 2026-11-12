import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

// 复刻 ChatWindow.test.tsx 的夹具(mock 按文件隔离,不能跨文件共享)。覆盖远程能力门控
// 与窄屏「列表 ↔ 对话」导航；mock 只提供这些流程需要的命令。
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({
  invoke,
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ close: vi.fn(() => Promise.resolve()), setTitle: vi.fn(() => Promise.resolve()), onDragDropEvent: () => Promise.resolve(() => {}) }),
}));
vi.mock("./ManagedTerminal", () => ({
  ManagedTerminal: (props: { sessionId: number }) => <div>PTY {props.sessionId}</div>,
}));
// 这里只验顶栏入口到受控页签的接线；文件树/diff 内容由 GitDiffView 的组件测试覆盖，
// 不把懒加载 chunk 的时序变成全量测试的不稳定因素。
vi.mock("./chat/GitDiffView", () => ({
  GitDiffView: ({ activeTab }: { activeTab: "changes" | "files" }) => activeTab === "files"
    ? <input placeholder="搜索文件名或内容…" />
    : <div>工作区没有改动</div>,
}));

vi.mock("./chat/SidePanel", () => ({
  SidePanel: ({ state }: { state: { tabs: { key: string; type: string }[]; active: string | null } }) =>
    state.tabs.find((tab) => tab.key === state.active)?.type === "diff" ? <div>工作区没有改动</div> : <div>面板启动列表</div>,
}));

import { ChatWindow } from "./ChatWindow";
import { markRemoteUi, SELECT_SESSION_EVENT } from "../remoteMode";

function setNarrow(matches: boolean) {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: (query: string) => ({
      matches: query === "(max-width: 880px)" ? matches : false,
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }),
  });
}

// git 仓库 + 运行中会话:diff 入口(需 cwd)与「终端」页签都应在桌面态出现。
function runningSessionImpl(command: string, args?: Record<string, unknown>): Promise<unknown> {
  if (command === "get_live_sessions_page") {
    return Promise.resolve([{
      session: { id: 7, cc_session_id: "cc-7", status: "running", last_event_at: Date.now() },
      task_title: "门控用例", project_name: "repo", cwd: "C:/repo", provider: "claude",
      connected: true, pending_review: null, busy_subagents: 0,
    }]);
  }
  if (command === "get_chat_history") {
    const h = {
      connected: true, sessionId: 7, title: "门控用例", status: "running",
      provider: "claude", cwd: "C:/repo", supported: true, offset: 10, reset: false,
      pendingReview: null, ptyManaged: true,
      items: [{ type: "assistant_text", id: "a1", timestamp: null, text: "在跑" }],
    };
    const cursor = (args?.offset as number) ?? 0;
    if (cursor > 0 && cursor >= h.offset) return Promise.resolve({ ...h, items: [], hasMore: false });
    return Promise.resolve(h);
  }
  if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
  if (command === "git_diff_summary") return Promise.resolve({ gitAvailable: true, isRepo: true, branch: "main", ahead: null, behind: null, files: [] });
  if (command === "list_dir_entries") return Promise.resolve([]);
  if (command === "managed_terminal_binding") return Promise.resolve(null);
  if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 7, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
  return Promise.resolve();
}
function runningSessionWithRepo() {
  invoke.mockImplementation(runningSessionImpl);
}

afterEach(() => {
  cleanup();
  invoke.mockReset();
  // 门控标志挂在 globalThis,不复位会把后面的桌面用例污染成远程态(参照 remoteMode.test.ts)。
  (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = undefined;
  document.body.classList.remove("remote-ui");
  window.history.replaceState({}, "", "/");
  Reflect.deleteProperty(window, "matchMedia");
  localStorage.clear();
});

describe("ChatWindow 远程门控", () => {
  // 有待回答的提问、却既没有 broker 题面也没认出终端表单：手机上没有终端页可去，
  // 卡里直接给终端画面和按键，不再写「请在桌面端作答」（用户实拍反馈）。
  it("手机远程:认不出的提问给终端画面与按键,按键写进终端", async () => {
    markRemoteUi();
    setNarrow(false);
    window.history.replaceState({}, "", "/?sessionId=7");
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") {
        return (runningSessionImpl(command, args) as Promise<Record<string, unknown>>)
          .then((h) => ({ ...h, status: "waiting", pendingReview: "question" }));
      }
      if (command === "managed_terminal_screen") return Promise.resolve(["", " > 1. 深色界面", "   2. 浅色界面", ""]);
      return runningSessionImpl(command, args);
    });
    render(<ChatWindow />);
    expect(await screen.findByText("没认出选项，可直接操作终端")).toBeTruthy();
    expect(screen.queryByText("请在桌面端作答，完成后本卡自动关闭")).toBeNull();
    expect(await screen.findByText(/1\. 深色界面/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "↓" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 7, data: "\x1b[B" }));
  });

  it("桌面态:侧边面板开关与分支改动胶囊共用同一停靠面板", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    runningSessionWithRepo();
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getAllByText("门控用例").length).toBeGreaterThan(0));

    expect(screen.getByRole("button", { name: "终端" })).toBeTruthy();
    // 标题栏右侧照 Kimi：侧边面板开关打开面板；还没开过页签时面板里是启动列表
    // （页签本身由 SidePanel / panelTabs 的测试覆盖，这里它是替身）。
    fireEvent.click(await screen.findByRole("button", { name: "侧边面板" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "侧边面板" }).getAttribute("aria-pressed")).toBe("true"));
    expect(await screen.findByText("面板启动列表")).toBeTruthy();
    // 分支改动胶囊打开改动页签。
    fireEvent.click(screen.getByRole("button", { name: "查看改动" }));
    expect(await screen.findByText("工作区没有改动")).toBeTruthy();
    expect(screen.getByRole("button", { name: "查看改动" }).getAttribute("aria-pressed")).toBe("true");
    // 胶囊再点一次收起面板。
    fireEvent.click(screen.getByRole("button", { name: "查看改动" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "侧边面板" }).getAttribute("aria-pressed")).toBe("false"));
  });

  it("宽屏远程:保留停靠侧栏,隐藏终端页签与文件入口,改动入口放行", async () => {
    markRemoteUi();
    setNarrow(false);
    window.history.replaceState({}, "", "/?sessionId=7");
    runningSessionWithRepo();
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getAllByText("门控用例").length).toBeGreaterThan(0));

    expect(screen.getByRole("button", { name: "收起会话列表" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "终端" })).toBeNull();
    expect(screen.queryByRole("button", { name: "侧边面板" })).toBeNull();
    // 改动列表按用户决定放行（remote.rs 模块头「例外」，后端收口到会话目录与改动清单）。
    expect(await screen.findByRole("button", { name: "查看改动" })).toBeTruthy();
    expect(screen.getByRole("combobox", { name: "发送消息给 Agent" })).toBeTruthy();
  });

  it("手机远程:首开进会话列表,可选会话并返回,返回后不再恢复上次会话", async () => {
    markRemoteUi();
    setNarrow(true);
    runningSessionWithRepo();
    render(<ChatWindow />);

    const item = await screen.findByRole("button", { name: /门控用例/ });
    expect(screen.getByText("继续桌面上的工作")).toBeTruthy();
    expect(document.querySelector(".chat-window.is-remote-list")).not.toBeNull();
    fireEvent.click(item);
    await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    await waitFor(() => expect(localStorage.getItem("meowo-remote-last-session")).toBe("7"));

    fireEvent.click(screen.getByRole("button", { name: "返回会话列表" }));
    await waitFor(() => expect(document.querySelector(".chat-window.is-remote-list")).not.toBeNull());
    expect(screen.queryByRole("combobox", { name: "发送消息给 Agent" })).toBeNull();
    expect(localStorage.getItem("meowo-remote-last-session")).toBeNull();
  });

  // 进会话压一条历史记录：Safari 左缘右滑 / 安卓返回键（= 浏览器后退）回到会话列表，
  // 而不是退出整个页面（用户问「会话页向右滑能不能返回」）。
  it("手机远程:浏览器后退(右滑返回)从会话回到列表", async () => {
    markRemoteUi();
    setNarrow(true);
    runningSessionWithRepo();
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: /门控用例/ }));
    await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    await waitFor(() => expect((window.history.state as { meowoChat?: boolean } | null)?.meowoChat).toBe(true));
    window.history.back();
    await waitFor(() => expect(document.querySelector(".chat-window.is-remote-list")).not.toBeNull());
    expect(screen.queryByRole("combobox", { name: "发送消息给 Agent" })).toBeNull();
  });

  // M-1（走查 2026-11-12）：从对话返回列表，搜索词与页签原先全部重置（列表进对话即卸载）。
  // 兄弟场景：切到「设置」屏再回来（RemoteSessionList 本就只藏不卸）在 RemoteSessionList.test.tsx。
  it("手机远程:从对话返回列表后,搜索词与列表实例都还在", async () => {
    markRemoteUi();
    setNarrow(true);
    runningSessionWithRepo();
    render(<ChatWindow />);
    const box = await screen.findByRole("textbox", { name: "搜索" });
    fireEvent.change(box, { target: { value: "门控" } });
    fireEvent.click(await screen.findByRole("button", { name: /门控用例/ }));
    await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.click(screen.getByRole("button", { name: "返回会话列表" }));
    await waitFor(() => expect(document.querySelector(".chat-window.is-remote-list")).not.toBeNull());
    expect((screen.getByRole("textbox", { name: "搜索" }) as HTMLInputElement).value).toBe("门控");
    // 同一个列表实例（没卸载重建），滚动位置随 DOM 一起留着。
    expect(screen.getByRole("textbox", { name: "搜索" })).toBe(box);
  });

  // M-2（走查 2026-11-12）：「更改」页打开时右滑（浏览器后退）原先连对话一起退回列表。
  it("手机远程:「更改」页打开时后退只关「更改」页,留在对话", async () => {
    markRemoteUi();
    setNarrow(true);
    runningSessionWithRepo();
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: /门控用例/ }));
    await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    await waitFor(() => expect((window.history.state as { meowoChat?: boolean } | null)?.meowoChat).toBe(true));
    fireEvent.click(await screen.findByRole("button", { name: "查看改动" }));
    expect(await screen.findByText("工作区没有改动")).toBeTruthy();
    await waitFor(() => expect((window.history.state as { meowoLayer?: string } | null)?.meowoLayer).toBeTruthy());
    window.history.back();
    await waitFor(() => expect(screen.getByRole("button", { name: "查看改动" }).getAttribute("aria-pressed")).toBe("false"));
    expect(document.querySelector(".chat-window.is-remote-list")).toBeNull();
    expect(screen.getByRole("combobox", { name: "发送消息给 Agent" })).toBeTruthy();
    // 再后退才回到列表。
    window.history.back();
    await waitFor(() => expect(document.querySelector(".chat-window.is-remote-list")).not.toBeNull());
  });

  // 看过的会话再进：直接用快照出画面（对照 Kimi 的会话池），只从记住的 offset 续读增量，
  // 不再整读（offset=0）——手机上「返回列表再点回来」此前次次骨架屏重读整段。
  it("手机远程:返回列表再进同一会话,直接出内容且只读增量", async () => {
    markRemoteUi();
    setNarrow(true);
    runningSessionWithRepo();
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: /门控用例/ }));
    await screen.findByText("在跑");
    fireEvent.click(screen.getByRole("button", { name: "返回会话列表" }));
    await waitFor(() => expect(document.querySelector(".chat-window.is-remote-list")).not.toBeNull());
    const fullReads = () => invoke.mock.calls.filter(([c, a]) => c === "get_chat_history" && (a as { offset: number }).offset === 0).length;
    const before = fullReads();
    fireEvent.click(await screen.findByRole("button", { name: /门控用例/ }));
    // 同步可见：不经骨架屏、不等任何请求。
    expect(screen.getByText("在跑")).toBeTruthy();
    expect(document.querySelector(".chat-loading")).toBeNull();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_chat_history", expect.objectContaining({ sessionId: 7, offset: 10 })));
    expect(fullReads()).toBe(before);
  });

  // 列表上按下即预取首屏（对照 Kimi 的 prefetchMainTranscript）：点进去时接过同一发请求，
  // 不重复整读；读完再点则直接用快照出画面。
  it("手机远程:按下列表项即预取,点进去不重复整读", async () => {
    markRemoteUi();
    setNarrow(true);
    runningSessionWithRepo();
    render(<ChatWindow />);
    const item = await screen.findByRole("button", { name: /门控用例/ });
    const fullReads = () => invoke.mock.calls.filter(([c, a]) => c === "get_chat_history" && (a as { sessionId: number; offset: number }).sessionId === 7 && (a as { offset: number }).offset === 0).length;
    expect(fullReads()).toBe(0);
    fireEvent.pointerDown(item);
    expect(fullReads()).toBe(1);
    fireEvent.click(item);
    await screen.findByText("在跑");
    expect(fullReads()).toBe(1);
  });

  // 首读未回时顶栏先摆列表里的真实标题（对照 Kimi），不是通用的「会话对话」占位。
  it("手机远程:点进会话首读未回时,顶栏已是该会话标题", async () => {
    markRemoteUi();
    setNarrow(true);
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return new Promise(() => {});
      return runningSessionImpl(command, args);
    });
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: /门控用例/ }));
    await waitFor(() => expect(document.querySelector(".chat-bar .chat-title")?.textContent).toBe("门控用例"));
    expect(document.querySelector(".chat-bar .chat-title-menu")?.getAttribute("aria-busy")).toBe("true");
  });

  // 回合在跑时手机占位也要说清发出去会怎样（此前一律「发消息给 Agent」，发完才知道排没排队）。
  it("手机远程:回合运行中,输入框占位说明发送后的去向", async () => {
    markRemoteUi();
    setNarrow(true);
    runningSessionWithRepo();
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: /门控用例/ }));
    await screen.findByText("在跑");
    const box = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    await waitFor(() => expect(box.getAttribute("placeholder")).not.toBe("发消息给 Agent"));
    expect(["发送会插入当前回合，不打断它", "发送后排队，当前任务结束再发", "当前任务结束后才能发送"]).toContain(box.getAttribute("placeholder"));
  });

  // 手机没有 Ctrl+Shift+F 也没有命令面板：标题菜单是对话内搜索唯一的入口（Kimi 手机上可搜）。
  it("手机远程:标题菜单可打开对话内搜索", async () => {
    markRemoteUi();
    setNarrow(true);
    runningSessionWithRepo();
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: /门控用例/ }));
    await screen.findByText("在跑");
    fireEvent.click(document.querySelector(".chat-bar .chat-title-btn") as HTMLElement);
    fireEvent.click(await screen.findByRole("menuitem", { name: "在对话中搜索" }));
    expect(await screen.findByLabelText("在对话中搜索")).toBeTruthy();
  });

  it("远程态:新建会话事件选中临时 id(启动中占位),binding 认领后落到真会话", async () => {
    markRemoteUi();
    setNarrow(true);
    // 认领结果挂进我们手里的 deferred:负 id 生效的那一拍 resolve() 就发出(挂载即
    // void resolve(),不等 250ms 节拍),mock 若立即兑现,占位「会话正在启动…」只在
    // 相邻两次 commit 之间存在,waitFor 首拍经常抓不到——全量跑高负载下 flake 过。
    // 挂起认领把占位钉成稳态,断言后再放行,占位/认领两段时序各自确定。
    let releaseClaim: ((id: number) => void) | undefined;
    const claim = new Promise<number>((resolve) => { releaseClaim = resolve; });
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      // 临时 id -3 认领成真 id 7(T-13 binding 轮询通道,managed_terminal_binding 在远程白名单)。
      if (command === "managed_terminal_binding") {
        return (args?.sessionId as number) === -3 ? claim : Promise.resolve(null);
      }
      return runningSessionImpl(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("继续桌面上的工作");

    // RemoteApp 在 NewSessionPanel 启动成功后派发:临时负 id 即导航句柄。
    window.dispatchEvent(new CustomEvent(SELECT_SESSION_EVENT, { detail: -3 }));
    await waitFor(() => expect(screen.getByText("会话正在启动…")).toBeTruthy());
    expect(document.querySelector(".chat-window.is-remote-list")).toBeNull();

    // 放行认领:原地重绑成真会话,空态/占位都退场。
    releaseClaim!(7);
    await waitFor(() => expect(screen.getAllByText("门控用例").length).toBeGreaterThan(0), { timeout: 3000 });
    expect(screen.queryByText("会话正在启动…")).toBeNull();
  });

  it("远程窄屏:独立顶栏并进标题行,侧栏开关落在标题行", async () => {
    // jsdom 无 matchMedia,ChatWindow 恒按宽窗走;这里装一个恒命中的替身模拟手机竖屏。
    const original = window.matchMedia;
    window.matchMedia = ((query: string) => ({
      matches: true, media: query, onchange: null,
      addEventListener: () => {}, removeEventListener: () => {},
      addListener: () => {}, removeListener: () => {}, dispatchEvent: () => false,
    })) as typeof window.matchMedia;
    try {
      markRemoteUi();
      window.history.replaceState({}, "", "/?sessionId=7");
      runningSessionWithRepo();
      const { container } = render(<ChatWindow />);
      await waitFor(() => expect(document.querySelector(".chat-title")?.textContent).toBe("门控用例"));

      expect(container.querySelector(".chat-topbar")).toBeNull();
      expect(container.querySelector(".chat-window.is-remote-compact")).toBeTruthy();
      expect(container.querySelector(".chat-bar .chat-sidebar-open")).toBeTruthy();
    } finally {
      window.matchMedia = original;
    }
  });
});

describe("ChatWindow 远程底部面板", () => {
  /// 对照 Kimi 手机端：键盘弹起后输入框上方放不下菜单，斜杠命令改走底部面板。面板自带搜索框，
  /// 与输入框共用同一份文本；没有命中也不收（边打边消失的搜索框没法用）；Esc 只收面板、不丢字。
  it("手机上打 / 弹出命令面板，搜索框与输入框同一份文本", async () => {
    const original = window.matchMedia;
    window.matchMedia = ((query: string) => ({
      matches: true, media: query, onchange: null,
      addEventListener: () => {}, removeEventListener: () => {},
      addListener: () => {}, removeListener: () => {}, dispatchEvent: () => false,
    })) as typeof window.matchMedia;
    try {
      markRemoteUi();
      window.history.replaceState({}, "", "/?sessionId=7");
      runningSessionWithRepo();
      render(<ChatWindow />);
      await waitFor(() => expect(document.querySelector(".chat-title")?.textContent).toBe("门控用例"));
      const composer = screen.getByRole("combobox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement;
      fireEvent.change(composer, { target: { value: "/" } });
      const search = await screen.findByRole("textbox", { name: "搜索命令" }) as HTMLInputElement;
      expect(screen.getByRole("dialog", { name: "命令" })).toBeTruthy();
      expect(search.value).toBe("/");

      fireEvent.change(search, { target: { value: "/xyz" } });
      expect(screen.getByText("没有匹配的命令")).toBeTruthy();
      expect(composer.value).toBe("/xyz");

      fireEvent.keyDown(window, { key: "Escape" });
      await waitFor(() => expect(screen.queryByRole("dialog", { name: "命令" })).toBeNull());
      expect(composer.value).toBe("/xyz");
    } finally {
      window.matchMedia = original;
    }
  });
});

describe("ChatWindow 远程答题", () => {
  // 远程无 push 事件(listen 是空订阅),题面只经 pending_interaction 轮询到达。
  function questionSessionImpl(question: Record<string, unknown>) {
    return (command: string, args?: Record<string, unknown>): Promise<unknown> => {
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question });
      return runningSessionImpl(command, args);
    };
  }

  it("可代答题面:手机上直接作答提交,不再指路桌面端", async () => {
    markRemoteUi();
    window.history.replaceState({}, "", "/?sessionId=7");
    invoke.mockImplementation(questionSessionImpl({
      sessionId: 7, requestId: "request-remote-answer", provider: "claude", toolName: "AskUserQuestion",
      description: null, answerable: true, permissionSuggestions: [],
      input: JSON.stringify({ questions: [{ question: "晚饭吃什么？", multiSelect: false, options: [{ label: "火锅" }, { label: "烧烤" }] }] }),
    }));
    render(<ChatWindow />);
    expect(await screen.findByText(/晚饭吃什么/)).toBeTruthy();
    expect(screen.queryByText(/请在桌面端作答/)).toBeNull();
    expect(screen.queryByRole("button", { name: "转为逐题作答" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /火锅/ }));
    fireEvent.click(screen.getByRole("button", { name: "提交选择" }));
    await waitFor(() => {
      const resolved = invoke.mock.calls.find((call) => call[0] === "resolve_pending_approval");
      expect(resolved).toBeTruthy();
      const choice = (resolved![1] as { choice: string }).choice;
      expect(JSON.parse(choice.slice("answers:".length))).toEqual({ "晚饭吃什么？": "火锅" });
    });
  });

  it("卡内答不了的题面:转为逐题作答只放行挂起,不切终端页", async () => {
    markRemoteUi();
    window.history.replaceState({}, "", "/?sessionId=7");
    // 两题同文:答案按题面原文键控,卡内无法表达(answersRepresentable=false)。
    invoke.mockImplementation(questionSessionImpl({
      sessionId: 7, requestId: "request-remote-stepwise", provider: "claude", toolName: "AskUserQuestion",
      description: null, answerable: true, permissionSuggestions: [],
      input: JSON.stringify({ questions: [
        { header: "甲", question: "选哪个？", multiSelect: false, options: [{ label: "A" }] },
        { header: "乙", question: "选哪个？", multiSelect: false, options: [{ label: "B" }] },
      ] }),
    }));
    render(<ChatWindow />);
    expect(await screen.findByText("这组问题无法在卡片内作答，请转为逐题作答")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "转为逐题作答" }));
    await waitFor(() => {
      const resolved = invoke.mock.calls.find((call) => call[0] === "resolve_pending_approval");
      expect((resolved?.[1] as { choice?: string } | undefined)?.choice).toBe("pass");
    });
    // 远程没有终端视图:仍停在对话页(输入框在场)。
    expect(screen.getByRole("combobox", { name: "发送消息给 Agent" })).toBeTruthy();
  });
});
