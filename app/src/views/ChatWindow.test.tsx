import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

const invoke = vi.hoisted(() => vi.fn());
const openDialog = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const confirmDialog = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openDialog, confirm: confirmDialog }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ close: vi.fn(() => Promise.resolve()) }),
}));
// 记录挂载次数：切 tab 不应重建终端（重建=dispose+new Terminal+全量 backlog 重放）。
const terminalMounts = vi.hoisted(() => ({ count: 0 }));
// 真实组件的屏幕识别在这里跑不动（没有 xterm）。把它的回调与入参暴露出来，测试就能
// 用**真实解析器**产出的 attention 驱动 ChatWindow，验证渲染与按键下发这一段。
const terminalProps = vi.hoisted(() => ({ current: null as null | Record<string, unknown> }));
vi.mock("./ManagedTerminal", async () => {
  const { useEffect } = await import("react");
  return {
    ManagedTerminal: (props: { sessionId: number }) => {
      terminalProps.current = props as unknown as Record<string, unknown>;
      useEffect(() => { terminalMounts.count += 1; }, []);
      return <div>PTY {props.sessionId}</div>;
    },
  };
});

import { ChatWindow } from "./ChatWindow";
import { chatUi } from "../test/agents";
import { terminalAttention } from "../terminalAttention";

function respondWithHistory(history: unknown, approval: unknown = null) {
  invoke.mockImplementation((command: string) => {
    if (command === "get_chat_history") return Promise.resolve(history);
    if (command === "get_pending_approval") return Promise.resolve(approval);
    if (command === "managed_terminal_binding") return Promise.resolve(null);
    if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 1, active: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
    return Promise.resolve();
  });
}

afterEach(() => {
  cleanup();
  invoke.mockReset();
  openDialog.mockReset();
  window.history.replaceState({}, "", "/");
  // 侧栏折叠状态持久化在 localStorage，不清会串到下一个用例。
  localStorage.clear();
});

describe("ChatWindow", () => {
  it("renders structured transcript entries", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    respondWithHistory({
      sessionId: 7,
      title: "实现同步对话",
      status: "running",
      provider: "claude",
      cwd: "C:/repo",
      supported: true,
      offset: 120,
      reset: false,
      pendingReview: null,
      items: [
        { type: "user_text", id: "u1", timestamp: null, text: "开始" },
        { type: "assistant_text", id: "a1", timestamp: null, text: "我来实现" },
        { type: "reasoning", id: "r1", timestamp: null, text: "先检查现有协议" },
        { type: "tool_use", id: "t1", timestamp: null, name: "Bash", summary: "cargo test" },
        { type: "tool_result", id: "tr1", timestamp: null, tool_use_id: "t1", text: "ok", is_error: false },
      ],
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("实现同步对话")).toBeTruthy());
    expect(screen.getByText("实现同步对话").hasAttribute("data-tauri-drag-region")).toBe(true);
    // cwd 是「打开项目目录」按钮：可点击、不做拖拽区（拖拽与点击手势会互相吞）。
    const cwd = screen.getByText("C:/repo");
    expect(cwd.tagName).toBe("BUTTON");
    fireEvent.click(cwd);
    expect(invoke).toHaveBeenCalledWith("open_project_dir", { cwd: "C:/repo" });
    expect(screen.getByText("开始")).toBeTruthy();
    expect(screen.getByText("我来实现")).toBeTruthy();
    expect(screen.getByText("先检查现有协议")).toBeTruthy();
    const activity = screen.getByText("执行了 1 次工具调用").closest("details");
    expect(activity?.hasAttribute("open")).toBe(false);
    expect(screen.getAllByText("运行终端").length).toBeGreaterThan(0);
    expect(screen.queryByText("工具结果")).toBeNull();
    expect(invoke).toHaveBeenCalledWith("get_chat_history", { sessionId: 7, offset: 0 });
    fireEvent.change(screen.getByRole("textbox", { name: "发送消息给 Agent" }), { target: { value: "继续实现" } });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "发送消息给 Agent" }), { key: "Enter" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 7, data: "继续实现" }));
    // Enter 在正文之后隔 SUBMIT_GAP_MS 才发（等 TUI 的斜杠补全渲染完，否则 Enter 会被
    // 补全菜单吃掉、只换行不提交），故这里同样要 waitFor。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 7, data: "\r" }));
    fireEvent.click(screen.getByRole("button", { name: "终端" }));
    expect(screen.getByText("PTY 7")).toBeTruthy();
  });

  it("展开子任务时才拉取它的时间线，并嵌套渲染", async () => {
    window.history.replaceState({}, "", "/?sessionId=9");
    const nested = [{
      label: "agent-0", status: "completed",
      items: [
        { type: "assistant_text", id: "s1", timestamp: null, text: "子任务的结论" },
        { type: "tool_use", id: "st1", timestamp: null, name: "Bash", summary: "rg foo" },
      ],
    }];
    respondWithHistory({
      sessionId: 9, title: "派活", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 10, reset: false, pendingReview: null,
      items: [
        { type: "user_text", id: "u1", timestamp: null, text: "查一下" },
        {
          type: "tool_use", id: "toolu_1", timestamp: null, name: "Agent", summary: "验证审批双轨",
          subagent: { description: "验证审批双轨", agent_type: "general-purpose", count: 1 },
        },
      ],
    });
    invoke.mockImplementation((command: string, args: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 9, title: "派活", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 10, reset: false, pendingReview: null,
        items: [
          { type: "user_text", id: "u1", timestamp: null, text: "查一下" },
          {
            type: "tool_use", id: "toolu_1", timestamp: null, name: "Agent", summary: "验证审批双轨",
            subagent: { description: "验证审批双轨", agent_type: "general-purpose", count: 1 },
          },
        ],
      });
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 9, active: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "get_subagent_transcript") {
        expect(args).toEqual({ sessionId: 9, toolUseId: "toolu_1" });
        return Promise.resolve(nested);
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);

    // 子任务自成一行，不被并进「N 个操作」那一坨；摘要显示的是描述而不是整包参数。
    const summary = await screen.findByText("验证审批双轨");
    expect(screen.getByText("子任务")).toBeTruthy();
    expect(screen.getByText("general-purpose")).toBeTruthy();
    expect(screen.queryByText("执行了 1 次工具调用")).toBeNull();
    // 未展开前绝不请求：一个会话可能有几十个子任务。
    expect(invoke).not.toHaveBeenCalledWith("get_subagent_transcript", expect.anything());

    const details = summary.closest("details")!;
    const toggle = (open: boolean) => {
      details.open = open;
      fireEvent(details, new Event("toggle"));
    };
    toggle(true);
    expect(await screen.findByText("子任务的结论")).toBeTruthy();
    // 嵌套时间线沿用同一套渲染：里面的工具调用照样分组。
    expect(screen.getByText("执行了 1 次工具调用")).toBeTruthy();

    // 折叠再展开不该重复请求（结果缓存在组件里）。
    const calls = invoke.mock.calls.filter(([command]) => command === "get_subagent_transcript").length;
    toggle(false);
    toggle(true);
    expect(invoke.mock.calls.filter(([command]) => command === "get_subagent_transcript").length).toBe(calls);
  });

  it("不展开也显示子任务状态：无回执=在跑，有回执=按结局统计", async () => {
    window.history.replaceState({}, "", "/?sessionId=17");
    const swarm = {
      type: "tool_use", id: "tool_s", timestamp: null, name: "AgentSwarm", summary: "分组审查",
      subagent: { description: "分组审查", agent_type: "explore", count: 3 },
    };
    let done = false;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 17, title: "批量", status: "running", provider: "kimi", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null,
        items: done
          ? [swarm, {
              type: "tool_result", id: "r1", timestamp: null, tool_use_id: "tool_s",
              text: "done", is_error: false,
              subagent: { running: 0, completed: 2, failed: 1 },
            }]
          // 一批 fan-out 的结局要等整批跑完才写进主链——跑着的时候主链上没有回执。
          : [swarm],
      });
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 17, active: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("kimi"));
      return Promise.resolve();
    });
    render(<ChatWindow />);

    // 没有回执 → 三个都在跑，且**不必展开**（不该为一个徽标去读侧车流）。
    expect(await screen.findByText("3 进行中")).toBeTruthy();
    expect(invoke).not.toHaveBeenCalledWith("get_subagent_transcript", expect.anything());

    // 回执到达后按真实结局显示（靠历史轮询自然刷新，不手动重渲染）。
    done = true;
    await waitFor(() => expect(screen.getAllByText("2 完成 · 1 失败").length).toBeGreaterThan(0), { timeout: 3_000 });
    expect(screen.queryByText("3 进行中")).toBeNull();
    expect(invoke).not.toHaveBeenCalledWith("get_subagent_transcript", expect.anything());
  });

  it("交互式菜单型 CLI：切换模型发出 /model，再把弹出的菜单转成按钮", async () => {
    window.history.replaceState({}, "", "/?sessionId=18");
    // 真机抓屏形态（见 app/src-tauri/tests/capture_model_menu.rs）：无编号，靠 ❯ 标当前项。
    const menu = [
      "\x1b[2J Select a model  (type to search)",
      "  Tab toggle provider · ↑↓ navigate · Enter select · Esc cancel",
      "     K2.7 Coding            Kimi Code",
      "   ❯ K3                     Kimi Code ← current",
    ].join("\r\n");
    let sentMenuCommand = false;
    invoke.mockImplementation((command: string, args: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 18, title: "换模型", status: "running", provider: "kimi", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: "K3",
      });
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("kimi"));
      if (command === "write_managed_terminal" && args.data === "/model") sentMenuCommand = true;
      if (command === "managed_terminal_snapshot") {
        // 命令发出后，CLI 把菜单画到屏幕上。
        return Promise.resolve({
          sessionId: 18, active: true,
          data: sentMenuCommand ? btoa(menu) : btoa("ready"),
          startOffset: 0, endOffset: sentMenuCommand ? 400 : 5, exited: false, exitCode: null,
        });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);

    // kimi 没有静态预设，按钮改为「发命令打开 CLI 自己的菜单」。
    const button = await screen.findByRole("button", { name: "切换模型" });
    fireEvent.click(button);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 18, data: "/model" }));

    // 识别窗口已打开，交给终端组件去认（真实组件读 xterm 画面，这里直接喂真实解析结果）。
    await waitFor(() => expect(terminalProps.current?.expectMenu).toBe(true));
    const attention = terminalAttention(menu, [], false, true);
    expect(attention?.id).toBe("interactive:cursor-menu");
    act(() => { (terminalProps.current?.onAttention as (a: unknown) => void)(attention); });

    // 菜单被渲染成 GUI 选项；选项文字来自 CLI 现给的清单，宿主没有维护一份。
    const choice = await screen.findByRole("button", { name: /K2\.7 Coding/ }, { timeout: 3_000 });
    fireEvent.click(choice);
    // 菜单首尾循环：从 ❯（K3）上移一格到 K2.7 再回车。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 18, data: "\x1b[A\r" }));
    // 切换模型不产生 Stop hook，模型不会自己落库；必须主动刷一次，
    // 否则对话页与贴纸会一直挂着旧模型直到下一条消息跑完。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("refresh_session_model", { sessionId: 18 }), { timeout: 3_000 });
  });

  it("菜单已打开时再点不重发命令（否则会打进搜索框把候选全过滤掉）", async () => {
    window.history.replaceState({}, "", "/?sessionId=19");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 19, title: "换模型", status: "running", provider: "kimi", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: "K3",
      });
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("kimi"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 19, active: true, data: btoa("ready"), startOffset: 0, endOffset: 5, exited: false, exitCode: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const button = await screen.findByRole("button", { name: "切换模型" });
    const sentModel = () => invoke.mock.calls.filter(([command, args]) =>
      command === "write_managed_terminal" && (args as { data?: string }).data === "/model").length;

    fireEvent.click(button);
    await waitFor(() => expect(sentModel()).toBe(1));
    // 识别窗口开着时再点：只收起（发 Esc），绝不重发——重发会变成 `Search: /model/model`。
    await waitFor(() => expect(terminalProps.current?.expectMenu).toBe(true));
    fireEvent.click(button);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 19, data: "\x1b" }));
    expect(sentModel()).toBe(1);
  });

  it("shows the provider capability fallback", async () => {
    window.history.replaceState({}, "", "/?sessionId=8");
    respondWithHistory({
      sessionId: 8, title: "Codex", status: "ended", provider: "codex", cwd: null,
      supported: false, offset: 0, reset: false, pendingReview: null, items: [],
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("这个 Agent 暂未提供结构化对话记录")).toBeTruthy());
  });

  it("running session with no entries says the agent is working, not that there is nothing", async () => {
    // 刚启动的会话：hook 已入库（running）但 transcript 还没落第一条。此时「还没有可显示的
    // 对话记录」与下方的运行指示自相矛盾——空列表 ≠ 没在干活。
    window.history.replaceState({}, "", "/?sessionId=41");
    respondWithHistory({
      sessionId: 41, title: "刚启动", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null,
      currentActivity: null, items: [],
    });
    render(<ChatWindow />);
    expect(await screen.findByText("Agent 已开始工作，对话内容马上出现")).toBeTruthy();
    expect(screen.queryByText("还没有可显示的对话记录")).toBeNull();
    // 会话结束且确实没有记录时，仍然如实说「没有」。
  });

  it("renders the hook-recorded exchange while the transcript has not landed yet", async () => {
    // transcript 未落盘/未定位到 ≠ 什么都不知道：UserPromptSubmit / Stop 已把最近一问一答
    // 落进 DB（lastUserText / lastAiText），空窗期先渲染它们，而不是一句占位文案。
    window.history.replaceState({}, "", "/?sessionId=42");
    respondWithHistory({
      sessionId: 42, title: "空窗期", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null,
      lastUserText: "帮我修这个 bug", lastAiText: "我先复现一下", items: [],
    });
    render(<ChatWindow />);
    expect(await screen.findByText("帮我修这个 bug")).toBeTruthy();
    expect(screen.getByText("我先复现一下")).toBeTruthy();
    expect(screen.queryByText("Agent 已开始工作，对话内容马上出现")).toBeNull();
  });

  it("shows the hook-recorded exchange for agents without structured transcripts", async () => {
    // 不提供结构化 transcript 的 agent：hook 数据仍是真实内容，「暂未提供」降为注脚。
    window.history.replaceState({}, "", "/?sessionId=43");
    respondWithHistory({
      sessionId: 43, title: "无结构化记录", status: "running", provider: "gemini", cwd: null,
      supported: false, offset: 0, reset: false, pendingReview: null,
      lastUserText: "整理下这份文档", items: [],
    });
    render(<ChatWindow />);
    expect(await screen.findByText("整理下这份文档")).toBeTruthy();
    expect(screen.getByText("这个 Agent 暂未提供结构化对话记录")).toBeTruthy();
  });

  it("deduplicates adjacent equivalent Kimi user records", async () => {
    window.history.replaceState({}, "", "/?sessionId=18");
    respondWithHistory({
      sessionId: 18, title: "Kimi", status: "ended", provider: "kimi", cwd: null,
      supported: true, offset: 100, reset: false, pendingReview: null,
      items: [
        { type: "user_text", id: "turn", timestamp: null, text: "同一条输入" },
        { type: "user_text", id: "append", timestamp: null, text: "同一条输入" },
      ],
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getAllByText("同一条输入")).toHaveLength(1));
  });

  it("opens a pending managed launch directly in the terminal", async () => {
    window.history.replaceState({}, "", "/?sessionId=-3");
    invoke.mockResolvedValue(null);
    render(<ChatWindow />);
    expect(await screen.findByText("PTY -3")).toBeTruthy();
    expect(invoke).toHaveBeenCalledWith("managed_terminal_binding", { sessionId: -3 });
    expect(invoke).not.toHaveBeenCalledWith("get_chat_history", expect.anything());
  });

  it("merges streaming assistant deltas into one message", async () => {
    window.history.replaceState({}, "", "/?sessionId=9");
    respondWithHistory({
      sessionId: 9, title: "Kimi", status: "running", provider: "kimi", cwd: null,
      supported: true, offset: 2, reset: false, pendingReview: null, items: [
        { type: "user_text", id: "u", timestamp: null, text: "继续" },
        { type: "assistant_delta", id: "d1", timestamp: null, text: "正在" },
        { type: "assistant_delta", id: "d2", timestamp: null, text: "处理" },
      ],
    });
    render(<ChatWindow />);
    expect(await screen.findByText("正在处理")).toBeTruthy();
    expect(screen.queryByText("正在")).toBeNull();
  });

  it("sends selected images and files through the managed PTY", async () => {
    window.history.replaceState({}, "", "/?sessionId=11");
    respondWithHistory({
      sessionId: 11, title: "附件", status: "running", provider: "codex", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    });
    openDialog.mockResolvedValue(["C:/tmp/design.png", "C:/tmp/spec.pdf"]);
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "添加图片或文件" }));
    expect(await screen.findByText("design.png")).toBeTruthy();
    expect(screen.getByText("spec.pdf")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", {
      sessionId: 11,
      data: expect.stringContaining("C:/tmp/design.png"),
    }));
  });

  it("automatically resumes an inactive managed terminal before sending", async () => {
    window.history.replaceState({}, "", "/?sessionId=13");
    const history = {
      sessionId: 13, title: "恢复", status: "ended", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    };
    respondWithHistory(history);
    let started = false;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "start_managed_terminal") { started = true; return Promise.resolve(); }
      if (command === "managed_terminal_snapshot") {
        // endOffset 是「已产生多少输出」的判据（data 现在是 base64 增量，可能为空）；
        // 就绪判定还要求 data 里有可见文本（纯控制序列不算）。
        return Promise.resolve(started
          ? { sessionId: 13, active: true, data: btoa("ready"), startOffset: 0, endOffset: 5, exited: false, exitCode: null }
          : { sessionId: 13, active: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("textbox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "继续" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("start_managed_terminal", {
      sessionId: 13, cols: 100, rows: 30,
    }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 13, data: "继续" }), { timeout: 2_000 });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 13, data: "\r" }));
  });

  it("opens a non-Claude startup trust prompt in the terminal without typing the chat message into it", async () => {
    window.history.replaceState({}, "", "/?sessionId=14");
    const history = {
      sessionId: 14, title: "待信任目录", status: "ended", provider: "codex", cwd: "C:/new-repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    };
    let started = false;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("codex"));
      if (command === "start_managed_terminal") { started = true; return Promise.resolve(); }
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve(started
          ? {
              sessionId: 14, active: true,
              data: btoa("\x1b[2JDo you trust the contents of this directory?\r\n> 1. Yes, continue\r\n  2. No, quit"),
              startOffset: 0, endOffset: 76, exited: false, exitCode: null,
            }
          : { sessionId: 14, active: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("textbox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "继续修复" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    expect((await screen.findByRole("alert")).textContent).toContain("是否信任此文件夹？");
    expect(screen.queryByRole("textbox", { name: "发送消息给 Agent" })).toBeNull();
    expect(screen.getByText("PTY 14")).toBeTruthy();
    expect(screen.getByRole("button", { name: "对话" }).className).toContain("is-active");
    expect(screen.getByText("PTY 14").closest(".chat-terminal-pane")?.className).toContain("is-background");
    expect(screen.getByText("PTY 14").closest(".chat-terminal-pane")?.getAttribute("aria-hidden")).toBe("true");
    expect(invoke).not.toHaveBeenCalledWith("write_managed_terminal", { sessionId: 14, data: "继续修复" });
    expect((input as HTMLTextAreaElement).value).toBe("继续修复");
    // 原始终端页已经显示 TUI，不再叠加 GUI 卡片；切回对话后仍可直接点击结构化选项。
    fireEvent.click(screen.getByRole("button", { name: "终端" }));
    expect(screen.queryByRole("alert")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "对话" }));
    fireEvent.click(await screen.findByRole("button", { name: "Yes, continue" }));
    // 光标已停在第一项：相对移动为 0，直接回车确认，不再盲按上键绕圈。
    expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 14, data: "\r" });
  });

  it("shows a managed PTY startup choice when the conversation opens without visiting Terminal", async () => {
    window.history.replaceState({}, "", "/?sessionId=44");
    const history = {
      // Agent 等用户处理启动选择时，reporter 可能已把会话从 running 标成 waiting。
      sessionId: 44, title: "后台信任提示", status: "waiting", provider: "claude", cwd: "C:/new-repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    };
    const prompt = "\x1b[2JDo you trust the files in this folder?\r\n> 1. Yes, I trust this folder\r\n  2. No, exit";
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ sessionId: 44, active: true, data: btoa(prompt), startOffset: 0, endOffset: prompt.length, exited: false, exitCode: null });
      }
      return Promise.resolve(null);
    });
    render(<ChatWindow />);

    expect(await screen.findByRole("button", { name: "Yes, I trust this folder" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "对话" }).className).toContain("is-active");
    expect(invoke).not.toHaveBeenCalledWith("start_managed_terminal", expect.anything());
  });

  it("renders assistant markdown but keeps user text verbatim", async () => {
    window.history.replaceState({}, "", "/?sessionId=21");
    respondWithHistory({
      sessionId: 21, title: "MD", status: "running", provider: "claude", cwd: null,
      supported: true, offset: 0, reset: false, pendingReview: null,
      items: [
        { type: "user_text", id: "u1", timestamp: null, text: "# 不是标题" },
        { type: "assistant_text", id: "a1", timestamp: null, text: "看 **重点** 和 `code`，详见 [官网](https://example.com/docs)" },
        { type: "assistant_text", id: "a2", timestamp: null, text: "```\n┌─────┐\n│ 会话A │\n└─────┘\n```" },
      ],
    });
    render(<ChatWindow />);
    const strong = await screen.findByText("重点");
    expect(strong.tagName).toBe("STRONG");
    expect(screen.getByText("code").tagName).toBe("CODE");
    // 用户消息按原文展示：行首 # 不得升格成标题。
    const user = screen.getByText("# 不是标题");
    expect(user.tagName).not.toMatch(/^H[1-6]$/);
    // 链接不许让 webview 导航（这个窗口跳走就回不来了），必须交给后端开默认浏览器。
    const link = screen.getByRole("link", { name: "官网" });
    fireEvent.click(link);
    expect(invoke).toHaveBeenCalledWith("open_link", { url: "https://example.com/docs" });
    expect(window.location.href).not.toContain("example.com");
    // 含框线字符的代码块被钉到字符网格：中文锁 2ch 盒子（renderGrid 拆成单字符 span），
    // 整块标记 chat-md-diagram；普通行内代码不受牵连、不被拆分。
    const wide = screen.getByText("话");
    expect(wide.className).toBe("chat-md-cell2");
    expect(wide.closest("code")?.className).toContain("chat-md-diagram");
    expect(screen.getByText("code").className).not.toContain("chat-md-diagram");
  });

  it("shows agent badge, running pulse, slash completions and model switcher", async () => {
    window.history.replaceState({}, "", "/?sessionId=31");
    const history = {
      sessionId: 31, title: "运行观察", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null,
      model: "Opus", contextPct: 63, contextWindow: 200000, currentActivity: "Bash: cargo test",
      items: [{ type: "user_text", id: "u1", timestamp: null, text: "跑" }],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "get_pending_approval") return Promise.resolve(null);
      // 斜杠补全与模型预设不是前端硬编码表：按会话查 agent_chat_ui（内置表 ∪ 自定义命令）。
      if (command === "agent_chat_ui") {
        return Promise.resolve(chatUi("claude", [
          { name: "/deploy", description: "部署到测试环境", source: "project" },
        ]));
      }
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ sessionId: 31, active: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findByText("运行观察");
    // agent logo（标题栏最前，aria-label=provider）+ 运行指示（有活动时显示活动文本）。
    expect(screen.getByLabelText("claude")).toBeTruthy();
    expect(screen.getByText("Bash: cargo test")).toBeTruthy();
    // 上下文用量环：环内百分比 + 环右已用/总量（63% × 200K ≈ 126K）。
    expect(screen.getByText("63")).toBeTruthy();
    expect(screen.getByText("126K/200K")).toBeTruthy();
    // "/" 前缀弹补全；选中后填入输入框并留出参数位，不自动发送。
    const input = screen.getByRole("textbox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "/mo" } });
    fireEvent.click(screen.getByRole("option", { name: /^\/model/ }));
    expect(input.value).toBe("/model ");
    // 自定义命令来自安装实况（agent_chat_ui 从项目目录发现的），描述取自命令文件头。
    fireEvent.change(input, { target: { value: "/de" } });
    // accessible-name 会按 DOM 实现把相邻 code/span 拼成有空格或无空格，两种都等价。
    fireEvent.click(screen.getByRole("option", { name: /^\/deploy\s*部署到测试环境/ }));
    expect(input.value).toBe("/deploy ");
    // 模型菜单：选择预设即向 PTY 发送 /model <id>。
    fireEvent.click(screen.getByRole("button", { name: "切换模型" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /^Sonnet/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 31, data: "/model sonnet" }));
  });

  it("keeps probing a pending runtime skill listing and exposes code-review when it arrives", async () => {
    window.history.replaceState({}, "", "/?sessionId=32");
    let offset = 1;
    let uiCalls = 0;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 32, title: "技能发现", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset, reset: false, pendingReview: null, items: [],
      });
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ sessionId: 32, active: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      if (command === "agent_chat_ui") {
        uiCalls += 1;
        const base = chatUi("claude")!;
        return Promise.resolve(offset === 1
          ? { ...base, runtime_commands_pending: true }
          : {
              ...base,
              runtime_commands_pending: false,
              slash_commands: [...base.slash_commands, {
                name: "/code-review", description: "Review the current diff", source: "builtin" as const,
              }],
            });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(uiCalls).toBeGreaterThan(0));
    offset = 2;

    const input = await screen.findByRole("textbox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "/code" } });
    // 探测有 2s 限频（避免随 650ms 轮询打满后端），等待窗口相应放宽。
    expect(await screen.findByRole("option", { name: /\/code-review/ }, { timeout: 4_000 })).toBeTruthy();
    expect(uiCalls).toBeGreaterThan(1);
  });

  it("still reflects metadata changes despite the re-render short-circuit", async () => {
    // sameHistoryMeta 保留旧引用来跳过稳态重渲染；漏掉某个字段就会「数据变了界面不动」。
    // 这里逐个字段改动并断言 UI 跟上，锁住那份比较清单。
    window.history.replaceState({}, "", "/?sessionId=21");
    const base = {
      sessionId: 21, title: "初始标题", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null,
      model: "Opus", agentModes: [{ dimension: "permission", value: "default" }], contextPct: 10, contextWindow: 200000,
      currentActivity: "Bash: 第一步", items: [],
    };
    let current: Record<string, unknown> = { ...base };
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(current);
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ sessionId: 21, active: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findByText("初始标题");
    expect(screen.getByText("Bash: 第一步")).toBeTruthy();
    expect(screen.getByText("10")).toBeTruthy();
    expect(screen.getByText("权限模式: 默认")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "切换模式: 权限模式" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 21, data: "\u001b[Z" }));

    // 逐字段单独改：若合并成一次改动，任一字段触发的重渲染都会把其它字段的漏判一并掩盖，
    // 测试就变成假绿（验证过：合并写法下从比较清单里删掉 currentActivity 仍然通过）。
    current = { ...base, currentActivity: "Bash: 第二步" };
    expect(await screen.findByText("Bash: 第二步")).toBeTruthy();

    current = { ...base, currentActivity: "Bash: 第二步", contextPct: 42 };
    expect(await screen.findByText("42")).toBeTruthy();

    current = { ...base, currentActivity: "Bash: 第二步", contextPct: 42, title: "改后标题" };
    expect(await screen.findByText("改后标题")).toBeTruthy();

    current = { ...base, currentActivity: "Bash: 第二步", contextPct: 42, title: "改后标题", agentModes: [{ dimension: "permission", value: "plan" }] };
    expect(await screen.findByText("权限模式: 计划")).toBeTruthy();

    // 兜底时间线读 lastUserText/lastAiText（transcript 空窗期渲染 hook 落库的最近往来），
    // 它们也在比较清单里——漏掉的话空窗期内容永远停在第一轮。
    current = { ...base, currentActivity: "Bash: 第二步", contextPct: 42, title: "改后标题", agentModes: [{ dimension: "permission", value: "plan" }], lastUserText: "hook 落库的提问" };
    expect(await screen.findByText("hook 落库的提问")).toBeTruthy();
  });

  it("renders Codex mode dimensions and sends direct Kimi mode actions", async () => {
    window.history.replaceState({}, "", "/?sessionId=41");
    const history = {
      sessionId: 41, title: "Kimi 模式", status: "running", provider: "kimi", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, model: null,
      agentModes: [
        { dimension: "work", value: "default" },
        { dimension: "permission", value: "manual" },
      ],
      contextPct: null, contextWindow: null, currentActivity: null, hasMore: false, items: [],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ sessionId: 41, active: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      if (command === "agent_chat_ui") return Promise.resolve({
        slash_commands: [], model_presets: [], version: "0.26.0",
        mode_controls: [
          {
            dimension: "work", cycle_input: "\u001b[Z", options: [
              { value: "default", inputs: [{ data: "/plan off", submit: true }] },
              { value: "plan", inputs: [{ data: "/plan on", submit: true }] },
            ],
          },
          {
            dimension: "permission", cycle_input: null, options: [
              { value: "manual", inputs: [{ data: "/yolo off", submit: true }, { data: "/auto off", submit: true }] },
              { value: "yolo", inputs: [{ data: "/yolo on", submit: true }] },
              { value: "auto", inputs: [{ data: "/auto on", submit: true }] },
            ],
          },
        ],
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findByText("工作模式: 默认");
    expect(screen.getByText("权限模式: 手动确认")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "切换模式: 权限模式" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "YOLO" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 41, data: "/yolo on" }));
    // Enter 隔 SUBMIT_GAP_MS 才发（见 submitToTerminal），同样要 waitFor。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 41, data: "\r" }));
    expect(await screen.findByText("权限模式: YOLO")).toBeTruthy();
  });

  it("offers to load earlier messages when the first read was truncated", async () => {
    window.history.replaceState({}, "", "/?sessionId=33");
    const truncated = {
      sessionId: 33, title: "长会话", status: "running", provider: "claude", cwd: null,
      supported: true, offset: 500, reset: false, pendingReview: null,
      model: null, contextPct: null, contextWindow: null, currentActivity: null,
      hasMore: true,
      items: [{ type: "user_text", id: "recent", timestamp: null, text: "最近的消息" }],
    };
    // 增量轮询恒为 hasMore:false——提示不能因此闪掉。
    const incremental = { ...truncated, items: [], hasMore: false };
    let firstRead = true;
    invoke.mockImplementation((command: string, args: { full?: boolean }) => {
      if (command === "get_chat_history") {
        if (args?.full) {
          return Promise.resolve({
            ...truncated, hasMore: false,
            items: [
              { type: "user_text", id: "old", timestamp: null, text: "很早以前的消息" },
              { type: "user_text", id: "recent", timestamp: null, text: "最近的消息" },
            ],
          });
        }
        if (firstRead) { firstRead = false; return Promise.resolve(truncated); }
        return Promise.resolve(incremental);
      }
      if (command === "get_pending_approval") return Promise.resolve(null);
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findByText("最近的消息");
    const button = await screen.findByRole("button", { name: "加载更早的对话" });
    // 被裁掉的消息此刻不在 DOM 里——这正是首屏省下的成本。
    expect(screen.queryByText("很早以前的消息")).toBeNull();

    fireEvent.click(button);
    expect(await screen.findByText("很早以前的消息")).toBeTruthy();
    expect(invoke).toHaveBeenCalledWith("get_chat_history", { sessionId: 33, offset: 0, full: true });
    // 取完整历史后提示消失，且不重复插入已有消息。
    await waitFor(() => expect(screen.queryByRole("button", { name: "加载更早的对话" })).toBeNull());
    expect(screen.getAllByText("最近的消息")).toHaveLength(1);
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
    fireEvent.click(screen.getByRole("button", { name: "对话" }));
    fireEvent.click(screen.getByRole("button", { name: "终端" }));
    expect(terminalMounts.count).toBe(1);
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
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 7, active: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
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
    expect(screen.queryByRole("textbox", { name: "发送消息给 Agent" })).toBeNull();
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

  it("外部占用时就地给出接管入口，确认后重放刚才那次发送", async () => {
    window.history.replaceState({}, "", "/?sessionId=15");
    const history = {
      sessionId: 15, title: "外部运行中", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    };
    let takenOver = false;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") {
        // 接管前没有托管 PTY；接管后有，且已画出可见内容。
        return Promise.resolve(takenOver
          ? { sessionId: 15, active: true, data: btoa("ready"), startOffset: 0, endOffset: 5, exited: false, exitCode: null }
          : { sessionId: 15, active: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      // 进程是否真活着由后端按 pid 判定——前端不再靠 status 猜，而是让这次 start 被拒。
      if (command === "start_managed_terminal") return Promise.reject("会话仍在外部终端运行，不能重复接管");
      if (command === "takeover_managed_terminal") { takenOver = true; return Promise.resolve(); }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("textbox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "别起第二个" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    expect(await screen.findByText(/会话仍在外部终端运行/)).toBeTruthy();
    // 输入不能丢：接管后要原样重发这条。
    expect(input.value).toBe("别起第二个");
    expect(invoke).not.toHaveBeenCalledWith("write_managed_terminal", expect.anything());

    // 接管是破坏性的（杀掉外部进程），必须显式确认；取消则什么都不做。
    confirmDialog.mockResolvedValueOnce(false);
    fireEvent.click(screen.getByRole("button", { name: "结束外部进程并接管" }));
    await waitFor(() => expect(confirmDialog).toHaveBeenCalled());
    expect(invoke).not.toHaveBeenCalledWith("takeover_managed_terminal", expect.anything());

    confirmDialog.mockResolvedValueOnce(true);
    fireEvent.click(screen.getByRole("button", { name: "结束外部进程并接管" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("takeover_managed_terminal", { sessionId: 15, cols: 100, rows: 30 }));
    // 接管成功后自动重放刚才那次发送，用户不必重打一遍。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 15, data: "别起第二个" }), { timeout: 3_000 });
  });

  it("外部会话其实已经死了（status 陈旧）时，直接起托管终端而不是一律拒绝", async () => {
    window.history.replaceState({}, "", "/?sessionId=16");
    // status 仍是 running/stale，但进程早没了——后端 pid 判定会放行这次 start。
    const history = {
      sessionId: 16, title: "陈旧状态", status: "stale", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    };
    let started = false;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "start_managed_terminal") { started = true; return Promise.resolve(); }
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve(started
          ? { sessionId: 16, active: true, data: btoa("ready"), startOffset: 0, endOffset: 5, exited: false, exitCode: null }
          : { sessionId: 16, active: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("textbox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "继续" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("start_managed_terminal", { sessionId: 16, cols: 100, rows: 30 }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 16, data: "继续" }), { timeout: 3_000 });
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
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") {
        snapshotCalls += 1;
        return Promise.resolve(snapshotCalls === 1
          ? { sessionId: 14, active: false, data: "", exited: false, exitCode: null }
          : { sessionId: 14, active: false, data: "launch error", exited: true, exitCode: 1 });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("textbox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "不要丢失" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(await screen.findByText(/Agent 启动后立即退出（退出码 1）/)).toBeTruthy();
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
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 45, active: true, data: btoa(prompt), startOffset: 0, endOffset: prompt.length,
        exited: false, exitCode: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);

    expect(await screen.findByRole("button", { name: "允许一次" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "允许并记住 · cargo build *" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "拒绝" })).toBeTruthy();
    expect(screen.getByText(/cargo build -p meowo-agent/)).toBeTruthy();
    expect(screen.getByText("Do you want to proceed?")).toBeTruthy();
    expect(screen.queryByText("该请求来自非托管会话，请在原终端中处理")).toBeNull();
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
      if (command === "get_pending_approval") return Promise.resolve(null);
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 46, active: true, data: btoa(prompt), startOffset: 0, endOffset: prompt.length,
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
      if (command === "get_pending_approval") return Promise.resolve(pending);
      if (command === "resolve_pending_approval") { pending = null; return Promise.resolve(); }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    expect(await screen.findByText("运行测试")).toBeTruthy();
    expect(screen.getByText("{\"command\":\"cargo test\"}")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /^允许并记住（此项目、本机）/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("resolve_pending_approval", {
      sessionId: 12, requestId: "request-1", choice: "suggestion:0",
    }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "允许一次" })).toBeNull());
    expect(screen.queryByText("该请求来自非托管会话，请在原终端中处理")).toBeNull();
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
      if (command === "get_pending_approval") return Promise.resolve({
        sessionId: 12, requestId: "request-lean", provider: "codex", toolName: "Bash",
        description: "运行测试", input: "{\"command\":\"cargo test\"}",
        // 刻意没有 permissionSuggestions —— 模拟被 skip 掉字段的瘦负载。
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    // 审批条正常出现：允许/拒绝都在，只是没有「记住」类按钮。
    expect(await screen.findByRole("button", { name: "允许一次" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "拒绝" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /允许并记住/ })).toBeNull();
  });
});
