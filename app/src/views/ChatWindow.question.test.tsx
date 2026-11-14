// 提问题面卡、@ 补全、图片附件渲染与发送、停靠栏面板、滚动钉底。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emptyComposerFrame, invoke, openDialog, eventListeners, setTitleMock, terminalProps, pickAttachments, respondWithHistory } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { chatUi } from "../test/agents";
import { terminalAttention } from "../terminalAttention";
import { zh } from "../i18n/zh";

describe("ChatWindow", () => {
  /**
   * AskUserQuestion 自动放行后的题面直达：interactive-question 事件携带结构化题面，
   * 选择列表卡与终端表单同步渲染（问题/选项/描述来自 hook 参数 JSON，不等屏幕识别
   * 反推），且**不再**出现允许/拒绝的审批按钮——提问不是权限，broker 已经放行了。
   */
  it("interactive-question 题面卡从结构化参数同步渲染,不再是审批卡", async () => {
    window.history.replaceState({}, "", "/?sessionId=31");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 31, title: "提问会话", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
        ptyManaged: true, // 托管会话才开放点选排队
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 31, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("提问会话")).toBeTruthy());

    const payload = {
      sessionId: 31, requestId: "request-question", provider: "claude", toolName: "AskUserQuestion",
      description: null,
      input: JSON.stringify({
        questions: [{
          header: "仓库名", question: "新仓库叫什么名字？", multiSelect: false,
          options: [{ label: "autopilot-v2", description: "沿用产品名" }, { label: "autopilot-core" }],
        }],
      }),
      permissionSuggestions: [],
    };
    act(() => { eventListeners.get("interactive-question")?.({ payload }); });
    // 题面立即可见：问题、选项与描述。
    expect(await screen.findByText(/新仓库叫什么名字/)).toBeTruthy();
    expect(screen.getByText("autopilot-v2")).toBeTruthy();
    expect(screen.getByText("沿用产品名")).toBeTruthy();
    expect(screen.getByRole("button", { name: "去终端页" })).toBeTruthy();
    // 不是审批卡：没有允许/拒绝，也没有原始 JSON 参数。
    expect(screen.queryByRole("button", { name: "允许一次" })).toBeNull();
    expect(screen.queryByRole("button", { name: "拒绝" })).toBeNull();
    expect(screen.queryByText(/"questions"/)).toBeNull();
    // 点选即排队：提示切换为「已选…」，等屏幕识别确认表单在屏后才自动落键，
    // 不在此刻向 PTY 写任何字节。再点一次取消排队。
    //
    // 断言「**这次点击**没产生写入」而不是「全程零写入」：本用例的 snapshot 返回
    // active:true，会挂载终端组件，它自身的初始化时序在不同平台上可能产生 IPC——
    // 全局零调用的写法因此在 macOS runner 上间歇失败（CI 实测），而那与本用例要
    // 验证的「点选只排队」毫无关系。
    const writes = () =>
      invoke.mock.calls.filter((call) => call[0] === "write_managed_terminal").length;
    const before = writes();
    fireEvent.click(screen.getByRole("button", { name: /autopilot-v2/ }));
    expect(screen.getByText("已选「autopilot-v2」，表单就绪后自动作答")).toBeTruthy();
    expect(writes()).toBe(before);
    fireEvent.click(screen.getByRole("button", { name: /autopilot-v2/ }));
    expect(screen.queryByText(/已选「/)).toBeNull();
  });

  /** 提问挂着时往上翻「加载更早」：前插的旧历史不是「答案已送达」，题面卡必须留着。
   *  此前收卡判定数 items.length，一翻就把题面从后端表里撤掉，手机只剩「没认出选项」
   *  的兜底卡（2026-11-05 实拍）。 */
  it("提问挂着时加载更早的对话，题面卡不被误收", async () => {
    window.history.replaceState({}, "", "/?sessionId=34");
    const truncated = {
      sessionId: 34, title: "长会话提问", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 500, reset: false, pendingReview: null, connected: true, ptyManaged: true,
      hasMore: true, earliest: 120,
      items: [{ type: "user_text", id: "recent", timestamp: null, text: "最近的消息" }],
    };
    const incremental = { ...truncated, items: [], hasMore: false, earliest: 0 };
    let firstRead = true;
    invoke.mockImplementation((command: string, args: { before?: number }) => {
      if (command === "get_chat_history") {
        if (args?.before === 120) {
          return Promise.resolve({
            ...truncated, hasMore: false, earliest: 0,
            items: [
              { type: "user_text", id: "old-1", timestamp: null, text: "更早的消息一" },
              { type: "user_text", id: "old-2", timestamp: null, text: "更早的消息二" },
            ],
          });
        }
        if (firstRead) { firstRead = false; return Promise.resolve(truncated); }
        return Promise.resolve(incremental);
      }
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findByText("最近的消息");
    const payload = {
      sessionId: 34, requestId: "request-scroll", provider: "claude", toolName: "AskUserQuestion",
      description: null,
      input: JSON.stringify({
        questions: [{ header: "测试", question: "往上翻时卡还在吗？", multiSelect: false, options: [{ label: "在" }, { label: "不在" }] }],
      }),
      permissionSuggestions: [],
    };
    act(() => { eventListeners.get("interactive-question")?.({ payload }); });
    expect(await screen.findByText(/往上翻时卡还在吗/)).toBeTruthy();
    // 跳过 1.5s 的静置期：之后任何「transcript 增长」都会被当成答案已送达。
    const realNow = Date.now();
    const nowSpy = vi.spyOn(Date, "now").mockImplementation(() => realNow + 5_000);
    try {
      fireEvent.click(await screen.findByRole("button", { name: "加载更早的对话" }));
      expect(await screen.findByText("更早的消息二", { selector: ".chat-text" })).toBeTruthy();
      expect(screen.getByText(/往上翻时卡还在吗/)).toBeTruthy();
      expect(invoke).not.toHaveBeenCalledWith("dismiss_interactive_question", expect.anything());
    } finally {
      nowSpy.mockRestore();
    }
  });

  /** 题面卡的「仅收起」留一条可再展开的折叠条（7C-2 尾）；但挂起在别处了结后，折叠条
   *  必须自己撤掉——否则它永远挂着，点开是一张已死的卡，去 resolve 一个不存在的请求
   *  （复核实测）。存活以轮询 pending_interaction 回的 question 为准。 */
  it("题面卡收起后留折叠条；挂起在别处了结时折叠条一并撤掉", async () => {
    window.history.replaceState({}, "", "/?sessionId=33");
    // 先让轮询确认这条挂起还在，收起后折叠条才该留着。
    let questionAlive = true;
    const question = {
      sessionId: 33, requestId: "request-gone", provider: "claude", toolName: "AskUserQuestion",
      description: null, answerable: false,
      input: JSON.stringify({
        questions: [{ header: "去向", question: "接下来做哪一步？", multiSelect: false, options: [{ label: "继续" }] }],
      }),
      permissionSuggestions: [],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 33, title: "收起会话", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
        ptyManaged: true,
      });
      if (command === "pending_interaction") {
        return Promise.resolve({ approval: null, question: questionAlive ? question : null });
      }
      // 真实链路的耦合：清后端待处理表之后，轮询就再也读不到这条挂起了。上一版 mock
      // 让它返回空 Promise、questionAlive 不跟着翻，于是「收起即被自己秒清」这条回归
      // 溜了过去（复核指出）。这里如实建模：dismiss 一调，挂起即消失。
      if (command === "dismiss_interactive_question") {
        questionAlive = false;
        return Promise.resolve();
      }
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 33, active: false, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("收起会话")).toBeTruthy());
    expect(await screen.findByText(/接下来做哪一步/, {}, { timeout: 5_000 })).toBeTruthy();

    // 「仅收起」：卡走了，折叠条留下（题面用自己的文案，与屏幕识别那条分得开）。
    const dismissCalls = () => invoke.mock.calls.filter((call) => call[0] === "dismiss_interactive_question").length;
    fireEvent.click(screen.getByRole("button", { name: "仅收起" }));
    expect(screen.queryByText(/接下来做哪一步/)).toBeNull();
    expect(await screen.findByRole("button", { name: "已收起提问，点击展开" })).toBeTruthy();
    // 收起 ≠ 了结：后端待处理表必须留着。清掉的话轮询立刻读不到这条挂起，折叠条会被
    // 判活逻辑自己秒清（复核实测 2.5s 内），恢复入口没了——只断言「折叠条出现过」抓不住
    // 那条回归（最终态同样是消失），得直接钉住「没去清后端表」。
    expect(dismissCalls()).toBe(0);
    // 挂起还在，轮询每拍都会取回它：收起的卡不能被轮询塞回来（用户实拍：卡关不掉，
    // 又盖住终端里的真表单）。等过两拍轮询再看。
    const pollsBefore = invoke.mock.calls.filter((call) => call[0] === "pending_interaction").length;
    await waitFor(
      () => expect(invoke.mock.calls.filter((call) => call[0] === "pending_interaction").length).toBeGreaterThan(pollsBefore + 1),
      { timeout: 8_000 },
    );
    expect(screen.queryByText(/接下来做哪一步/)).toBeNull();
    // 折叠条展开照常，再收起一次留给下面的了结断言。
    fireEvent.click(screen.getByRole("button", { name: "已收起提问，点击展开" }));
    expect(await screen.findByText(/接下来做哪一步/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "仅收起" }));
    expect(await screen.findByRole("button", { name: "已收起提问，点击展开" })).toBeTruthy();

    // 在终端答掉/超时：下一轮轮询回 question:null，折叠条必须自己消失。
    questionAlive = false;
    await waitFor(
      () => expect(screen.queryByRole("button", { name: "已收起提问，点击展开" })).toBeNull(),
      { timeout: 5_000 },
    );
  });

  /** broker 挂起代答（answerable）：卡片是真正的作答面——多问题跨 tab、多选勾选、
   *  自定义输入，提交把 `answers:<JSON 映射>` 发给 resolve 通道，不写一个 PTY 字节。
   *  出口只有提交与「去终端作答」，刻意没有「仅收起」（收起=让 hook 干等 300s）。 */
  it("answerable 题面卡内作答:多问题多选提交 answer 正文", async () => {
    window.history.replaceState({}, "", "/?sessionId=32");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 32, title: "代答会话", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
        ptyManaged: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 32, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("代答会话")).toBeTruthy());

    const payload = {
      sessionId: 32, requestId: "request-answerable", provider: "claude", toolName: "AskUserQuestion",
      description: null, answerable: true,
      input: JSON.stringify({
        questions: [
          { header: "晚饭", question: "晚饭吃什么？", multiSelect: false, options: [{ label: "火锅" }, { label: "烧烤" }] },
          { header: "配菜", question: "配菜选哪些？", multiSelect: true, options: [{ label: "毛肚" }, { label: "虾滑" }, { label: "青菜" }] },
        ],
      }),
      permissionSuggestions: [],
    };
    act(() => { eventListeners.get("interactive-question")?.({ payload }); });
    expect(await screen.findByText(/晚饭吃什么/)).toBeTruthy();
    // 作答卡没有「仅收起」，提交键在但没答完先禁用。
    expect(screen.queryByRole("button", { name: "仅收起" })).toBeNull();
    const submit = screen.getByRole("button", { name: "提交选择" });
    expect((submit as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("还有 2 题未作答")).toBeTruthy();

    // 第 1 题单选：点选后完成度推进；换选直接替换。
    fireEvent.click(screen.getByRole("button", { name: /火锅/ }));
    expect(screen.getByText("还有 1 题未作答")).toBeTruthy();
    // 第 2 题多选：切 tab 勾两项，再补一句自定义。
    fireEvent.click(screen.getByRole("tab", { name: /配菜/ }));
    fireEvent.click(screen.getByRole("button", { name: /毛肚/ }));
    fireEvent.click(screen.getByRole("button", { name: /虾滑/ }));
    fireEvent.change(screen.getByPlaceholderText("输入其他回答"), { target: { value: "少放辣" } });
    expect(screen.getByText("提交后 Agent 立即继续")).toBeTruthy();
    // tab 上出现已答 ✓（两题都有内容）。
    expect(screen.getAllByLabelText("已作答").length).toBe(2);

    const writes = () =>
      invoke.mock.calls.filter((call) => call[0] === "write_managed_terminal").length;
    const before = writes();
    fireEvent.click(screen.getByRole("button", { name: "提交选择" }));
    await waitFor(() => {
      const resolved = invoke.mock.calls.find((call) => call[0] === "respond_runtime_request");
      expect(resolved).toBeTruthy();
      const args = resolved![1] as { sessionId: number; requestId: string; choice: string };
      expect(args.sessionId).toBe(32);
      expect(args.requestId).toBe("request-answerable");
      expect(args.choice.startsWith("answers:")).toBe(true);
      expect(JSON.parse(args.choice.slice("answers:".length))).toEqual({
        "晚饭吃什么？": "火锅",
        "配菜选哪些？": "毛肚, 虾滑, 少放辣",
      });
    });
    // 提交即收卡，且全程不写 PTY（代答走 resolve 通道，不是按键回放）。
    await waitFor(() => expect(screen.queryByText(/晚饭吃什么/)).toBeNull());
    expect(writes()).toBe(before);
  });

  for (const outcome of ["empty", "resolved", "passed"] as const) {
    it(`RuntimeRequest 一端作答后另一端收卡：${outcome}`, async () => {
      window.history.replaceState({}, "", "/?sessionId=42");
      const request = {
        sessionId: 42, requestId: `request-cross-client-${outcome}`, provider: "claude",
        kind: "question", status: "pending", toolName: "AskUserQuestion", description: null,
        input: JSON.stringify({ questions: [{ question: "双端同步吗？", multiSelect: false, options: [{ label: "是" }] }] }),
        permissionSuggestions: [], responseCapability: { type: "broker_live" },
      };
      let live = true;
      invoke.mockImplementation((command: string) => {
        if (command === "get_chat_history") return Promise.resolve({
          sessionId: 42, title: "双端会话", status: "running", provider: "claude", cwd: "C:/repo",
          supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
          ptyManaged: true,
        });
        if (command === "pending_interaction") return Promise.resolve({
          approval: null,
          question: null,
          runtimeRequest: live ? request : outcome === "empty" ? null : {
            ...request,
            status: outcome === "resolved" ? "resolved" : "cancelled",
            responseCapability: {
              type: "not_resumable",
              reason: outcome === "resolved" ? "answered" : "passed_to_terminal",
            },
          },
        });
        if (command === "managed_terminal_binding") return Promise.resolve(null);
        return Promise.resolve();
      });
      render(<ChatWindow />);
      expect(await screen.findByText(/双端同步吗/)).toBeTruthy();
      expect(screen.getByRole("button", { name: "提交选择" })).toBeTruthy();

      live = false;
      await waitFor(() => expect(screen.queryByText(/双端同步吗/)).toBeNull());
      expect(screen.queryByRole("button", { name: "提交选择" })).toBeNull();
      expect(screen.getByText(outcome === "passed"
        ? "已转到终端页，请继续作答"
        : "该提问已在别处处理或已结束")).toBeTruthy();
    });
  }

  /** 作答卡的「去终端作答」不再只是切视图：先 resolve `pass` 把挂起交还终端
   *  （表单随权限流程出现在那里），再切终端页。 */
  it("answerable 题面卡去终端作答:先 resolve pass 再切终端", async () => {
    window.history.replaceState({}, "", "/?sessionId=33");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 33, title: "交还会话", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
        ptyManaged: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 33, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("交还会话")).toBeTruthy());
    act(() => {
      eventListeners.get("interactive-question")?.({ payload: {
        sessionId: 33, requestId: "request-to-terminal", provider: "claude", toolName: "AskUserQuestion",
        description: null, answerable: true,
        input: JSON.stringify({ questions: [{ question: "继续吗？", multiSelect: false, options: [{ label: "是" }] }] }),
        permissionSuggestions: [],
      } });
    });
    expect(await screen.findByText(/继续吗/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "去终端页" }));
    await waitFor(() => {
      const resolved = invoke.mock.calls.find((call) => call[0] === "respond_runtime_request");
      expect(resolved).toBeTruthy();
      expect((resolved![1] as { choice: string }).choice).toBe("pass");
    });
  });

  /** 挂起结算/超时后轮询把 answerable 翻 false：作答卡当场降级为展示卡
   *  （恢复「仅收起」、不再有卡内提交），不需要任何 cleared 事件。
   *  题面用多问题：降级后选项仍可点选排队（按问题 keyed，C-11 尾）——降级只收回
   *  「提交选择」这条直达通道，排队落键路径不受影响。 */
  it("answerable 翻 false 时作答卡降级为展示卡", async () => {
    window.history.replaceState({}, "", "/?sessionId=34");
    const question = {
      sessionId: 34, requestId: "request-degrade", provider: "claude", toolName: "AskUserQuestion",
      description: null, answerable: true,
      input: JSON.stringify({ questions: [
        { header: "配菜", question: "配菜选哪些？", multiSelect: true, options: [{ label: "毛肚" }, { label: "青菜" }] },
        { header: "主食", question: "主食吃什么？", multiSelect: false, options: [{ label: "米饭" }] },
      ] }),
      permissionSuggestions: [],
    };
    let polledQuestion: typeof question | null = null;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 34, title: "降级会话", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
        ptyManaged: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: polledQuestion });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 34, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("降级会话")).toBeTruthy());
    act(() => { eventListeners.get("interactive-question")?.({ payload: question }); });
    expect(await screen.findByRole("button", { name: "提交选择" })).toBeTruthy();
    // 挂起在后端结算：同 requestId 但 answerable=false 从轮询回来。
    polledQuestion = { ...question, answerable: false };
    await waitFor(() => expect(screen.queryByRole("button", { name: "提交选择" })).toBeNull());
    // 降级为展示形态：选项变成点选排队按钮（多问题按问题 keyed 排队），
    // 点「毛肚」进排队并提示「已选…」。
    fireEvent.click(await screen.findByRole("button", { name: /毛肚/ }));
    expect(screen.getByText(/已选「毛肚」/)).toBeTruthy();
  });

  /** 提问卡两视图都出（用户要求）：终端视图下 broker 挂起时终端画面上没有表单，
   *  不渲染卡的话用户对「agent 在等」毫无感知。可表达题面在终端视图不出
   * 「去终端作答」（卡本身就是作答面），点选提交照常走 broker 直达。 */
  it("终端视图下也渲染可作答题面卡：无「在终端作答」，可直接提交", async () => {
    window.history.replaceState({}, "", "/?sessionId=37");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 37, title: "终端视图提问", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
        ptyManaged: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 37, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("终端视图提问")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "终端" }));
    expect(screen.getByText("PTY 37")).toBeTruthy();
    act(() => {
      eventListeners.get("interactive-question")?.({ payload: {
        sessionId: 37, requestId: "request-term-view", provider: "claude", toolName: "AskUserQuestion",
        description: null, answerable: true,
        input: JSON.stringify({ questions: [{ question: "终端里也能看到吗？", multiSelect: false, options: [{ label: "能" }] }] }),
        permissionSuggestions: [],
      } });
    });
    // 卡在终端视图渲染（题面与终端画面同时在场），且不被拉回对话页。
    expect(await screen.findByText(/终端里也能看到吗/)).toBeTruthy();
    expect(screen.getByText("PTY 37")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "去终端页" })).toBeNull();
    expect(screen.queryByRole("button", { name: "在终端作答" })).toBeNull();
    // 点选 + 提交：broker 直达，不经过终端按键。
    fireEvent.click(screen.getByRole("button", { name: /能/ }));
    fireEvent.click(screen.getByRole("button", { name: "提交选择" }));
    await waitFor(() => {
      const resolved = invoke.mock.calls.find((call) => call[0] === "respond_runtime_request");
      expect(resolved).toBeTruthy();
      const args = resolved![1] as { requestId: string; choice: string };
      expect(args.requestId).toBe("request-term-view");
      expect(args.choice.startsWith("answers:")).toBe(true);
    });
    expect(invoke).not.toHaveBeenCalledWith("write_managed_terminal", { sessionId: 37, data: expect.anything() });
  });

  /** 终端视图下题面无法在卡内表达（同文重题）时：提交不可用，「在终端作答」是唯
   *  一出口——按下只把挂起交还终端（resolve pass），人已经在终端页，不再切视图。 */
  it("终端视图下不可表达的题面保留「在终端作答」", async () => {
    window.history.replaceState({}, "", "/?sessionId=38");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 38, title: "重题会话", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
        ptyManaged: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 38, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("重题会话")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "终端" }));
    act(() => {
      eventListeners.get("interactive-question")?.({ payload: {
        sessionId: 38, requestId: "request-dup", provider: "claude", toolName: "AskUserQuestion",
        description: null, answerable: true,
        input: JSON.stringify({ questions: [
          { question: "选哪个？", multiSelect: false, options: [{ label: "甲" }] },
          { question: "选哪个？", multiSelect: false, options: [{ label: "乙" }] },
        ] }),
        permissionSuggestions: [],
      } });
    });
    expect(await screen.findByText(/这组问题无法在卡片内作答/)).toBeTruthy();
    const release = screen.getByRole("button", { name: "在终端作答" });
    fireEvent.click(release);
    await waitFor(() => {
      const resolved = invoke.mock.calls.find((call) => call[0] === "respond_runtime_request");
      expect(resolved).toBeTruthy();
      expect((resolved![1] as { choice: string }).choice).toBe("pass");
    });
    // 人已在终端页：不切视图（终端画面仍在）。
    expect(screen.getByText("PTY 38")).toBeTruthy();
  });

  /** 同步展示卡（broker 自动放行）在终端视图先收成一条提示：卡浮在 composer 上方，正好
   *  盖住终端底部的真表单（用户实拍：识别没接管时两头都答不了）。点开后照旧可点选排队，
   *  「去终端页」按钮在终端视图无意义，不出。 */
  it("终端视图下同步展示卡先收成提示条，点开后不出「去终端页」", async () => {
    window.history.replaceState({}, "", "/?sessionId=39");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 39, title: "同步题面", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
        ptyManaged: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 39, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("同步题面")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "终端" }));
    act(() => {
      eventListeners.get("interactive-question")?.({ payload: {
        sessionId: 39, requestId: "request-sync", provider: "claude", toolName: "AskUserQuestion",
        description: null, answerable: false,
        input: JSON.stringify({ questions: [{ question: "火锅还是寿司？", multiSelect: false, options: [{ label: "火锅" }, { label: "寿司" }] }] }),
        permissionSuggestions: [],
      } });
    });
    // 终端页：卡不盖住终端，只留提示条。
    const fold = await screen.findByRole("button", { name: "终端里有待答的提问，点此展开卡片" });
    expect(screen.queryByText(/火锅还是寿司/)).toBeNull();
    fireEvent.click(fold);
    expect(await screen.findByText(/火锅还是寿司/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "去终端页" })).toBeNull();
    // 点选排队照常可用：排进队列并提示，落键等屏幕识别确认（此处不驱动识别）。
    fireEvent.click(screen.getByRole("button", { name: /火锅/ }));
    expect(screen.getByText(/已选「火锅」/)).toBeTruthy();
  });

  /** 题面卡在场、8s 还没认出终端表单：写一条诊断（带识别看到的屏幕），每题只记一次。
   *  用户实拍过卡在这一步，当时没留下任何现场。 */
  it("提问卡迟迟认不出终端表单时写一条诊断日志", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      window.history.replaceState({}, "", "/?sessionId=41");
      invoke.mockImplementation((command: string) => {
        if (command === "get_chat_history") return Promise.resolve({
          sessionId: 41, title: "诊断题面", status: "running", provider: "claude", cwd: "C:/repo",
          supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
          ptyManaged: true,
        });
        if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
        if (command === "managed_terminal_binding") return Promise.resolve(null);
        if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 41, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
        return Promise.resolve();
      });
      render(<ChatWindow />);
      await waitFor(() => expect(screen.getByText("诊断题面")).toBeTruthy());
      act(() => {
        eventListeners.get("interactive-question")?.({ payload: {
          sessionId: 41, requestId: "request-diag", provider: "claude", toolName: "AskUserQuestion",
          description: null, answerable: false,
          input: JSON.stringify({ questions: [{ question: "火锅还是寿司？", multiSelect: false, options: [{ label: "火锅" }, { label: "寿司" }] }] }),
          permissionSuggestions: [],
        } });
      });
      expect(await screen.findByText(/火锅还是寿司/)).toBeTruthy();
      const diagCalls = () => invoke.mock.calls.filter((call) => call[0] === "diag_log");
      await act(async () => { vi.advanceTimersByTime(7_000); });
      expect(diagCalls()).toHaveLength(0);
      await act(async () => { vi.advanceTimersByTime(2_000); });
      await waitFor(() => expect(diagCalls()).toHaveLength(1));
      const args = diagCalls()[0][1] as { scope: string; message: string };
      // 统一的对话窗运行日志格式（chat/chatDiag，验收 19）：种类、Provider、会话 id、原因。
      expect(args.scope).toBe("chat");
      expect(args.message).toContain("kind=card_unrecognized");
      expect(args.message).toContain("provider=claude");
      expect(args.message).toContain("session=41");
      expect(args.message).toContain("reason=question_form_not_seen_8s");
      // 不带题面与终端画面上的文字。
      expect(args.message).not.toContain("火锅");
      // 同一道题不重复记。
      await act(async () => { vi.advanceTimersByTime(20_000); });
      expect(diagCalls()).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  /** C-11 尾：多问题展示卡按问题 keyed 排队 + 聚焦题识别落键。两题都点上答案后，
   *  屏幕停在第 2 题（题面原文在屏）→ 只落第 2 题的答案，第 1 题的排队保留等轮到它。 */
  it("多问题展示卡按问题排队：识别出第 2 题在屏时只落第 2 题的答案", async () => {
    window.history.replaceState({}, "", "/?sessionId=36");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 36, title: "多题会话", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
        ptyManaged: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 36, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("多题会话")).toBeTruthy());
    act(() => {
      eventListeners.get("interactive-question")?.({ payload: {
        sessionId: 36, requestId: "request-keyed", provider: "claude", toolName: "AskUserQuestion",
        description: null, answerable: false,
        input: JSON.stringify({ questions: [
          { header: "配菜", question: "配菜选哪些？", multiSelect: true, options: [{ label: "毛肚" }, { label: "虾滑" }] },
          { header: "主食", question: "主食吃什么？", multiSelect: false, options: [{ label: "米饭" }, { label: "面条" }] },
        ] }),
        permissionSuggestions: [],
      } });
    });
    // 第 1 题（默认激活 tab）点「毛肚」；切到第 2 题 tab 点「米饭」。两题排队都进提示。
    fireEvent.click(await screen.findByRole("button", { name: /毛肚/ }));
    fireEvent.click(screen.getByRole("tab", { name: /主食/ }));
    fireEvent.click(screen.getByRole("button", { name: /米饭/ }));
    expect(screen.getByText(/已选「毛肚、米饭」/)).toBeTruthy();
    // 屏幕识别接管：表单停在第 2 题（实拍形态：tab 条只有 header，题面原文在选项上方）。
    const form = [
      "\x1b[2J← 配菜 主食 ✓ Submit →",
      "主食吃什么？",
      "❯ 1. 米饭",
      "  2. 面条",
      "  3. Type something.",
      "Enter to select · Tab/Arrow keys to navigate · Esc to cancel",
    ].join("\r\n");
    const attention = terminalAttention(form, [], true);
    expect(attention?.id).toBe("interactive:numbered-selector");
    act(() => { (terminalProps.current?.onAttention as (a: unknown) => void)(attention); });
    // 只落第 2 题的答案：❯ 已停在「米饭」，直接回车；第 1 题的「毛肚」绝不动。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 36, data: "\r" }));
    expect(invoke).not.toHaveBeenCalledWith("write_managed_terminal", { sessionId: 36, data: "\x1b[B\r" });
  });

  /** 用户实拍：hook 没写上 pendingReview（会话挂着后台 Bash），题面卡照样收下两题的答案，
   *  识别却一直没开，答案永远等不到「表单就绪」。题面在就要开识别；认出表单后题面不能清，
   *  第 1 题落完，第 2 题的排队答案还得在。 */
  it("多问题展示卡：pendingReview 缺席也开识别，两题的排队答案依次落进表单", async () => {
    window.history.replaceState({}, "", "/?sessionId=37");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 37, title: "两题会话", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
        ptyManaged: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 37, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("两题会话")).toBeTruthy());
    expect(terminalProps.current?.interactivePrompt).toBeFalsy();
    act(() => {
      eventListeners.get("interactive-question")?.({ payload: {
        sessionId: 37, requestId: "request-two", provider: "claude", toolName: "AskUserQuestion",
        description: null, answerable: false,
        input: JSON.stringify({ questions: [
          { header: "GUI 限制", question: "「一个激活码只能登录一个 GUI」按哪种算？", multiSelect: false, options: [{ label: "绑定一台电脑（推荐）" }, { label: "同一时间只能有一个 GUI 在线" }] },
          { header: "多机器", question: "大于 0 时，码可以登录多个 GUI 吗？", multiSelect: false, options: [{ label: "不行，GUI 永远只有一个（推荐）" }, { label: "可以，和以前一样" }] },
        ] }),
        permissionSuggestions: [],
      } });
    });
    fireEvent.click(await screen.findByRole("button", { name: /同一时间只能有一个 GUI 在线/ }));
    fireEvent.click(screen.getByRole("tab", { name: /多机器/ }));
    fireEvent.click(screen.getByRole("button", { name: /不行，GUI 永远只有一个/ }));
    // 题面在，识别就开（不再只看 pendingReview）。
    await waitFor(() => expect(terminalProps.current?.interactivePrompt).toBe(true));
    const form = (tabs: string, question: string, options: string[]) => [
      `\x1b[2J←  ${tabs}  ✔ Submit  →`,
      question,
      ...options.map((label, index) => `${index === 0 ? "❯" : " "} ${index + 1}. ${label}`),
      `  ${options.length + 1}. Type something.`,
      "Enter to select · Tab/Arrow keys to navigate · Esc to cancel",
    ].join("\r\n");
    const inject = (screenText: string) => {
      const attention = terminalAttention(screenText, [], true);
      expect(attention?.id).toBe("interactive:numbered-selector");
      act(() => { (terminalProps.current?.onAttention as (a: unknown) => void)(attention); });
    };
    // 第 1 题在屏：落「同一时间只能有一个 GUI 在线」（下移一格 + 回车）。
    inject(form("☐ GUI 限制  ☐ 多机器", "「一个激活码只能登录一个 GUI」按哪种算？", ["绑定一台电脑（推荐）", "同一时间只能有一个 GUI 在线"]));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 37, data: "\x1b[B\r" }));
    // 识别接管后开关仍开着，第 2 题在屏时它的排队答案还在：❯ 已停在「不行…」，直接回车。
    expect(terminalProps.current?.interactivePrompt).toBe(true);
    inject(form("☒ GUI 限制  ☐ 多机器", "大于 0 时，码可以登录多个 GUI 吗？", ["不行，GUI 永远只有一个（推荐）", "可以，和以前一样"]));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 37, data: "\r" }));
  });

  /** C-11：单问题多选的展示卡（非 answerable 路径）可点选排队——勾选累加进
   *  queuedAnswers 并提示「已选…」，再点取消；多问题的跨题落键见上个用例。 */
  it("单问题多选展示卡可点选排队：勾选累加、再点取消", async () => {
    window.history.replaceState({}, "", "/?sessionId=35");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 35, title: "排队会话", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
        ptyManaged: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 35, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("排队会话")).toBeTruthy());
    act(() => {
      eventListeners.get("interactive-question")?.({ payload: {
        sessionId: 35, requestId: "request-queue-multi", provider: "claude", toolName: "AskUserQuestion",
        description: null, answerable: false,
        input: JSON.stringify({ questions: [
          { header: "配菜", question: "配菜选哪些？", multiSelect: true, options: [{ label: "毛肚" }, { label: "虾滑" }] },
        ] }),
        permissionSuggestions: [],
      } });
    });
    const tripe = await screen.findByRole("button", { name: /毛肚/ });
    const shrimp = screen.getByRole("button", { name: /虾滑/ });
    // 多选可攒多个：两个都勾上，提示带排队中的答案。
    fireEvent.click(tripe);
    fireEvent.click(shrimp);
    expect(screen.getByText(/已选「毛肚、虾滑」/)).toBeTruthy();
    // 再点取消该项，提示回落到「点选答案…」前的单选态。
    fireEvent.click(screen.getByRole("button", { name: /虾滑/ }));
    expect(screen.getByText(/已选「毛肚」/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /毛肚/ }));
    expect(screen.getByText("点选答案，终端表单就绪后自动作答")).toBeTruthy();
  });

  /**
   * 用户消息里的图片引用（Claude Code 记成「[Image: source: 本地路径]」）渲染为缩略图，
   * 原始路径绝不上屏——它是对话里最脏的元素（asset 加载失败时降级为文件名徽章）。
   */
  /** `@` 一打出就要有清单（目录浏览模式），选文件插入 @相对路径——「打了 @ 没反应」
   *  是最直接的功能不可见（实拍反馈：曾要求 ≥2 字符才查询，@ 后一片死寂）。 */
  it("@ 文件补全:打出 @ 即列目录,选中插入路径", async () => {
    window.history.replaceState({}, "", "/?sessionId=61");
    respondWithHistory({
      sessionId: 61, title: "补全", status: "waiting", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [],
    });
    const base = invoke.getMockImplementation()!;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "list_dir_entries") return Promise.resolve([
        { name: "src", relPath: "src", isDir: true },
        { name: "README.md", relPath: "README.md", isDir: false },
      ]);
      return base(command, args);
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "@" } });
    // 目录带 / 后缀可下钻；文件点击即插入完整 @路径 + 空格，菜单收起。
    expect(await screen.findByRole("option", { name: "@src/" })).toBeTruthy();
    fireEvent.click(await screen.findByRole("option", { name: "@README.md" }));
    await waitFor(() => expect((input as HTMLTextAreaElement).value).toBe("@README.md "));
    expect(screen.queryByRole("option", { name: "@README.md" })).toBeNull();
  });

  it("图片引用渲染为缩略图,原始路径不上屏", async () => {
    window.history.replaceState({}, "", "/?sessionId=41");
    respondWithHistory({
      sessionId: 41, title: "贴图", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 1, reset: false, pendingReview: null,
      items: [
        { type: "user_text", id: "u1", timestamp: null, text: "看这张图 [Image: source: C:\\Users\\me\\.claude\\image-cache\\abc\\1.png]" },
      ],
    });
    render(<ChatWindow />);
    const img = await screen.findByAltText("1.png");
    expect((img as HTMLImageElement).src).toContain("1.png");
    expect(screen.queryByText(/image-cache/)).toBeNull();
    expect(screen.getByText(/看这张图/)).toBeTruthy();
    // 点击缩略图开灯箱看大图，Esc 关闭。
    fireEvent.click(img.closest("button")!);
    const lightbox = screen.getByRole("dialog", { name: "1.png" });
    expect(lightbox.querySelector("img")).toBeTruthy();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "1.png" })).toBeNull();
    // 再开一次，走明显的关闭按钮；缩放按钮存在且百分比随点击变化。
    fireEvent.click(img.closest("button")!);
    fireEvent.click(screen.getByRole("button", { name: "放大" }));
    expect(screen.getByText("125%")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "关闭大图" }));
    expect(screen.queryByRole("dialog", { name: "1.png" })).toBeNull();
  });

  /** Claude Code 把一次多图粘贴记成连续多条独立的纯图片消息——必须合并成一行缩略图，
   *  否则竖着摞一列大图（用户实拍反馈过）。 */
  it("连续多条纯图片消息合并成一行缩略图", async () => {
    window.history.replaceState({}, "", "/?sessionId=42");
    respondWithHistory({
      sessionId: 42, title: "多图", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 2, reset: false, pendingReview: null,
      items: [
        { type: "user_text", id: "u1", timestamp: null, text: "[Image: source: C:\\cache\\a.png]" },
        { type: "user_text", id: "u2", timestamp: null, text: "[Image: source: C:\\cache\\b.png]" },
      ],
    });
    render(<ChatWindow />);
    await screen.findByAltText("a.png");
    const rows = document.querySelectorAll(".chat-image-row");
    expect(rows.length).toBe(1);
    expect(rows[0].querySelectorAll("img").length).toBe(2);
  });

  /** CLI 会把附件行「- <路径>」里的图片路径吃掉、转成独立 image 块：transcript 文本
   *  只剩指令头 + 光杆「-」，没有任何 [Image: source:] 引用。这形状同样要剥——
   *  曾因 splitUserText 在无图片引用时早退，把指令头和「-」原样上屏（实拍回归）。 */
  it("附件路径被 CLI 转成 image 块后,指令头与光杆弹头行不上屏", async () => {
    window.history.replaceState({}, "", "/?sessionId=44");
    respondWithHistory({
      sessionId: 44, title: "贴图残行", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 1, reset: false, pendingReview: null,
      items: [
        { type: "user_text", id: "u1", timestamp: null, text: "[Image #2]这个高度能不能低一点\n请读取并结合以下本地附件完成任务（图片请使用图像读取能力）：\n-" },
      ],
    });
    render(<ChatWindow />);
    await screen.findByText(/这个高度能不能低一点/);
    expect(screen.queryByText(/本地附件/)).toBeNull();
    expect(screen.queryByText(/^-$/)).toBeNull();
  });

  /** CC 还会把光杆指代「[Image #N]」挪到提交文本最前,与指令头粘成一行
   *  (「[Image #1]请读取并结合…」)——精确比对指令头会失配,整段样板漏进气泡
   *  (远程发图实拍)。判头前须先剥光杆指代。 */
  it("光杆 [Image #N] 粘在指令头行首时,样板照样剥净", async () => {
    window.history.replaceState({}, "", "/?sessionId=45");
    respondWithHistory({
      sessionId: 45, title: "贴图粘头", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 1, reset: false, pendingReview: null,
      items: [
        { type: "user_text", id: "u1", timestamp: null, text: "[Image #1]请读取并结合以下本地附件完成任务（图片请使用图像读取能力）：\n-" },
      ],
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("贴图粘头")).toBeTruthy());
    expect(screen.queryByText(/本地附件/)).toBeNull();
    expect(screen.queryByText(/\[Image #1\]/)).toBeNull();
  });

  /** 只剥「文本顶端机器前缀」里的独行光杆:用户在正文中手打的 [Image #N] 是内容,
   *  不能一起吃掉——否则气泡与真正发出的 prompt 不一致(审查回归,非远程限定)。 */
  it("用户正文里手打的独行 [Image #N] 不被附件剥离误删", async () => {
    window.history.replaceState({}, "", "/?sessionId=46");
    respondWithHistory({
      sessionId: 46, title: "用户指代", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 1, reset: false, pendingReview: null,
      items: [
        { type: "user_text", id: "u1", timestamp: null, text: "[Image #1]重点看这张\n[Image #2]\n其余忽略\n\n请读取并结合以下本地附件完成任务（图片请使用图像读取能力）：\n-" },
      ],
    });
    render(<ChatWindow />);
    await screen.findByText(/其余忽略/);
    // 顶端机器前缀被剥,样板不上屏。
    expect(screen.queryByText(/本地附件/)).toBeNull();
    // 用户在正文里手打的 [Image #2] 保留(它是内容,不是机器残留)。
    expect(screen.getByText(/\[Image #2\]/)).toBeTruthy();
  });

  /** kimi 文本回退时不吃指令里的路径行:「指令头 + '- <图片绝对路径>'」整段落进文本
   *  (claude 会转成 [Image: source: …] 引用行)。有指令头时图片路径行抽成 chip、
   *  头与路径行都不上屏;单图/多图同口径。 */
  it("kimi 式指令头 + 图片路径行:抽成缩略图 chip,头与路径不上屏", async () => {
    window.history.replaceState({}, "", "/?sessionId=47");
    respondWithHistory({
      sessionId: 47, title: "kimi 路径行", status: "running", provider: "kimi", cwd: "C:/repo",
      supported: true, offset: 1, reset: false, pendingReview: null,
      items: [
        { type: "user_text", id: "u1", timestamp: null, text: "请读取并结合以下本地附件完成任务（图片请使用图像读取能力）：\n- C:\\Users\\me\\meowo-paste\\a.png\n- D:\\img\\b 2.jpeg" },
      ],
    });
    render(<ChatWindow />);
    await screen.findByAltText("a.png");
    expect(screen.getByAltText("b 2.jpeg")).toBeTruthy();
    expect(screen.queryByText(/本地附件/)).toBeNull();
    expect(screen.queryByText(/meowo-paste/)).toBeNull();
  });

  /** 混排非图附件行维持既有保留语义:图片路径行抽成 chip,指令头与「- <非图路径>」
   *  原文保留(头要留着解释那行附件是什么)。 */
  it("kimi 式混排附件:图片行抽 chip,非图附件行与指令头保留", async () => {
    window.history.replaceState({}, "", "/?sessionId=48");
    respondWithHistory({
      sessionId: 48, title: "kimi 混排", status: "running", provider: "kimi", cwd: "C:/repo",
      supported: true, offset: 1, reset: false, pendingReview: null,
      items: [
        { type: "user_text", id: "u1", timestamp: null, text: "请读取并结合以下本地附件完成任务（图片请使用图像读取能力）：\n- C:\\img\\a.png\n- C:\\docs\\notes.txt" },
      ],
    });
    render(<ChatWindow />);
    await screen.findByAltText("a.png");
    expect(screen.getByText(/本地附件/)).toBeTruthy();
    expect(screen.getByText(/notes\.txt/)).toBeTruthy();
    expect(screen.queryByText(/C:\\img/)).toBeNull();
  });

  /** 无指令头时不动:用户手打的「- <路径>」列表是正文内容,不能当附件吃掉。 */
  it("无指令头的「- 图片路径」行是用户正文,不抽 chip", async () => {
    window.history.replaceState({}, "", "/?sessionId=49");
    respondWithHistory({
      sessionId: 49, title: "手打列表", status: "running", provider: "kimi", cwd: "C:/repo",
      supported: true, offset: 1, reset: false, pendingReview: null,
      items: [
        { type: "user_text", id: "u1", timestamp: null, text: "这几个文件看下：\n- C:\\img\\a.png" },
      ],
    });
    render(<ChatWindow />);
    await screen.findByText(/C:\\img\\a\.png/);
    expect(screen.queryByAltText("a.png")).toBeNull();
  });

  /** 多问题题面用 tab 切换：全部竖排会把卡片堆得比对话区还高、把输入框挤出可视区。 */
  it("多问题题面渲染成 tab,一次只显示一题", async () => {
    window.history.replaceState({}, "", "/?sessionId=43");
    respondWithHistory({
      sessionId: 43, title: "多题", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [], ptyManaged: true,
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("多题")).toBeTruthy());
    const payload = {
      sessionId: 43, requestId: "request-multi", provider: "claude", toolName: "AskUserQuestion",
      description: null,
      input: JSON.stringify({
        questions: [
          { header: "日志方案", question: "日志怎么处理？", multiSelect: false, options: [{ label: "自建本地日志" }] },
          { header: "auth 迁移", question: "现在做吗？", multiSelect: false, options: [{ label: "现在做" }] },
        ],
      }),
      permissionSuggestions: [],
    };
    act(() => { eventListeners.get("interactive-question")?.({ payload }); });
    // tab 条出现，默认停在第一题：第二题的内容不可见。
    expect(await screen.findByText(/日志怎么处理/)).toBeTruthy();
    expect(screen.getByRole("tab", { name: "日志方案" })).toBeTruthy();
    expect(screen.queryByText(/现在做吗/)).toBeNull();
    // 切到第二题：内容互换。
    fireEvent.click(screen.getByRole("tab", { name: "auth 迁移" }));
    expect(screen.getByText(/现在做吗/)).toBeTruthy();
    expect(screen.queryByText(/日志怎么处理/)).toBeNull();
  });

  /**
   * 冷启动路径：`interactive-question` 事件在 WebView2 起来之前发出（emit 不排队、
   * 不重放），窗口起来时事件早已消失——题面卡必须靠轮询补回来。这里刻意**不**投递
   * 事件，只让 pending_interactive_question 返回题面。
   */
  it("事件错过时,轮询把题面卡补回来", async () => {
    window.history.replaceState({}, "", "/?sessionId=33");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 33, title: "冷启动提问", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
        ptyManaged: true,
      });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 33, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      if (command === "pending_interaction") return Promise.resolve({
        approval: null,
        question: {
          sessionId: 33, requestId: "request-cold", provider: "claude", toolName: "AskUserQuestion",
          description: null,
          input: JSON.stringify({ questions: [{ question: "冷启动也要能看到题面？", options: [{ label: "必须能" }] }] }),
          permissionSuggestions: [],
        },
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    // 没有任何 interactive-question 事件，卡仍然出现。
    expect(await screen.findByText(/冷启动也要能看到题面/)).toBeTruthy();
    expect(screen.getByText("必须能")).toBeTruthy();
  });

  it("新后端的集合没有未迁移 Provider 题面时，仍从旧 question 字段补卡", async () => {
    window.history.replaceState({}, "", "/?sessionId=100");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 100, title: "未迁移提问", status: "running", provider: "codex", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true, ptyManaged: true,
      });
      if (command === "pending_interaction") return Promise.resolve({
        approval: null, runtimeRequest: null, runtimeRequests: [],
        question: {
          sessionId: 100, requestId: "request-legacy-on-new", provider: "codex", toolName: "AskUserQuestion",
          description: null, answerable: false, permissionSuggestions: [],
          input: JSON.stringify({ questions: [{ question: "旧题面不能消失", options: [{ label: "保留" }] }] }),
        },
      });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      return Promise.resolve();
    });
    render(<ChatWindow />);
    expect(await screen.findByText(/旧题面不能消失/)).toBeTruthy();
    expect(screen.getByText("保留")).toBeTruthy();
  });

  /**
   * 外部终端会话（GUI 不持有 PTY）：题面纯展示——选项不是按钮（点了也写不进按键，
   * 不承诺做不到的事），提示改为「回终端作答」，「去终端作答」按钮也不给（终端页
   * 对外部会话是空的）。
   */
  it("外部会话的题面卡纯展示:选项不可点,提示回终端作答", async () => {
    window.history.replaceState({}, "", "/?sessionId=32");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 32, title: "外部提问", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
        ptyManaged: false,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 32, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("外部提问")).toBeTruthy());
    const payload = {
      sessionId: 32, requestId: "request-question-ext", provider: "claude", toolName: "AskUserQuestion",
      description: null,
      input: JSON.stringify({ questions: [{ question: "晚饭吃什么？", multiSelect: false, options: [{ label: "火锅" }, { label: "寿司" }] }] }),
      permissionSuggestions: [],
    };
    act(() => { eventListeners.get("interactive-question")?.({ payload }); });
    expect(await screen.findByText(/晚饭吃什么/)).toBeTruthy();
    expect(screen.getByText("火锅")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /火锅/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "去终端页" })).toBeNull();
    // 外部会话 Meowo 写不进按键：写明做不到的原因，不再叫人「回到运行它的终端」（第三轮验收 13）。
    expect(screen.getByText(zh.chat.questionExternalHint)).toBeTruthy();
  });

  /**
   * 附件注入按 agent 能力分流:声明了 attachment_mention(claude/gemini,实测 @绝对路径
   * 在提交时被原生附加)就用 `@路径` 提及;图片优先走剪贴板原生附加(Ctrl-V),写不进
   * 剪贴板时退回指令文本;含空白的路径同样退回(提及会在空白处截断)。
   */
  it("claude 附件走原生 @提及,剪贴板写失败的图片退回指令文本兜底", async () => {
    window.history.replaceState({}, "", "/?sessionId=21");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 21, title: "附件注入", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
      });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      // 剪贴板被占用/附件文件已删:写不进去 → 退回指令文本(绝不能 Ctrl-V 贴别人的内容)。
      if (command === "clipboard_set_image") return Promise.reject(new Error("clipboard occupied"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 21, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("附件注入")).toBeTruthy());
    const box = () => screen.getByRole("combobox", { name: "发送消息给 Agent" });

    // 纯文本文件 + 无空白路径 → 原生 @提及,不再有指令文本。
    openDialog.mockResolvedValueOnce(["C:\\repo\\notes.txt"]);
    pickAttachments();
    await waitFor(() => expect(screen.getByText("notes.txt")).toBeTruthy());
    fireEvent.change(box(), { target: { value: "看看这个" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 21, data: "@C:\\repo\\notes.txt 看看这个" }));
    // 等提交回车落地(SUBMIT_GAP_MS 之后),sending 才复位,下一次发送才不会被守卫吞掉。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 21, data: "\r" }));

    // 只有附件、没有正文 → 退回指令文本:opencode 的 @ 补全框会吃掉「@路径」收尾的回车。
    const submits = () => invoke.mock.calls.filter(([command, args]) => command === "write_managed_terminal"
      && (args as { data: string }).data === "\r").length;
    openDialog.mockResolvedValueOnce(["C:\\repo\\notes.txt"]);
    pickAttachments();
    await waitFor(() => expect(screen.getByText("notes.txt")).toBeTruthy());
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", {
      sessionId: 21,
      data: "\u001b[200~请读取并结合以下本地附件完成任务（图片请使用图像读取能力）：\n- C:\\repo\\notes.txt\u001b[201~",
    }));
    await waitFor(() => expect(submits()).toBe(2));

    // 图片 → 剪贴板写失败,退回指令文本(原生附加见下两个用例)。
    openDialog.mockResolvedValueOnce(["C:\\repo\\shot.png"]);
    pickAttachments();
    // 图片附件是缩略图（alt=文件名）。
    await waitFor(() => expect(screen.getByAltText("shot.png")).toBeTruthy());
    fireEvent.change(box(), { target: { value: "看图" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    // 多行文本经括号粘贴包裹送入 PTY(\x1b[200~…\x1b[201~),指令文本恒多行,断言带包裹。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", {
      sessionId: 21,
      data: `\x1b[200~看图\n\n请读取并结合以下本地附件完成任务（图片请使用图像读取能力）：\n- C:\\repo\\shot.png\x1b[201~`,
    }));
  });

  /**
   * 剪贴板原生图片附加:附件全是图片 → 逐张写剪贴板 + Ctrl-V 让 TUI 自己读(claude 原生
   * [Image #N]),占位符确认后写正文提交,全程不出现指令文本;结束还原剪贴板快照。
   */
  it("纯图片附件:写剪贴板 + Ctrl-V 原生附加,占位符确认后写正文", async () => {
    window.history.replaceState({}, "", "/?sessionId=22");
    let pasted = false;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 22, title: "原生图", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
      });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "save_pasted_attachment") return Promise.resolve("C:\\Temp\\meowo-paste\\1-0\\image.png");
      if (command === "clipboard_set_image") return Promise.resolve();
      if (command === "write_managed_terminal" && args?.data === "\x16") {
        pasted = true;
        return Promise.resolve();
      }
      if (command === "managed_terminal_snapshot") {
        // ^V 之后屏幕出现 claude 的原生占位符;mock 无状态,重复返回同段增量无妨。
        const data = pasted ? btoa("> [Image #1]") : "";
        return Promise.resolve({ sessionId: 22, active: true, managed: true, data, startOffset: 0, endOffset: pasted ? 12 : 0, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("原生图")).toBeTruthy());
    const box = () => screen.getByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.paste(box(), { clipboardData: { files: [new File([new Uint8Array([137, 80])], "image.png", { type: "image/png" })] } });
    await waitFor(() => expect(screen.getByAltText("image.png")).toBeTruthy());
    fireEvent.change(box(), { target: { value: "看这张图" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    // 先把附件写进剪贴板(以落盘路径为凭),再发 Ctrl-V。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("clipboard_set_image", { path: "C:\\Temp\\meowo-paste\\1-0\\image.png" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 22, data: "\x16" }));
    // 占位符确认(首个 250ms 轮询)+ SUBMIT_GAP 后正文与回车相继写入。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 22, data: "看这张图" }), { timeout: 3000 });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 22, data: "\r" }), { timeout: 3000 });
    // 全程没有指令文本注入;结束后还原剪贴板快照。
    const writes = invoke.mock.calls.filter((call) => call[0] === "write_managed_terminal");
    expect(writes.every((call) => !String((call[1] as { data: string }).data).includes("请读取并结合"))).toBe(true);
    expect(invoke).toHaveBeenCalledWith("clipboard_restore");
  });

  /**
   * 多张图片:逐张写剪贴板、逐张等占位符计数达标再贴下一张——两家 TUI 的 composer 都
   * 支持多图连续粘贴,原生化不应只覆盖单图(用户实拍:多图退回指令文本,路径裸奔)。
   */
  it("多张图片:逐张写剪贴板连续 Ctrl-V,占位符计数逐个达标", async () => {
    window.history.replaceState({}, "", "/?sessionId=23");
    let pasted = 0;
    let saved = 0;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame("kimi"));
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 23, title: "多图", status: "waiting", provider: "kimi", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
      });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("kimi"));
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "save_pasted_attachment") {
        saved += 1;
        return Promise.resolve(`C:\\Temp\\meowo-paste\\3-0\\image${saved}.png`);
      }
      if (command === "clipboard_set_image") return Promise.resolve();
      if (command === "write_managed_terminal" && args?.data === "\x1bv") {
        pasted += 1;
        return Promise.resolve();
      }
      if (command === "managed_terminal_snapshot") {
        // kimi 的原生占位符带尺寸;第 N 次 Alt+V 后屏幕上累计 N 个。
        const line = pasted >= 2 ? "> [image #1 (10×10)] [image #2 (20×20)]" : pasted === 1 ? "> [image #1 (10×10)]" : "";
        return Promise.resolve({ sessionId: 23, active: true, managed: true, data: line ? btoa(line) : "", startOffset: 0, endOffset: line.length, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("多图")).toBeTruthy());
    const box = () => screen.getByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.paste(box(), { clipboardData: { files: [
      new File([new Uint8Array([137, 80])], "image1.png", { type: "image/png" }),
      new File([new Uint8Array([137, 80])], "image2.png", { type: "image/png" }),
    ] } });
    await waitFor(() => expect(screen.getByAltText("image2.png")).toBeTruthy());
    fireEvent.change(box(), { target: { value: "做成类似这样的" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 23, data: "做成类似这样的" }), { timeout: 3000 });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 23, data: "\r" }), { timeout: 3000 });
    // 两张各写一次剪贴板、顺序与附件一致;各发一次粘贴键——kimi 在 Windows 上是 Alt+V
    // (\x1bv),不是 Ctrl-V(发 \x16 会被 composer 无视,用户实拍故障)。
    const sets = invoke.mock.calls.filter((call) => call[0] === "clipboard_set_image");
    expect(sets.map((call) => (call[1] as { path: string }).path)).toEqual([
      "C:\\Temp\\meowo-paste\\3-0\\image1.png",
      "C:\\Temp\\meowo-paste\\3-0\\image2.png",
    ]);
    expect(invoke.mock.calls.filter((call) => call[0] === "write_managed_terminal" && (call[1] as { data: string }).data === "\x1bv")).toHaveLength(2);
    // 全程没有指令文本注入;结束后还原剪贴板快照。
    const writes = invoke.mock.calls.filter((call) => call[0] === "write_managed_terminal");
    expect(writes.every((call) => !String((call[1] as { data: string }).data).includes("请读取并结合"))).toBe(true);
    expect(invoke).toHaveBeenCalledWith("clipboard_restore");
  });

  /**
   * 假运行中校正：DB 的 running 在进程死后、reaper 收尾前是滞留值。connected:false 且
   * 托管 PTY 也不活时,运行指示一律不显示——标题栏徽标退到「未连接」,运行条不渲染。
   */
  it("进程已死的 running 会话不显示运行指示", async () => {
    window.history.replaceState({}, "", "/?sessionId=90");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 90, title: "假运行", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: false,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 90, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("假运行")).toBeTruthy());
    // 标题栏状态徽标已移除:断言它不复活,且不出现任何「运行中」指示。
    expect(document.querySelector(".chat-live")).toBeNull();
    expect(screen.queryByText("运行中")).toBeNull();
    expect(document.querySelector(".chat-running")).toBeNull();
    // 窗口标题不带运行记号。
    await waitFor(() => expect(setTitleMock).toHaveBeenCalledWith("假运行 · Meowo"));
  });

  /**
   * 标题栏状态徽标已整体移除(运行态有窗口标题「▶」记号/底部运行条/侧栏状态点三处
   * 冗余信号,错误态由侧栏状态点承载):errored 会话不再渲染徽标——防悄悄回归。
   */
  it("errored 会话标题栏不再渲染状态徽标", async () => {
    window.history.replaceState({}, "", "/?sessionId=91");
    respondWithHistory({
      sessionId: 91, title: "翻车会话", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [], errored: true,
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("翻车会话")).toBeTruthy());
    expect(document.querySelector(".chat-live")).toBeNull();
    expect(screen.queryByText("出错了")).toBeNull();
    expect(screen.queryByText("运行中")).toBeNull();
    // 没有结束入口的会话(endable 缺省/false:非托管且进程不活)不显示「结束会话」。
    expect(screen.queryByText("结束会话")).toBeNull();
  });

  it("底部进度面板带清单图标，可用 pill 或 Esc 关闭", async () => {
    window.history.replaceState({}, "", "/?sessionId=95");
    respondWithHistory({
      sessionId: 95, title: "带任务", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
      todos: [{ content: "已完成的一步", status: "completed" }],
    });
    render(<ChatWindow />);
    const pill = await screen.findByRole("button", { name: "当前进度 1/1" });
    fireEvent.click(pill);
    expect(document.querySelector(".panel-todos .dock-work-title-icon")).toBeTruthy();
    expect(document.querySelector(".panel-todos .dock-work-close")).toBeNull();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(document.querySelector(".panel-todos")).toBeNull();
    fireEvent.click(pill);
    expect(document.querySelector(".panel-todos")).toBeTruthy();
    fireEvent.click(pill);
    expect(document.querySelector(".panel-todos")).toBeNull();
  });

  it("停靠栏面板点外部关闭，点面板内不关，入口胶囊照常开合", async () => {
    window.history.replaceState({}, "", "/?sessionId=96");
    respondWithHistory({
      sessionId: 96, title: "点外关闭", status: "waiting", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
      agentModes: [{ dimension: "permission", value: "plan" }],
    });
    render(<ChatWindow />);
    const pill = await screen.findByRole("button", { name: "计划" });
    fireEvent.pointerDown(pill);
    fireEvent.click(pill);
    const hint = await screen.findByText("计划模式已开启，写出的计划会显示在这里");
    fireEvent.pointerDown(hint);
    expect(screen.getByText("计划模式已开启，写出的计划会显示在这里")).toBeTruthy();
    fireEvent.pointerDown(document.body);
    await waitFor(() => expect(screen.queryByText("计划模式已开启，写出的计划会显示在这里")).toBeNull());
  });

  it("计划与后台 Bash 入口按需出现：没有数据不出入口，计划模式开着才出计划入口", async () => {
    window.history.replaceState({}, "", "/?sessionId=95");
    respondWithHistory({
      sessionId: 95, title: "空任务", status: "waiting", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
      agentModes: [{ dimension: "permission", value: "plan" }],
    });
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "计划" }));
    expect(await screen.findByText("计划模式已开启，写出的计划会显示在这里")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "后台 Bash" })).toBeNull();
  });

  /**
   * 吸底的两条规矩（手机上滚动条抽搐的修复）：内容自己长高（折叠块展开、代码高亮）
   * 不经 items 变化，也要在下一帧前钉回底部；手指按在屏幕上时一律让路，不跟人抢位置。
   */
  it("内容长高时钉回底部,触摸期间不抢滚动位置", async () => {
    window.history.replaceState({}, "", "/?sessionId=96");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 96, title: "吸底", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 1, reset: false, pendingReview: null, connected: true,
        items: [{ type: "assistant_text", id: "a1", timestamp: null, text: "第一段回答" }],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      return Promise.resolve();
    });
    render(<ChatWindow />);
    expect(await screen.findByText("第一段回答")).toBeTruthy();
    const scroller = document.querySelector(".chat-scroll") as HTMLElement;
    let height = 1000;
    Object.defineProperty(scroller, "scrollHeight", { get: () => height, configurable: true });
    Object.defineProperty(scroller, "clientHeight", { value: 500, configurable: true });
    const frame = () => new Promise((resolve) => requestAnimationFrame(() => resolve(null)));
    const grow = async () => {
      height += 300;
      scroller.appendChild(document.createElement("div"));
      await frame();
      await frame();
    };

    scroller.scrollTop = 500;
    await grow();
    expect(scroller.scrollTop).toBe(1300);

    // 手指按下后内容再长：位置不动。
    fireEvent.touchStart(scroller);
    scroller.scrollTop = 1000;
    await grow();
    expect(scroller.scrollTop).toBe(1000);
  });

  /**
   * 进会话落在最新消息的另一半：条目是 content-visibility:auto，滚到底后底部几条才长回实高，
   * 这一步不改 DOM（用户实拍「每次打开不在最新」）。顶层条目的尺寸变化也得钉回底部。
   */
  it("条目长回实高(不改 DOM)时仍钉回底部", async () => {
    const callbacks: ResizeObserverCallback[] = [];
    const observedEls: Element[] = [];
    vi.stubGlobal("ResizeObserver", class {
      constructor(cb: ResizeObserverCallback) { callbacks.push(cb); }
      observe(target: Element) { observedEls.push(target); }
      unobserve() {}
      disconnect() {}
    });
    try {
      window.history.replaceState({}, "", "/?sessionId=97");
      invoke.mockImplementation((command: string) => {
        if (command === "get_chat_history") return Promise.resolve({
          sessionId: 97, title: "占位", status: "ended", provider: "claude", cwd: "C:/repo",
          supported: true, offset: 1, reset: false, pendingReview: null, connected: true,
          items: [{ type: "assistant_text", id: "a1", timestamp: null, text: "最后一条回答" }],
        });
        if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
        if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
        return Promise.resolve();
      });
      render(<ChatWindow />);
      const message = (await screen.findByText("最后一条回答")).closest(".chat-scroll > *") as Element;
      await waitFor(() => expect(observedEls).toContain(message));
      const scroller = document.querySelector(".chat-scroll") as HTMLElement;
      Object.defineProperty(scroller, "scrollHeight", { value: 3000, configurable: true });
      Object.defineProperty(scroller, "clientHeight", { value: 500, configurable: true });
      for (const cb of callbacks) cb([], {} as ResizeObserver);
      expect(scroller.scrollTop).toBe(3000);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  /**
   * 重新进入会话落在最新消息（对照 Kimi：对话区按会话 id 重置吸底，阅读位置不跨会话）：
   * 在 A 会话上翻到半截、切到 B，B 必须贴底，不能沿用 A 的「已脱离底部」。
   */
  it("换会话后落在最新消息,不沿用上一个会话的上翻位置", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    invoke.mockImplementation((command: string, args: { sessionId?: number }) => {
      if (command === "get_chat_history") {
        const id = args?.sessionId ?? 7;
        return Promise.resolve({
          sessionId: id, title: `会话 ${id}`, status: "ended", provider: "claude", cwd: null,
          supported: true, offset: 1, reset: false, pendingReview: null,
          items: [{ type: "assistant_text", id: `a-${id}`, timestamp: null, text: `回答 ${id}` }],
        });
      }
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "get_live_sessions_page") {
        return Promise.resolve([
          { session: { id: 7, cc_session_id: "a", status: "ended" }, project_name: "p", task_title: "会话 7", connected: true, pending_review: null, provider: "claude", cwd: "C:/a" },
          { session: { id: 42, cc_session_id: "b", status: "ended" }, project_name: "p", task_title: "另一个会话", connected: true, pending_review: null, provider: "claude", cwd: "C:/b" },
        ]);
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    expect(await screen.findByText("回答 7")).toBeTruthy();
    const scroller = document.querySelector(".chat-scroll") as HTMLElement;
    Object.defineProperty(scroller, "scrollHeight", { value: 2000, configurable: true });
    Object.defineProperty(scroller, "clientHeight", { value: 500, configurable: true });
    // 在 A 会话上翻：脱离底部，跟随关闭。
    scroller.scrollTop = 200;
    fireEvent.scroll(scroller);

    fireEvent.click(await screen.findByRole("button", { name: /另一个会话/ }));
    expect(await screen.findByText("回答 42")).toBeTruthy();
    await waitFor(() => expect(scroller.scrollTop).toBe(2000));
  });

  it("RuntimeRequest 审批与提问并存时不互相覆盖，审批走统一回答后提问仍保留", async () => {
    window.history.replaceState({}, "", "/?sessionId=98");
    const approval = {
      sessionId: 98, requestId: "request-runtime-approval", provider: "claude", kind: "approval", status: "pending",
      toolName: "Bash", description: "部署到生产环境", input: JSON.stringify({ command: "cargo deploy" }),
      permissionSuggestions: [], responseCapability: { type: "broker_live" },
    };
    const question = {
      sessionId: 98, requestId: "request-runtime-question", provider: "claude", kind: "question", status: "pending",
      toolName: "AskUserQuestion", description: null, permissionSuggestions: [], responseCapability: { type: "broker_live" },
      input: JSON.stringify({ questions: [{ question: "选择部署范围", multiSelect: false, options: [{ label: "仅当前仓库" }] }] }),
    };
    let approvalLive = true;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 98, title: "并行交互", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true, ptyManaged: true,
      });
      if (command === "pending_interaction") return Promise.resolve({
        approval: null, question: null, runtimeRequest: question,
        runtimeRequests: [approvalLive ? approval : {
          ...approval, status: "resolved", responseCapability: { type: "not_resumable", reason: "answered" },
        }, question],
      });
      if (command === "respond_runtime_request") { approvalLive = false; return Promise.resolve(); }
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      return Promise.resolve();
    });
    render(<ChatWindow />);
    expect(await screen.findByText("部署到生产环境")).toBeTruthy();
    // 既有界面一次只在停靠区展开一张卡：审批优先，提问留在统一查询里，审批结算后再显示。
    expect(screen.queryByText(/选择部署范围/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "允许一次" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("respond_runtime_request", {
      sessionId: 98, requestId: "request-runtime-approval", choice: "allow_once",
    }));
    expect(invoke).not.toHaveBeenCalledWith("resolve_pending_approval", expect.anything());
    await waitFor(() => expect(screen.queryByText("部署到生产环境")).toBeNull());
    expect(screen.getByText(/选择部署范围/)).toBeTruthy();
  });

  it("另一端已结算的审批经轮询收卡，不再提供可答按钮", async () => {
    window.history.replaceState({}, "", "/?sessionId=99");
    const approval = {
      sessionId: 99, requestId: "request-resolved-remotely", provider: "codex", kind: "approval", status: "pending",
      toolName: "Bash", description: "等待手机审批", input: "{}", permissionSuggestions: [],
      responseCapability: { type: "broker_live" },
    };
    let live = true;
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 99, title: "跨端审批", status: "waiting", provider: "codex", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true, ptyManaged: true,
      });
      if (command === "pending_interaction") return Promise.resolve({
        approval: null, question: null, runtimeRequest: null,
        runtimeRequests: [live ? approval : {
          ...approval, status: "resolved", responseCapability: { type: "not_resumable", reason: "resolved_elsewhere" },
        }],
      });
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      return Promise.resolve();
    });
    render(<ChatWindow />);
    expect(await screen.findByText("等待手机审批")).toBeTruthy();
    live = false;
    await waitFor(() => expect(screen.queryByText("等待手机审批")).toBeNull(), { timeout: 5_000 });
    expect(screen.queryByRole("button", { name: "允许一次" })).toBeNull();
    expect(invoke).not.toHaveBeenCalledWith("respond_runtime_request", expect.anything());
  });
});
