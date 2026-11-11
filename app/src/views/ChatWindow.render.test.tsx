// 渲染对话记录、附件发送、恢复与草稿、模型和模式胶囊。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke, openDialog, setTitleMock, terminalProps, pickAttachments, b64utf8, respondWithHistory } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";
import { chatUi, descriptors } from "../test/agents";
import { terminalAttention } from "../terminalAttention";

describe("ChatWindow", () => {
  /** 手打 `/add-dir "<path>"`（7T-4 尾，用户实拍）：claude 的 /add-dir 不剥引号，成对引号会
   *  进路径字面量。这条命令改走 add_session_extra_dir（后端剥引号 + 校验 + 落库 + 自己写
   *  PTY），前端**不再**往终端写一遍；后端拒绝时回退成剥了引号的原命令照常发送。 */
  const addDirHistory = {
    sessionId: 7, title: "附加目录会话", status: "idle", provider: "claude", cwd: "C:/repo",
    supported: true, offset: 1, reset: false, pendingReview: null,
    items: [{ type: "user_text", id: "u1", timestamp: null, text: "开始" }],
  };
  const typeAddDir = async () => {
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("附加目录会话")).toBeTruthy());
    const box = screen.getByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(box, { target: { value: '/add-dir "C:\\work\\x y"' } });
    fireEvent.keyDown(box, { key: "Enter" });
    return box as HTMLTextAreaElement;
  };
  // 附加目录功能已去掉：Meowo 不再替 /add-dir 落库，只剥掉引号原样交给 CLI。
  it("手打 /add-dir 只剥引号原样发给 CLI，不再落库", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    respondWithHistory(addDirHistory);
    await typeAddDir();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 7, data: "/add-dir C:\\work\\x y" }));
    expect(invoke.mock.calls.some(([command]) => command === "add_session_extra_dir")).toBe(false);
  });

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
    // 标题是动作菜单触发钮（Kimi 式）：主仓路径入口在菜单的「打开项目目录」;
    // 多仓的目录清单与附加管理在 diff 面板头行的仓菜单(用户指定)。
    fireEvent.click(screen.getByRole("button", { name: "会话操作" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "打开项目目录" }));
    expect(invoke).toHaveBeenCalledWith("open_project_dir", { cwd: "C:/repo" });
    expect(screen.getByText("开始")).toBeTruthy();
    expect(screen.getByText("我来实现")).toBeTruthy();
    // 思考 + 工具两条收成一个汇总块；点开汇总块看逐条。
    const fold = document.querySelector<HTMLButtonElement>(".turn-fold-head");
    if (fold?.getAttribute("aria-expanded") === "false") fireEvent.click(fold);
    // 思考过程恒为收起的一行（用户对照 Kimi 定），内容仍在 DOM 里供点开。
    const reasoning = screen.getByText("先检查现有协议").closest("details");
    expect(reasoning?.hasAttribute("open")).toBe(false);
    // 每条工具调用独立成行（Kimi 式扁平行），不再有分组卡片；有回执的调用默认收起。
    const activity = screen.getAllByText("运行终端")[0].closest("details");
    expect(activity?.className).toContain("chat-tool");
    expect(activity?.hasAttribute("open")).toBe(false);
    expect(screen.queryByText("工具结果")).toBeNull();
    expect(invoke).toHaveBeenCalledWith("get_chat_history", { sessionId: 7, offset: 0 });
    fireEvent.change(screen.getByRole("combobox", { name: "发送消息给 Agent" }), { target: { value: "继续实现" } });
    fireEvent.keyDown(screen.getByRole("combobox", { name: "发送消息给 Agent" }), { key: "Enter" });
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
      // 同 respondWithHistory：只有 offset 落后时才回 items，重复整批会在慢机上被追加两遍。
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 9, title: "派活", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 10, reset: false, pendingReview: null,
        items: (args.offset as number) >= 10 ? [] : [
          { type: "user_text", id: "u1", timestamp: null, text: "查一下" },
          {
            type: "tool_use", id: "toolu_1", timestamp: null, name: "Agent", summary: "验证审批双轨",
            subagent: { description: "验证审批双轨", agent_type: "general-purpose", count: 1 },
          },
        ],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 9, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
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
    expect(document.querySelector(".chat-activity-group")).toBeNull();
    // 未展开前绝不请求：一个会话可能有几十个子任务。
    expect(invoke).not.toHaveBeenCalledWith("get_subagent_transcript", expect.anything());

    const details = summary.closest("details")!;
    const toggle = (open: boolean) => {
      details.open = open;
      fireEvent(details, new Event("toggle"));
    };
    toggle(true);
    expect(await screen.findByText("子任务的结论")).toBeTruthy();
    // 嵌套时间线沿用同一套渲染：里面的工具调用也是扁平行。
    expect(document.querySelector(".chat-subagent .chat-flat-steps .chat-tool")).toBeTruthy();

    // 折叠再展开不该重复请求（结果缓存在组件里）。
    const calls = invoke.mock.calls.filter(([command]) => command === "get_subagent_transcript").length;
    toggle(false);
    toggle(true);
    expect(invoke.mock.calls.filter(([command]) => command === "get_subagent_transcript").length).toBe(calls);
  });

  it("子任务时间线里出现派出它自己的调用（fork 回声）时不再嵌套", async () => {
    window.history.replaceState({}, "", "/?sessionId=9");
    const launch = {
      type: "tool_use", id: "toolu_1", timestamp: null, name: "Agent", summary: "web 端重测",
      subagent: { description: "web 端重测", agent_type: "fork", count: 1 },
    };
    invoke.mockImplementation((command: string, args: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 9, title: "派活", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 10, reset: false, pendingReview: null,
        items: (args.offset as number) >= 10 ? [] : [{ type: "user_text", id: "u1", timestamp: null, text: "查一下" }, launch],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 9, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      // fork 的侧车开头带着派出它的那条调用，与主链同 id（实拍 2.1.317）。后端已滤掉；
      // 这里模拟格式变了没滤掉的情况，前端不得再挂子任务块，否则展开又拉回自己，无限嵌套。
      if (command === "get_subagent_transcript") return Promise.resolve([{ label: "agent-0", status: "running", items: [launch] }]);
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const details = (await screen.findByText("web 端重测")).closest("details")!;
    details.open = true;
    fireEvent(details, new Event("toggle"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_subagent_transcript", { sessionId: 9, toolUseId: "toolu_1" }));
    await waitFor(() => expect(document.querySelector(".chat-subagent-scroll")).toBeTruthy());
    expect(document.querySelectorAll(".chat-subagent")).toHaveLength(1);
    expect(invoke.mock.calls.filter(([command]) => command === "get_subagent_transcript")).toHaveLength(1);
  });

  it("不展开也显示子任务状态：无回执=在跑，有回执=按结局统计", async () => {
    window.history.replaceState({}, "", "/?sessionId=17");
    const swarm = {
      type: "tool_use", id: "tool_s", timestamp: "2026-08-18T04:00:00.000Z", name: "AgentSwarm", summary: "分组审查",
      subagent: { description: "分组审查", agent_type: "explore", count: 3 },
    };
    let done = false;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 17, title: "批量", status: "running", connected: true, provider: "kimi", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null,
        items: done
          ? [swarm, {
              type: "tool_result", id: "r1", timestamp: "2026-08-18T04:06:40.000Z", tool_use_id: "tool_s",
              text: "done", is_error: false,
              subagent: { running: 0, completed: 2, failed: 1 },
            }]
          // 一批 fan-out 的结局要等整批跑完才写进主链——跑着的时候主链上没有回执。
          : [swarm],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 17, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("kimi"));
      return Promise.resolve();
    });
    render(<ChatWindow />);

    // 没有回执 → 三个都在跑，且**不必展开**（不该为一个徽标去读侧车流）。
    expect(await screen.findByText("3 进行中")).toBeTruthy();
    expect(invoke).not.toHaveBeenCalledWith("get_subagent_transcript", expect.anything());
    // 在跑时图标脉冲——折叠状态下不必读徽标小字也能看出里面有活儿。
    expect(document.querySelector(".chat-subagent-icon.is-running")).toBeTruthy();

    // 回执到达后按真实结局显示（靠历史轮询自然刷新，不手动重渲染）。
    done = true;
    await waitFor(() => expect(screen.getAllByText("2 完成 · 1 失败").length).toBeGreaterThan(0), { timeout: 3_000 });
    expect(screen.queryByText("3 进行中")).toBeNull();
    // 结束后:图标停脉冲,显示「委派 → 最终回执」的执行时长(04:00:00 → 04:06:40)。
    expect(document.querySelector(".chat-subagent-icon.is-running")).toBeNull();
    expect(screen.getAllByText("6 分 40 秒").length).toBeGreaterThan(0);
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
        sessionId: 18, title: "换模型", status: "waiting", provider: "kimi", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: "K3",
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("kimi"));
      if (command === "write_managed_terminal" && args.data === "/model") sentMenuCommand = true;
      if (command === "managed_terminal_snapshot") {
        // 命令发出后，CLI 把菜单画到屏幕上。
        return Promise.resolve({
          sessionId: 18, active: true, managed: true,
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

  it("粘贴图片/文件：经宿主落盘拿路径，进入附件条（纯文本粘贴不受影响）", async () => {
    window.history.replaceState({}, "", "/?sessionId=53");
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 53, title: "粘贴", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: null,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 53, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      if (command === "save_pasted_attachment") {
        return Promise.resolve(`C:/tmp/meowo-paste/1-0/${(args as { fileName: string }).fileName}`);
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    // jsdom 的 File/Blob 实现参差，只按组件用到的形状（name + arrayBuffer）伪造。
    const file = { name: "shot.png", arrayBuffer: () => Promise.resolve(new Uint8Array([1, 2, 3]).buffer) } as unknown as File;
    fireEvent.paste(input, { clipboardData: { files: [file] } });
    // 内容经 base64 交给宿主落盘（[1,2,3] → "AQID"），路径回来后按文件名显示为附件。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("save_pasted_attachment", { fileName: "shot.png", dataBase64: "AQID" }));
    // 图片附件渲染为缩略图（ImageRef），文件名在 img 的 alt 上而不再是文本 chip。
    expect(await screen.findByAltText("shot.png")).toBeTruthy();
  });

  it("新会话认领(临时 id → 真 id)时，已粘的图和打的字跟过去", async () => {
    window.history.replaceState({}, "", "/?sessionId=-9");
    let claimed = false;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "managed_terminal_pending_launch") return Promise.resolve({ provider: "claude", cwd: "C:/repo" });
      if (command === "managed_terminal_binding") return Promise.resolve(claimed ? 77 : null);
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 77, title: "认领后", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: null,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: Number(args?.sessionId), active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      if (command === "save_pasted_attachment") {
        return Promise.resolve(`C:/tmp/meowo-paste/1-0/${(args as { fileName: string }).fileName}`);
      }
      return Promise.resolve(null);
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "看看这张图" } });
    const file = { name: "shot.png", arrayBuffer: () => Promise.resolve(new Uint8Array([1, 2, 3]).buffer) } as unknown as File;
    fireEvent.paste(input, { clipboardData: { files: [file] } });
    expect(await screen.findByAltText("shot.png")).toBeTruthy();

    claimed = true;
    await waitFor(() => expect(screen.getByText("认领后")).toBeTruthy());
    expect(screen.getByAltText("shot.png")).toBeTruthy();
    expect((screen.getByRole("combobox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement).value).toBe("看看这张图");
  });

  it("新会话认领撞上图片落盘在途：落盘回来的图不丢", async () => {
    window.history.replaceState({}, "", "/?sessionId=-10");
    let claimed = false;
    let finishSave: (path: string) => void = () => {};
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "managed_terminal_pending_launch") return Promise.resolve({ provider: "claude", cwd: "C:/repo" });
      if (command === "managed_terminal_binding") return Promise.resolve(claimed ? 78 : null);
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 78, title: "认领后在途", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: null,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: Number(args?.sessionId), active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      if (command === "save_pasted_attachment") return new Promise((resolve) => { finishSave = resolve; });
      return Promise.resolve(null);
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    const file = { name: "late.png", arrayBuffer: () => Promise.resolve(new Uint8Array([1]).buffer) } as unknown as File;
    fireEvent.paste(input, { clipboardData: { files: [file] } });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("save_pasted_attachment", expect.anything()));

    claimed = true;
    await waitFor(() => expect(screen.getByText("认领后在途")).toBeTruthy());
    finishSave("C:/tmp/meowo-paste/2-0/late.png");
    expect(await screen.findByAltText("late.png")).toBeTruthy();
  });

  it("手敲交互式内置命令（/config）走菜单识别通道：清空输入框并打开识别窗口", async () => {
    window.history.replaceState({}, "", "/?sessionId=52");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 52, title: "交互命令", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: "Opus",
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ sessionId: 52, active: true, managed: true, data: btoa("ready"), startOffset: 0, endOffset: 5, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    // 等 chatUi（menu_slash_commands 的来源）就位再回车，否则命中判断还没有数据。
    await screen.findByRole("button", { name: "切换模式: 权限模式" });
    fireEvent.change(input, { target: { value: "/config" } });
    fireEvent.keyDown(input, { key: "Enter" });
    // 命令原样送达 CLI，同时打开屏幕识别窗口（expectMenu 传给终端组件）。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 52, data: "/config" }));
    await waitFor(() => expect(terminalProps.current?.expectMenu).toBe(true));
    // 输入框已清空（提交序列含 SUBMIT_GAP_MS 的回车间隔，清空在其后）；识别期间给过渡
    // 横幅（识别不出的面板也有「切到终端/收起」的出口）。
    await waitFor(() => expect((input as HTMLTextAreaElement).value).toBe(""));
    expect(await screen.findByText("命令界面已在终端打开，正在识别可选项…")).toBeTruthy();
    // 识别不出来时「收起」也得真把终端里的菜单关掉（发 Esc）——此前这里是空操作，
    // 看不到终端的手机上菜单就一直开着。
    fireEvent.click(screen.getByRole("button", { name: "收起" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 52, data: "\x1b" }));
    // 普通消息不受影响：带参数的命令不是「裸命令」，不走菜单通道。
  });

  it("窗口关掉后,静默探测的收尾 Esc 不得再写进那个会话（幽灵按键会打断 agent 的回合）", async () => {
    // probeModelMenu 里 `await sendText(...)` 之后才调 endSilentProbe,而 sendText 内含
    // 写后回显校验与拉终端,期间用户完全可能关窗。此前这条路没有活跃会话复核,晚到的
    // Esc 会写进早已离开的会话——CI 实拍中它跨到了后面的用例里(sessionId 对不上)。
    window.history.replaceState({}, "", "/?sessionId=77");
    let failSend: (() => void) | null = null;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 77, title: "换模型", status: "running", provider: "kimi", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: "K3",
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("kimi"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 77, active: true, managed: true, data: btoa("ready"), startOffset: 0, endOffset: 5, exited: false, exitCode: null,
      });
      // 把 /model 的下发**挂住**,模拟「还在途中用户就关窗」;放行时让它失败,于是
      // sendText 返回 false,probeModelMenu 走 `if (!sent) endSilentProbe()` 那条收尾路。
      if (command === "write_managed_terminal" && (args as { data?: string })?.data === "/model") {
        return new Promise<void>((_, reject) => { failSend = () => reject(new Error("发送失败")); });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    await waitFor(() => expect(failSend).toBeTruthy());
    // 关窗,再让在途的那一发失败——收尾的 Esc 就是在这之后发出的。
    cleanup();
    await act(async () => {
      failSend!();
      for (let i = 0; i < 8; i += 1) await Promise.resolve();
    });
    expect(invoke).not.toHaveBeenCalledWith("write_managed_terminal", { sessionId: 77, data: "\x1b" });
  });

  it("菜单已打开时再点不重发命令（否则会打进搜索框把候选全过滤掉）", async () => {
    window.history.replaceState({}, "", "/?sessionId=19");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 19, title: "换模型", status: "running", provider: "kimi", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: "K3",
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("kimi"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 19, active: true, managed: true, data: btoa("ready"), startOffset: 0, endOffset: 5, exited: false, exitCode: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const button = await screen.findByRole("button", { name: "切换模型" });
    const sentModel = () => invoke.mock.calls.filter(([command, args]) =>
      command === "write_managed_terminal" && (args as { data?: string }).data === "/model").length;

    fireEvent.click(button);
    await waitFor(() => expect(sentModel()).toBe(1));
    // 识别尚未确认菜单时再点：不重发，也不盲发可能中断任务的 Esc。
    await waitFor(() => expect(terminalProps.current?.expectMenu).toBe(true));
    fireEvent.click(button);
    expect(invoke).not.toHaveBeenCalledWith("write_managed_terminal", { sessionId: 19, data: "\x1b" });
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
    // 信号由对话末尾的运行行给出：本轮还没有任何输出，说「请求中」。
    expect((await screen.findByText("请求中…")).closest(".chat-running")).toBeTruthy();
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

  // 新建会话(负数临时 id)落在用户偏好的视图上:在对话就显示对话、在终端就显示终端
  // (实拍反馈:此前强制进终端,对话党每次新建都被甩去终端页)。
  it("新建会话:默认(偏好对话)进对话页,渲染启动占位而非谎报「没有记录」", async () => {
    window.history.replaceState({}, "", "/?sessionId=-3");
    invoke.mockResolvedValue(null);
    render(<ChatWindow />);
    expect(await screen.findByText("会话正在启动…")).toBeTruthy();
    expect(invoke).not.toHaveBeenCalledWith("get_chat_history", expect.anything());
  });

  it("新建会话:视图偏好是终端时直接进终端", async () => {
    window.history.replaceState({}, "", "/?sessionId=-3");
    localStorage.setItem("meowo-chat-view-pref", "terminal");
    try {
      invoke.mockResolvedValue(null);
      render(<ChatWindow />);
      expect(await screen.findByText("PTY -3")).toBeTruthy();
      expect(invoke).toHaveBeenCalledWith("managed_terminal_binding", { sessionId: -3 });
      expect(invoke).not.toHaveBeenCalledWith("get_chat_history", expect.anything());
    } finally {
      localStorage.removeItem("meowo-chat-view-pref");
    }
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
    await screen.findByRole("button", { name: /^(添加|添加图片或文件)$/ });
    pickAttachments();
    // 图片附件是缩略图（alt=文件名），非图片仍是文本 chip。
    expect(await screen.findByAltText("design.png")).toBeTruthy();
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
    let wrote = false;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "start_managed_terminal") { started = true; return Promise.resolve(); }
      // 刚恢复的发送走写后回显验证(submitToTerminal 的 verify):正文写入后快照要
      // 能看到它的回显,否则发送被撤销——mock 据此在 write 之后把回显补进画面。
      if (command === "write_managed_terminal") {
        if (String((args as { data?: string } | undefined)?.data ?? "").includes("继续")) wrote = true;
        return Promise.resolve();
      }
      if (command === "managed_terminal_snapshot") {
        // endOffset 是「已产生多少输出」的判据（data 现在是 base64 增量，可能为空）；
        // 就绪判定还要求 data 里有可见文本（纯控制序列不算）。
        return Promise.resolve(started
          ? { sessionId: 13, active: true, managed: true, data: b64utf8(wrote ? "ready \u276f 继续" : "ready"), startOffset: 0, endOffset: wrote ? 20 : 5, exited: false, exitCode: null }
          : { sessionId: 13, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "继续" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("start_managed_terminal", {
      sessionId: 13, cols: 100, rows: 30,
    }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 13, data: "继续" }), { timeout: 2_000 });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 13, data: "\r" }), { timeout: 2_000 });
  });

  it("恢复后发送:正文没有回显时撤销发送、绝不发回车", async () => {
    // 实拍回归:接管长会话后 claude 弹 resume 确认选择器,而「可见+安静」的就绪判定
    // 认不出它——正文被选择器吞掉,旧逻辑的无条件回车还会替用户按下默认项:消息蒸发、
    // 恢复继续、没有任何报错(transcript 里根本没有那条 user 消息)。
    window.history.replaceState({}, "", "/?sessionId=71");
    const history = {
      sessionId: 71, title: "恢复吞字", status: "ended", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    };
    respondWithHistory(history);
    let started = false;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "start_managed_terminal") { started = true; return Promise.resolve(); }
      if (command === "managed_terminal_snapshot") {
        // 拉起后有可见画面但正文**永不回显**——未被识别的恢复确认选择器吞输入的形态。
        return Promise.resolve(started
          ? { sessionId: 71, active: true, managed: true, data: btoa("resume this session?"), startOffset: 0, endOffset: 20, exited: false, exitCode: null }
          : { sessionId: 71, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "go on" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 71, data: "go on" }), { timeout: 2_000 });
    // 回显等满超时后:Ctrl-U 撤销、报错可见、输入框保留原文——唯独不能出现的是回车。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 71, data: "\x15" }), { timeout: 5_000 });
    expect(invoke).not.toHaveBeenCalledWith("write_managed_terminal", { sessionId: 71, data: "\r" });
    expect(await screen.findByText(/消息未进入终端/)).toBeTruthy();
    // 先上屏再发送后，失败的原文退回输入框（人还在、输入框空着时）。
    await waitFor(() => expect(input.value).toBe("go on"));
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
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("codex"));
      if (command === "start_managed_terminal") { started = true; return Promise.resolve(); }
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve(started
          ? {
              sessionId: 14, active: true, managed: true,
              data: btoa("\x1b[2JDo you trust the contents of this directory?\r\n> 1. Yes, continue\r\n  2. No, quit"),
              startOffset: 0, endOffset: 76, exited: false, exitCode: null,
            }
          : { sessionId: 14, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "继续修复" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    // composer 常驻后 footer 里的 sendError 横幅(role=alert)可能与卡片同屏——卡片本体
    // G-16 后是 alertdialog，按角色 + 类名取。
    const trustCard = (await screen.findAllByRole("alertdialog")).find((el) => el.className.includes("chat-approval"));
    expect(trustCard?.textContent).toContain("是否信任此文件夹？");
    // C-9：卡片渲染在 overlay 层(零高度容器,不占文档流、不下推 composer),
    // 有卡在场时容器挂 is-active 启用消息列表底部的淡出过渡。
    expect(trustCard?.closest(".chat-approval-overlay")?.className).toContain("is-active");
    // 卡片在场时 composer 锁定但**不卸载**:textarea 还是同一个节点(草稿不丢),只是禁用。
    expect((screen.getByRole("combobox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement).disabled).toBe(true);
    expect(screen.getByText("PTY 14")).toBeTruthy();
    expect(screen.getByRole("button", { name: "终端" }).getAttribute("aria-pressed")).toBe("false");
    expect(screen.getByText("PTY 14").closest(".chat-terminal-pane")?.className).toContain("is-background");
    expect(screen.getByText("PTY 14").closest(".chat-terminal-pane")?.getAttribute("aria-hidden")).toBe("true");
    expect(invoke).not.toHaveBeenCalledWith("write_managed_terminal", { sessionId: 14, data: "继续修复" });
    expect((input as HTMLTextAreaElement).value).toBe("继续修复");
    // 原始终端页已经显示 TUI，不再叠加 GUI 卡片；切回对话后仍可直接点击结构化选项。
    fireEvent.click(screen.getByRole("button", { name: "终端" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    // 终端是开关：再点一次回到对话。
    fireEvent.click(screen.getByRole("button", { name: "终端" }));
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
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ sessionId: 44, active: true, managed: true, data: btoa(prompt), startOffset: 0, endOffset: prompt.length, exited: false, exitCode: null });
      }
      return Promise.resolve(null);
    });
    render(<ChatWindow />);

    expect(await screen.findByRole("button", { name: "Yes, I trust this folder" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "终端" }).getAttribute("aria-pressed")).toBe("false");
    expect(invoke).not.toHaveBeenCalledWith("start_managed_terminal", expect.anything());
  });

  it("shows the trust card for a temp session before it is claimed", async () => {
    // 新建会话认领前只有临时负 id、没有 history;claude 2.1.304 的信任页又在 hook 认领之前,
    // provider 只能从后端 pending 启动登记取(2026-10-21 dev 实拍:对话页一直「会话正在启动…」)。
    window.history.replaceState({}, "", "/?sessionId=-7");
    const prompt = "\x1b]0;claude\x1b\\\r\n"
      + "\x1b[2G\x1b[38;2;177;185;249m❯\x1b[4GNo,\x1b[8Gexit\x1b[39m\r\n"
      + "\x1b[4GYes,\x1b[9GI\x1b[11Gtrust\x1b[17Gthis\x1b[22Gfolder\r\n\r\n"
      + "\x1b[2GEnter\x1b[8Gto\x1b[11Gconfirm\x1b[19G·\x1b[21GEsc\x1b[25Gto\x1b[28Gcancel\r\n"
      + "\x1b]7501;?\x07";
    invoke.mockImplementation((command: string) => {
      if (command === "managed_terminal_pending_launch") return Promise.resolve({ provider: "claude", cwd: "C:/new-repo" });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ sessionId: -7, active: true, managed: true, data: btoa(unescape(encodeURIComponent(prompt))), startOffset: 0, endOffset: prompt.length, exited: false, exitCode: null });
      }
      return Promise.resolve(null);
    });
    render(<ChatWindow />);

    // 信任卡要经过 pending 启动登记 → 对话 UI → 终端快照 → 屏幕识别四次异步往返；单跑约 0.8s，
    // 全量并发下偶发超过 findBy 默认的 1s（UI 优化集成分支实测一次），按本文件惯例放宽。
    expect(await screen.findByRole("button", { name: "Yes, I trust this folder" }, { timeout: 5000 })).toBeTruthy();
    expect(screen.getByText("是否信任此文件夹？")).toBeTruthy();
  });

  it("renders assistant markdown but keeps user text verbatim", async () => {
    window.history.replaceState({}, "", "/?sessionId=21");
    respondWithHistory({
      sessionId: 21, title: "MD", status: "running", provider: "claude", cwd: null,
      supported: true, offset: 0, reset: false, pendingReview: null,
      items: [
        { type: "user_text", id: "u1", timestamp: null, text: "# 不是标题" },
        { type: "assistant_text", id: "a1", timestamp: null, text: "看 **重点** 和 `code`，详见 [官网](https://example.com/docs)，来信 [邮箱](mailto:a@b.com)" },
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
    // mailto: 这类非 http(s) 链接后端 open_link 必然拒绝，渲染层直接降级为纯文本——
    // 「可点却必失败」比不可点更糟。
    expect(screen.queryByRole("link", { name: "邮箱" })).toBeNull();
    expect(screen.getByText(/来信/).textContent).toContain("邮箱");
    expect(invoke).not.toHaveBeenCalledWith("open_link", { url: "mailto:a@b.com" });
    // 含框线字符的代码块被钉到字符网格：中文锁 2ch 盒子（renderGrid 拆成单字符 span），
    // 整块标记 chat-md-diagram；普通行内代码不受牵连、不被拆分。
    const wide = screen.getByText("话");
    expect(wide.className).toBe("chat-md-cell2");
    expect(wide.closest("code")?.className).toContain("chat-md-diagram");
    expect(screen.getByText("code").className).not.toContain("chat-md-diagram");
  });

  it("persists drafts under a per-session key and migrates the legacy whole-map format", async () => {
    // 旧版整表格式：首次加载拆成按会话 key（多窗整表读-改-写会互相覆盖草稿）并恢复进输入框。
    localStorage.setItem("meowo-chat-drafts", JSON.stringify({
      "21": { prompt: "旧草稿", attachments: [], at: 1 },
    }));
    window.history.replaceState({}, "", "/?sessionId=21");
    respondWithHistory({
      sessionId: 21, title: "草稿", status: "running", provider: "claude", cwd: null,
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    await waitFor(() => expect((input as HTMLTextAreaElement).value).toBe("旧草稿"));
    // 迁移完成：旧整表已删，按会话 key 在场。
    expect(localStorage.getItem("meowo-chat-drafts")).toBeNull();
    expect(localStorage.getItem("meowo-chat-draft:21")).toContain("旧草稿");
    // 继续编辑只写自己这条 key（400ms 防抖）。
    fireEvent.change(input, { target: { value: "新草稿" } });
    await waitFor(() => expect(localStorage.getItem("meowo-chat-draft:21")).toContain("新草稿"), { timeout: 2000 });
    // 清空即删条目。
    fireEvent.change(input, { target: { value: "" } });
    await waitFor(() => expect(localStorage.getItem("meowo-chat-draft:21")).toBeNull(), { timeout: 2000 });
  });

  it("shows agent badge, running pulse, slash completions and model switcher", async () => {
    window.history.replaceState({}, "", "/?sessionId=31");
    const history = {
      sessionId: 31, title: "运行观察", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, connected: true,
      model: "Opus", contextPct: 63, contextWindow: 200000, currentActivity: "Bash: cargo test",
      items: [{ type: "user_text", id: "u1", timestamp: null, text: "跑" }],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      // 斜杠补全与模型预设不是前端硬编码表：按会话查 agent_chat_ui（内置表 ∪ 自定义命令）。
      if (command === "agent_chat_ui") {
        return Promise.resolve(chatUi("claude", [
          { name: "/deploy", description: "部署到测试环境", source: "project" },
        ]));
      }
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ sessionId: 31, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findByText("运行观察");
    // agent logo（标题栏最前，aria-label=provider）+ 运行指示（有活动时显示活动文本）。
    // agent 徽标已从标题栏移除（实拍反馈），不再断言它。
    expect(screen.getByText("Bash: cargo test")).toBeTruthy();
    // 上下文用量只显示圆环；百分比与窗口详情保留在 tooltip，不再上屏。
    const meter = screen.getByRole("img", { name: "上下文已用 63%（窗口 200K）" });
    expect(meter.getAttribute("data-tip")).toBe("上下文已用 63%（窗口 200K）");
    expect(meter.querySelector(".chat-context-pct")).toBeNull();
    expect(meter.querySelector(".chat-context-usage")).toBeNull();
    expect(screen.queryByText("126K/200K")).toBeNull();
    // 截图入口移除；用量紧邻模型左侧。
    expect(screen.queryByRole("button", { name: "截图" })).toBeNull();
    const model = meter.parentElement!.querySelector(".chat-model-select")!;
    expect(meter.compareDocumentPosition(model) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // "/" 前缀弹补全；选中后填入输入框并留出参数位，不自动发送。
    const input = screen.getByRole("combobox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "/mo" } });
    // combobox/listbox/option 的 ARIA 连线:菜单开着时 expanded/controls 指向 listbox,
    // activedescendant 指向当前高亮 option(默认第一项)。
    expect(input.getAttribute("aria-expanded")).toBe("true");
    expect(input.getAttribute("aria-controls")).toBe("chat-slash-listbox");
    expect(input.getAttribute("aria-activedescendant")).toBe("chat-slash-option-0");
    fireEvent.click(screen.getByRole("option", { name: /^\/model/ }));
    expect(input.value).toBe("/model ");
    // 菜单收起后 ARIA 连线一并撤掉。
    expect(input.getAttribute("aria-expanded")).toBe("false");
    expect(input.getAttribute("aria-controls")).toBeNull();
    // 子串匹配:词中片段也给候选(此前只前缀匹配,/epl 时菜单直接消失)。
    fireEvent.change(input, { target: { value: "/epl" } });
    expect(screen.getByRole("option", { name: /^\/deploy\s*部署到测试环境/ })).toBeTruthy();
    // 参数提示行:命令精确命中且开始填参数后,候选菜单收起、换成该命令的说明行。
    fireEvent.change(input, { target: { value: "/deploy prod" } });
    expect(screen.queryByRole("option")).toBeNull();
    expect(screen.getByRole("note").textContent).toContain("/deploy");
    expect(screen.getByRole("note").textContent).toContain("部署到测试环境");
    // 自定义命令来自安装实况（agent_chat_ui 从项目目录发现的），描述取自命令文件头。
    fireEvent.change(input, { target: { value: "/de" } });
    // accessible-name 会按 DOM 实现把相邻 code/span 拼成有空格或无空格，两种都等价。
    fireEvent.click(screen.getByRole("option", { name: /^\/deploy\s*部署到测试环境/ }));
    expect(input.value).toBe("/deploy ");
    // 模型菜单：选择预设即向 PTY 发送 /model <id>。
    fireEvent.click(screen.getByRole("button", { name: "切换模型" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /^Sonnet/ }));
    await screen.findByText(zh.chat.menuBusy);
    expect(invoke).not.toHaveBeenCalledWith("write_managed_terminal", { sessionId: 31, data: "/model sonnet" });
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
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ sessionId: 32, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
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

    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
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
      supported: true, offset: 0, reset: false, pendingReview: null, connected: true,
      model: "Opus", agentModes: [{ dimension: "permission", value: "default" }], contextPct: 10, contextWindow: 200000,
      currentActivity: "Bash: 第一步", items: [],
    };
    let current: Record<string, unknown> = { ...base };
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(current);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ sessionId: 21, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findByText("初始标题");
    expect(screen.getByText("Bash: 第一步")).toBeTruthy();
    expect(screen.getByRole("img", { name: "上下文已用 10%（窗口 200K）" })).toBeTruthy();
    // 模式按钮只显示当前值，维度名在 aria-label/tooltip 里。
    expect(screen.getByText("默认")).toBeTruthy();
    // 工具行顺序固定为附件 → 权限 → 用量 → 模型。
    const meter = screen.getByRole("img", { name: "上下文已用 10%（窗口 200K）" });
    const actions = meter.parentElement!;
    const ordered = [
      actions.querySelector(".chat-attach-button")!,
      screen.getByRole("button", { name: "切换模式: 权限模式" }),
      meter,
      actions.querySelector(".chat-model-select")!,
    ];
    for (let i = 0; i < ordered.length - 1; i += 1) {
      expect(ordered[i].compareDocumentPosition(ordered[i + 1]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    }

    // 权限模式现在是下拉（选项由屏幕回显标记派生）：点按钮开菜单、点某项才发 cycle 键。
    fireEvent.click(screen.getByRole("button", { name: "切换模式: 权限模式" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /计划/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 21, data: "\u001b[Z" }));

    // 逐字段单独改：若合并成一次改动，任一字段触发的重渲染都会把其它字段的漏判一并掩盖，
    // 测试就变成假绿（验证过：合并写法下从比较清单里删掉 currentActivity 仍然通过）。
    current = { ...base, currentActivity: "Bash: 第二步" };
    expect(await screen.findByText("Bash: 第二步")).toBeTruthy();

    current = { ...base, currentActivity: "Bash: 第二步", contextPct: 42 };
    await waitFor(() => expect(document.querySelector(".chat-context")?.getAttribute("data-tip")).toBe("上下文已用 42%（窗口 200K）"));

    current = { ...base, currentActivity: "Bash: 第二步", contextPct: 42, title: "改后标题" };
    expect(await screen.findByText("改后标题")).toBeTruthy();

    current = { ...base, currentActivity: "Bash: 第二步", contextPct: 42, title: "改后标题", agentModes: [{ dimension: "permission", value: "plan" }] };
    expect(await screen.findByText("计划模式")).toBeTruthy();

    // 兜底时间线读 lastUserText/lastAiText（transcript 空窗期渲染 hook 落库的最近往来），
    // 它们也在比较清单里——漏掉的话空窗期内容永远停在第一轮。
    current = { ...base, currentActivity: "Bash: 第二步", contextPct: 42, title: "改后标题", agentModes: [{ dimension: "permission", value: "plan" }], lastUserText: "hook 落库的提问" };
    expect(await screen.findByText("hook 落库的提问")).toBeTruthy();

    // ptyManaged/endable 也在比较清单里——会话已 connected 时中途拉起托管 PTY,那一轮往往只有
    // 这两个字段变;漏掉的话「结束会话」按钮永远不出现(真实翻车过:发消息拉起 PTY 后按钮不见)。
    // 按钮门控看 endable(托管或进程仍活的孤儿),真实后端两者同轮翻真。
    expect(screen.queryByText("结束会话")).toBeNull();
    current = { ...base, currentActivity: "Bash: 第二步", contextPct: 42, title: "改后标题", agentModes: [{ dimension: "permission", value: "plan" }], lastUserText: "hook 落库的提问", ptyManaged: true, endable: true };
    expect(await screen.findByText("结束会话")).toBeTruthy();

    // 标题栏状态徽标已整体移除(errored 的窗口内表面只剩侧栏状态点,不在本测试射程)。
    // connected 仍在比较清单里——漏掉的话进程死亡(connected 翻 false、status 仍 running)
    // 时窗口标题滞留「▶」运行记号,假运行中复活。只翻 connected,断言标题记号退场。
    current = { ...base, currentActivity: "Bash: 第二步", contextPct: 42, title: "改后标题", agentModes: [{ dimension: "permission", value: "plan" }], lastUserText: "hook 落库的提问", ptyManaged: true, endable: true, connected: false };
    await waitFor(() => expect(setTitleMock).toHaveBeenCalledWith("改后标题 · Meowo"));
    // 每步都要等一轮 650ms 轮询,8 个步骤加起来超出 5s 默认时限。
  }, 15_000);

  it("renders Kimi mode dimensions and opens the native permission selector", async () => {
    window.history.replaceState({}, "", "/?sessionId=41");
    const history = {
      sessionId: 41, title: "Kimi 模式", status: "waiting", connected: true, provider: "kimi", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, model: null,
      agentModes: [
        { dimension: "work", value: "default" },
        { dimension: "permission", value: "manual" },
      ],
      contextPct: null, contextWindow: null, currentActivity: null, hasMore: false, items: [],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(history);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ sessionId: 41, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      if (command === "agent_chat_ui") return Promise.resolve({
        // 与 ChatUi 真实形状对齐(必填字段全给):此前缺 menu_slash_commands 等新字段,
        // 全靠生产码的可选链兜底侥幸通过,是唯一漏网的 mock。取值保持中性(空表/null),
        // 不引入本用例无关的菜单行为。
        slash_commands: [], model_presets: [], model_select_command: null, queue_input: "\r", version: "2.2.0",
        model_menu_command: null, menu_slash_commands: [],
        startup_attention_markers: [], selector_anchors: [], interrupt_input: null, runtime_commands_pending: false,
        attachment_mention: false, clipboard_image_paste: null, clipboard_paste_input: null,
        mode_controls: [
          {
            dimension: "work", menu_command: null, cycle_input: "\u001b[Z", options: [
              { value: "default", inputs: [{ data: "/plan off", submit: true }] },
              { value: "plan", inputs: [{ data: "/plan on", submit: true }] },
            ], screen_markers: [],
          },
          {
            dimension: "permission", menu_command: "/permission", cycle_input: null, options: [], screen_markers: [],
          },
        ],
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    // 模式按钮只显示当前值，维度名在 aria-label/tooltip 里。
    await screen.findByRole("button", { name: "切换模式: 工作模式" });
    expect(screen.getByText("默认")).toBeTruthy();
    expect(screen.getByText("手动确认")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "切换模式: 权限模式" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 41, data: "/permission" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 41, data: "\r" }));
    expect(screen.getByText("手动确认")).toBeTruthy();
    expect(invoke.mock.calls.some(([cmd, args]) => cmd === "write_managed_terminal" && String(args.data).includes("/yolo"))).toBe(false);
  });

  it("cycle 盲切后从终端屏幕回显真实落点（claude 空闲期 transcript 不写模式记录）", async () => {
    window.history.replaceState({}, "", "/?sessionId=51");
    const base = {
      sessionId: 51, title: "屏幕回显", status: "running", provider: "claude", cwd: "C:/repo", connected: true,
      supported: true, offset: 0, reset: false, pendingReview: null, model: null,
      agentModes: [{ dimension: "permission", value: "default" }],
      contextPct: null, contextWindow: null, currentActivity: null, hasMore: false, items: [],
    };
    let current: Record<string, unknown> = { ...base };
    const screenText = "plan mode on (shift+tab to cycle)";
    // 时间线式快照 mock:out 是 PTY 全量输出,snapshot(since) 回 since 之后的增量。
    // 回显必须出现在**按键之后的新输出**里——echo 循环的首帧只取偏移基线、不扫 backlog
    // (切换前的旧指示不作数),backlog 里预置的旧指示正好验证这一点。
    let out = "accept edits on (shift+tab to cycle)\r\n";
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve(current);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "list_agents") return Promise.resolve(descriptors(["claude"]));
      if (command === "write_managed_terminal" && (args as { data?: string } | undefined)?.data === "\x1b[Z") {
        // CLI 收到 Shift+Tab 后重绘出新指示。
        out += screenText;
      }
      if (command === "managed_terminal_snapshot") {
        const since = Math.min(Number((args as { since?: number } | undefined)?.since ?? 0), out.length);
        return Promise.resolve({ sessionId: 51, active: true, managed: true, data: btoa(out.slice(since)), startOffset: since, endOffset: out.length, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findByText("默认");
    // 空闲期：后续轮询不再带模式增量（真实后端普通增量的 agent_modes 就是空）。
    current = { ...base, agentModes: [] };
    // 只有 cycle 键的维度也给下拉：选项由屏幕回显标记派生，选中后循环按到位（cycleToMode）。
    fireEvent.click(screen.getByRole("button", { name: "切换模式: 权限模式" }));
    // 每档带一行后果说明（对照 Kimi 权限菜单）：启动项声明过的用启动说明，运行中才有的 auto 按值补。
    // 说明不进名字，读屏名字仍只报档名。
    expect(await screen.findByText("改文件自动，跑命令先问你")).toBeTruthy();
    const autoItem = screen.getByRole("menuitem", { name: "自动" });
    expect(document.getElementById(autoItem.getAttribute("aria-describedby") ?? "")?.textContent).toBe("安全操作自动，有风险的才问你");
    fireEvent.click(screen.getByRole("menuitem", { name: /计划/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 51, data: "\u001b[Z" }));
    // transcript 静默，但屏幕是 CLI 自己画的权威状态——标签应据它回显为「计划」。
    expect(await screen.findByText("计划模式")).toBeTruthy();
  });

  it("休眠会话的权限胶囊改卖「下一次恢复的权限」：选择只落启动参数，不碰终端", async () => {
    // connected=false 时切「活进程的模式」是空谈——进程都不在了，循环按键还得先拉起会话，
    // 且循环够不着 bypassPermissions 这类启动期档位。此时胶囊菜单换成启动档位（launch_options），
    // 选择记入 resumePermission，随下一次发送作为 start_managed_terminal 的 options 下发。
    window.history.replaceState({}, "", "/?sessionId=61");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 61, title: "休眠会话", status: "ended", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, model: null, connected: false,
        agentModes: [{ dimension: "permission", value: "default" }],
        contextPct: null, contextWindow: null, currentActivity: null, hasMore: false, items: [],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "list_agents") return Promise.resolve(descriptors(["claude"]));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 61, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      // 拉起在断言点之后立刻失败收场，免得 waitForTerminalReady 的就绪轮询在测试结束后游荡。
      if (command === "start_managed_terminal") return Promise.reject("测试桩：不真正拉起");
      return Promise.resolve();
    });
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "切换模式: 权限模式" }));
    // 菜单是启动档位（含「沿用原设置」与循环够不着的 bypassPermissions）；等 list_agents
    // 就位后条目才换装——findByRole 的重试恰好吸收这次异步。
    await screen.findByRole("menuitem", { name: "权限：沿用原设置" });
    fireEvent.click(screen.getByRole("menuitem", { name: "跳过权限确认" }));
    // 选择只落状态:不发 cycle 键、不发模式命令。不断言「零写入」——前序用例
    // submitToTerminal 的延迟回车/撤销键/探测写在慢机上会跨用例落进共享的 invoke 记录
    // (sessionId 16 用例注释记载过同一竞态;泄漏字节形形色色,曾按 \r/\x15 枚举仍抖出
    // 第三种)。改按 sessionId 隔离:只统计发给**本用例会话 61** 的写入,来源即隔断。
    const modeWrites = invoke.mock.calls.filter(([command, args]) =>
      command === "write_managed_terminal"
      && (args as { sessionId?: number } | undefined)?.sessionId === 61);
    expect(modeWrites).toEqual([]);
    // 胶囊显示所选档（启动档位词「跳过权限确认」，非运行时模式词「跳过权限检查」）。
    expect(await screen.findByText("跳过权限确认")).toBeTruthy();
    // 下一次发送把它作为启动选项带给 start_managed_terminal。
    const input = screen.getByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "继续" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("start_managed_terminal", { sessionId: 61, cols: 100, rows: 30, options: { permission: "bypassPermissions" } }));
  });

  it("会话存过启动档位时，「沿用原设置」直接亮成那一档", async () => {
    // 回归：占位文案「权限：沿用原设置」是句黑盒——后端 sessions.launch_args 存着上次
    // 选的档，能对上插件 choices 就直接显示并预选它；「沿用」= 选中它本身，不写覆盖。
    window.history.replaceState({}, "", "/?sessionId=62");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 62, title: "存过档位", status: "ended", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, model: null, connected: false,
        agentModes: [{ dimension: "permission", value: "default" }],
        contextPct: null, contextWindow: null, currentActivity: null, hasMore: false, items: [],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "list_agents") return Promise.resolve(descriptors(["claude"]));
      if (command === "session_launch_selections") return Promise.resolve({ permission: "bypassPermissions" });
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 62, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    // 胶囊直接显示存的档（启动档位词），不再是 transcript 回读的旧模式词。
    expect(await screen.findByText("跳过权限确认")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "切换模式: 权限模式" }));
    // 菜单里没有黑盒占位项；存的那档带高亮。
    const item = await screen.findByRole("menuitem", { name: "跳过权限确认" });
    expect(screen.queryByRole("menuitem", { name: "权限：沿用原设置" })).toBeNull();
    expect(item.className).toContain("is-active");
    // 高风险档（插件 risk 声明）：按钮与该菜单项都转危险色（对照 Kimi 的 perm-pill），普通档不带。
    expect(screen.getByRole("button", { name: "切换模式: 权限模式" }).className).toContain("is-risk");
    expect(item.className).toContain("is-risk");
    expect(screen.getByRole("menuitem", { name: /默认/ }).className).not.toContain("is-risk");
  });
});
