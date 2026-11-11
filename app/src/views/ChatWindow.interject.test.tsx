// 运行中插话与排队回执、中断、结束会话、运行指示。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke, openDialog, setTitleMock, pickAttachments, respondWithHistory, chatUiReady } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { submitGapMs } from "./chat/terminalSubmit";
import { zh } from "../i18n/zh";
import { chatUi } from "../test/agents";

describe("ChatWindow", () => {
  /**
   * 运行中插话:CLI 把回合中收到的消息排队到回合结束,期间 transcript 不显示——
   * GUI 必须给排队回执,否则消息像消失了。中断键有声明(claude)时提供「中断并处理」,
   * 它只发中断键(队列由 CLI 自己接着处理);回合结束回执自动消解。
   */
  it("运行中插话显示排队回执,可中断并处理,收到消费证据才消解", async () => {
    window.history.replaceState({}, "", "/?sessionId=93");
    let current: Record<string, unknown> = {
      sessionId: 93, title: "排队回执", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
    };
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(current);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 93, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await chatUiReady();
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "插一句" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 93, data: "插一句" }), { timeout: 3_000 });
    expect(await screen.findByText("1 条插话等待插入当前回合")).toBeTruthy();
    // 对话末尾的运行行先说插话的去向（用户此刻最关心的事），压过「工作中/请求中」。
    expect(screen.getByText("等这一步完成后插入你的插话").closest(".chat-running")).toBeTruthy();
    // 排队的插话以气泡排在对话末尾（下一轮将出现的位置），不再藏进输入框上方的胶囊。
    const bubble = screen.getByText("插一句", { selector: ".chat-queued-bubble" });
    expect(bubble.closest(".chat-scroll")).toBeTruthy();
    expect(document.querySelector(".dock-workbar")?.textContent ?? "").not.toContain("等待插入");
    fireEvent.click(screen.getByText("1 条插话等待插入当前回合"));

    fireEvent.click(screen.getByRole("button", { name: "中断并处理" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 93, data: "\u001b" }));

    current = { ...current, status: "waiting" };
    await new Promise((resolve) => setTimeout(resolve, 800));
    expect(screen.getAllByText(/等待插入/).length).toBeGreaterThan(0);
    current = { ...current, lastUserText: "插一句" };
    await waitFor(() => expect(screen.queryByText(/等待插入/)).toBeNull(), { timeout: 3_000 });
  });

  /**
   * 插话打断当前回合被 CLI 立即处理时,tone 不离开 running(打断后无缝进入新回合),
   * 回执只能靠「消息出现在 transcript」消解。落盘文本带 TUI 的 [Image #N] 占位前缀与
   * 附件指令,与用户原文不全等——匹配必须是归一化后的包含关系,否则回执永远挂着。
   */
  it("插话被立即处理:transcript 出现带前缀的落盘文本即消解回执", async () => {
    window.history.replaceState({}, "", "/?sessionId=95");
    let current: Record<string, unknown> = {
      sessionId: 95, title: "插话消解", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
    };
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(current);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 95, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "插一句" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(await screen.findByText("1 条插话等待插入当前回合")).toBeTruthy();

    current = {
      ...current,
      offset: 10,
      items: [{ type: "user_text", id: "u9", timestamp: null, text: "[Image #1] 插一句\n请读取并结合以下本地附件完成任务（图片请使用图像读取能力）：\n- C:\\tmp\\x.png" }],
    };
    await waitFor(() => expect(screen.queryByText(/等待插入/)).toBeNull(), { timeout: 3_000 });
    // 会话仍在运行(新回合),消解不靠回合结束——运行条(徽标已移除)仍在即为证。
    expect(document.querySelector(".chat-running")).toBeTruthy();
  });

  /**
   * 消解水位线:「包含」匹配对短插话(ok/继续)常态性子串命中**上一回合**的旧消息。
   * 回归:旧的无水位线消解在下一次轮询就收走回执,而消息还在 CLI 队列里——复现了
   * 回执本要防止的「我的消息不见了」。只有入队之后才出现的证据才可消解。
   */
  it("短插话不被上一回合旧消息子串命中,新证据到来才消解", async () => {
    window.history.replaceState({}, "", "/?sessionId=97");
    let current: Record<string, unknown> = {
      sessionId: 97, title: "水位线", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, connected: true,
      // 上一回合的旧消息(lastUserText 与 transcript 各一份)都包含「ok」这个子串。
      lastUserText: "ok, run the tests",
      items: [{ type: "user_text", id: "u1", timestamp: null, text: "ok, run the tests" }],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(current);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 97, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    // 等旧消息上屏,确保入队时快照里已含旧证据。
    await screen.findByText("ok, run the tests");
    fireEvent.change(input, { target: { value: "ok" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(await screen.findByText("1 条插话等待插入当前回合")).toBeTruthy();
    // 多轮轮询过去,旧证据不变:回执必须还在。
    await new Promise((resolve) => setTimeout(resolve, 1_500));
    expect(screen.queryByText(/等待插入/)).toBeTruthy();
    // hook 在入队瞬间就把 lastUserText 改成插话本身(claude 2.1.304 实测),消息却还在队列里、
    // transcript 没有它:transcript 在场时这不算消费证据,回执必须还在。
    current = { ...current, lastUserText: "ok" };
    await new Promise((resolve) => setTimeout(resolve, 1_500));
    expect(screen.queryByText(/等待插入/)).toBeTruthy();
    // 注入后以新条目落盘 transcript:这才消解。
    current = {
      ...current,
      offset: 20,
      items: [...(current.items as unknown[]), { type: "user_text", id: "u2", timestamp: null, text: "ok" }],
    };
    await waitFor(() => expect(screen.queryByText(/等待插入/)).toBeNull(), { timeout: 3_000 });
  });

  /**
   * 纯附件插话的回执:正文为空时回执文本是占位「（附件）」,它永不落盘——文本包含
   * 匹配对它恒失败,旧逻辑下回执永不消解(claude/kimi 同样命中)。消解改认「水位线
   * 之后出现任意新 user_text」为证据。
   */
  it("纯附件插话:占位回执在新 user_text 落盘时消解", async () => {
    window.history.replaceState({}, "", "/?sessionId=98");
    let current: Record<string, unknown> = {
      sessionId: 98, title: "纯附件回执", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
    };
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve(current);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      // 剪贴板写不进去 → 退回指令文本通道;记账不受影响,占位回执照样进排队清单。
      if (command === "clipboard_set_image") return Promise.reject(new Error("clipboard occupied"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 98, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("纯附件回执")).toBeTruthy());
    const box = () => screen.getByRole("combobox", { name: "发送消息给 Agent" });
    // 只挂附件、不写正文:回执文本只能是占位「（附件）」。
    openDialog.mockResolvedValueOnce(["C:\\repo\\shot.png"]);
    pickAttachments();
    await waitFor(() => expect(screen.getByAltText("shot.png")).toBeTruthy());
    fireEvent.keyDown(box(), { key: "Enter" });
    expect(await screen.findByText("1 条插话等待插入当前回合")).toBeTruthy();
    // transcript 落盘的是图片引用行,不含占位文本——旧的包含匹配在此永不消解。
    current = {
      ...current,
      offset: 10,
      items: [{ type: "user_text", id: "u9", timestamp: null, text: "[Image: source: C:\\cache\\shot.png]" }],
    };
    await waitFor(() => expect(screen.queryByText(/等待插入/)).toBeNull(), { timeout: 3_000 });
    // 会话仍在运行(打断后无缝进入新回合):消解不靠回合结束。
    expect(document.querySelector(".chat-running")).toBeTruthy();
  });

  /**
   * issue #72:终端输入行里未提交的草稿不能拼进对话页发出的消息。发送前看仿真整屏:
   * composer 非空才按暂存键(CLI 提交后自动还原草稿);空 composer 不按——暂存键是切换
   * 语义,空时按会放出旧暂存;认不出 composer 也不按。屏幕按真机形态模拟(见
   * tests/probe_draft_residual.rs):暂存后 composer 只剩提示符,上方常驻「› stashed」。
   */
  describe("发送前收起终端草稿", () => {
    const run = async (sessionId: number, initial: { composer: string | null; stashed: boolean }) => {
      window.history.replaceState({}, "", `/?sessionId=${sessionId}`);
      let composer = initial.composer;
      let stashed = initial.stashed;
      invoke.mockImplementation((command: string, args?: { data?: string }) => {
        if (command === "get_chat_history") return Promise.resolve({
          sessionId, title: "草稿", status: "waiting", provider: "claude", cwd: "C:/repo",
          supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
        });
        if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
        if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
        if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
        if (command === "managed_terminal_screen") {
          return Promise.resolve([
            "❯ 上一回合的用户消息",
            ...(stashed ? ["                          › stashed"] : []),
            "────────",
            ...(composer === null ? [] : [composer ? `❯ ${composer}` : "❯"]),
            "────────",
          ]);
        }
        if (command === "write_managed_terminal" && args?.data === "\u0013") {
          // 真机切换语义:非空 → 暂存;空且有暂存 → 放回 composer。
          if (composer) { stashed = true; composer = ""; } else if (stashed) { stashed = false; composer = "旧暂存"; }
        }
        return Promise.resolve();
      });
      render(<ChatWindow />);
      const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
      fireEvent.change(input, { target: { value: "hello" } });
      fireEvent.keyDown(input, { key: "Enter" });
      await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId, data: "\r" }), { timeout: 3_000 });
      return invoke.mock.calls
        .filter(([command]) => command === "write_managed_terminal")
        .map(([, args]) => (args as { data: string }).data);
    };

    it("composer 有残留草稿:先按暂存键收走,再写正文提交", async () => {
      expect(await run(301, { composer: "abc", stashed: false })).toEqual(["\u0013", "hello", "\r"]);
    });

    it("composer 有草稿且已有手动暂存:只按一次(标记按前按后都在,以 composer 变空为准)", async () => {
      expect(await run(304, { composer: "abc", stashed: true })).toEqual(["\u0013", "hello", "\r"]);
    });

    it("composer 为空(即使已有手动暂存):不按暂存键,免得放出旧暂存", async () => {
      expect(await run(302, { composer: "", stashed: true })).toEqual(["hello", "\r"]);
    });

    it("屏上认不出 composer:不盲按暂存键", async () => {
      expect(await run(303, { composer: null, stashed: false })).toEqual(["hello", "\r"]);
    });
  });

  /**
   * 打断并发送已从 composer 上的常驻按钮降成 Ctrl+Enter：它和右边的圆钮都是「把话递
   * 出去」的入口，并排摆着只会让人先停下来分辨该按哪个。功能本身不能丢——顺序仍是
   * 先写中断键、再提交正文。
   */
  it("打断并发送(Ctrl+Enter):先写中断键(Esc)再提交正文", async () => {
    window.history.replaceState({}, "", "/?sessionId=94");
    let interrupted = false;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "write_managed_terminal" && args?.data === "\u001b") interrupted = true;
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 94, title: "强制插话", status: interrupted ? "waiting" : "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 94, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "改用另一种方案" } });
    // composer 上不该再有第二个「发送」入口:一颗圆钮 + 一个快捷键,没有常驻文字按钮。
    expect(screen.queryByRole("button", { name: "打断并发送" })).toBeNull();
    // Ctrl+Enter=打断并发送的说明挂在发送圆钮的 data-tip 上(回合在跑且有草稿时)。
    const sendBtn = screen.getByRole("button", { name: "发送" });
    expect(sendBtn.getAttribute("data-tip")).toBe(zh.chat.interruptAndSendTip);
    fireEvent.keyDown(input, { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 94, data: "\u001b" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 94, data: "改用另一种方案" }), { timeout: 3_000 });
  });

  /**
   * 裸中断:发出去的消息撤不回,能叫停当前回合的只有中断键——它不该以「输入框里先写点
   * 什么」为前提。回合运行中且输入框为空时,composer 右下角那颗主圆钮**原地**变成停止键
   * (同一个 .chat-send-button,不是旁边多出来的第二个按钮),只写 Esc,不把任何正文打进
   * PTY;一开始打字就换回「发送」。
   */
  it("运行中无草稿:主圆钮变停止键,点击只发中断键", async () => {
    window.history.replaceState({}, "", "/?sessionId=96");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 96, title: "裸中断", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 96, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const button = await screen.findByRole("button", { name: "中断" });
    // 就是那颗发送圆钮本人:换了身份,没换位置,也没在旁边多长一个。
    expect(button.className).toContain("chat-send-button");
    expect(screen.queryByRole("button", { name: "发送" })).toBeNull();
    expect(screen.queryByRole("button", { name: "打断并发送" })).toBeNull();
    // 停止键不能是 disabled 的:输入框空着正是它唯一能用的时候。
    expect((button as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(button);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 96, data: "\u001b" }));
    // 只按会话 96 算账：前面用例留下的在途异步链（模型菜单静默探测的幽灵 Esc 之类，
    // 见 ChatWindow 2527 行的实拍注释）晚到时会混进全量调用，把 1 写成 2（CI 实拍）。
    const writes = invoke.mock.calls.filter(([command, args]) => command === "write_managed_terminal"
      && (args as { sessionId: number }).sessionId === 96);
    expect(writes).toHaveLength(1);

    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "换个方案" } });
    // 一开始打字,圆钮就让回「发送」;停回合改走 Ctrl+Enter,composer 上不长出新按钮。
    expect((await screen.findByRole("button", { name: "发送" })).className).toContain("chat-send-button");
    expect(screen.queryByRole("button", { name: "中断" })).toBeNull();
    expect(screen.queryByRole("button", { name: "打断并发送" })).toBeNull();
  });

  /// opencode 首按 Esc 只提示 "esc again to interrupt";两个 Esc 合成一次写入又被当成 Alt+Esc。
  /// 插件声明 interrupt_presses=2 时,中断键必须**分两次**写。
  it("中断键声明连按两次的 agent(opencode):Esc 分两次写入", async () => {
    window.history.replaceState({}, "", "/?sessionId=196");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 196, title: "双 Esc", status: "running", provider: "opencode", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("opencode"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 196, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "中断" }));
    const writes = () => invoke.mock.calls.filter(([command, args]) => command === "write_managed_terminal"
      && (args as { sessionId: number }).sessionId === 196).map(([, args]) => (args as { data: string }).data);
    await waitFor(() => expect(writes()).toEqual(["\u001b", "\u001b"]));
  });

  /**
   * 排队回执逐条可移除。**只清 GUI 记账**:消息早已写进 PTY,CLI 队列里的它照常执行——
   * 移除不得再向终端写任何按键(否则用户以为"撤回"了,实际还多打断了一次回合)。
   */
  it("排队回执可逐条移除,且不向终端写任何按键", async () => {
    window.history.replaceState({}, "", "/?sessionId=97");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 97, title: "移除回执", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 97, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "第一句" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 97, data: "第一句" }), { timeout: 3_000 });
    // 发送成功后组件才清空输入框——不等它,第二句会被那次 setPrompt("") 抹掉。
    await waitFor(() => expect((input as HTMLTextAreaElement).value).toBe(""));
    fireEvent.change(input, { target: { value: "第二句" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 97, data: "第二句" }), { timeout: 3_000 });
    expect(await screen.findByText("2 条插话等待插入当前回合")).toBeTruthy();
    fireEvent.click(screen.getByText("2 条插话等待插入当前回合"));

    const writesBefore = invoke.mock.calls.filter(([command]) => command === "write_managed_terminal").length;
    fireEvent.click(screen.getAllByRole("button", { name: "移除这条回执" })[0]);
    expect((await screen.findAllByText("1 条插话等待插入当前回合")).length).toBeGreaterThan(0);
    // 被移除的是第一条,第二条还挂着(按 id 删,不受重复文本/自动消解并发影响)。
    expect(screen.queryByText("第一句")).toBeNull();
    expect(screen.queryByText("第二句")).toBeTruthy();
    expect(invoke.mock.calls.filter(([command]) => command === "write_managed_terminal").length).toBe(writesBefore);
  });

  it("未声明中断键(interrupt_input=null)的 agent:圆钮不变停止,Ctrl+Enter 不发中断键", async () => {
    window.history.replaceState({}, "", "/?sessionId=95");
    // 当前五家都声明了 Esc;这里显式造一个 null 的 chatUi(模拟未取证/未来新 agent),
    // 验证门控:没有中断键就没有能停下回合的手段,界面不许摆出停止的样子。
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 95, title: "无中断键", status: "running", provider: "kimi", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve({ ...chatUi("kimi")!, interrupt_input: null });
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 95, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(input, { target: { value: "插话" } });
    await waitFor(() => expect((input as HTMLTextAreaElement).value).toBe("插话"));
    // Ctrl+Enter 退化成普通发送:降级要静默,不能凭空发一个这家 CLI 不认的按键。
    fireEvent.keyDown(input, { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 95, data: "插话" }), { timeout: 3_000 });
    expect(invoke.mock.calls.filter(([command, args]) =>
      command === "write_managed_terminal" && (args as { data: string }).data === "\x1b")).toHaveLength(0);
    // 圆钮同一道门:停不下来就不许摆出停止的样子,按钮不能骗人。
    fireEvent.change(input, { target: { value: "" } });
    await waitFor(() => expect((input as HTMLTextAreaElement).value).toBe(""));
    expect(screen.queryByRole("button", { name: "中断" })).toBeNull();
    // 圆钮本身还在,只是身份仍是发送(刚发完那条时它可能正处于「发送中…」)。
    expect(screen.getByRole("button", { name: /^发送(中…)?$/ })).toBeTruthy();
  });

  /**
   * 占位符只随 needsTakeover 翻（C-15）：普通发送失败（终端写入被拒等）有 sendError 条
   * 说真实原因，输入框占位必须保持原样——此前任何错误都把占位改成「尚未接管」，
   * 与真实原因无关，误导用户去找一个不存在的接管入口。
   */
  it("普通发送错误不改变输入框占位符", async () => {
    window.history.replaceState({}, "", "/?sessionId=99");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 99, title: "发送失败", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], connected: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 99, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      if (command === "write_managed_terminal") return Promise.reject(new Error("pty write broke"));
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    // 运行中的会话占位是排队说明（随 Agent 能力而定），这里只钉「出错前后不变、不变成尚未接管」。
    const before = (input as HTMLTextAreaElement).placeholder;
    expect(before).not.toBe(zh.chat.inputUnavailable);
    fireEvent.change(input, { target: { value: "发不出去" } });
    fireEvent.keyDown(input, { key: "Enter" });
    // 错误条出现（真实原因），占位符纹丝不动，也不冒出接管入口。
    await screen.findByRole("alert");
    expect((input as HTMLTextAreaElement).placeholder).toBe(before);
    expect(screen.queryByRole("button", { name: zh.chat.terminalTakeover })).toBeNull();
  });

  /**
   * GUI 用户的「结束会话」:此前结束入口只藏在终端页操作条里,不开终端页的人无从结束,
   * 会话在后台一直占着进程。标题栏入口按 endable 口径可见(托管 PTY 或进程仍活的孤儿
   * 会话),confirm 通过才调 stop_managed_terminal,取消则不动。
   */
  it("endable 会话可从标题栏结束,confirm 取消则不调用", async () => {
    window.history.replaceState({}, "", "/?sessionId=92");
    // 确认走应用内原生小窗(invoke confirm_dialog):按队列给答案。
    const confirmAnswers: boolean[] = [false, true];
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 92, title: "托管会话", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [],
        connected: true, ptyManaged: true, endable: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("claude"));
      if (command === "confirm_dialog") return Promise.resolve(confirmAnswers.shift() ?? false);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 92, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const button = await screen.findByRole("button", { name: "结束会话" });
    // 取消(队列首个 false)→ 不杀进程。
    fireEvent.click(button);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("confirm_dialog", expect.anything()));
    expect(invoke.mock.calls.some(([command]) => command === "stop_managed_terminal")).toBe(false);

    // 确定(队列次个 true)→ 杀进程。
    fireEvent.click(button);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("stop_managed_terminal", { sessionId: 92 }));
  });

  /**
   * 孤儿会话(真机事故复盘):ConPTY kill 静默无效,broker 已收掉 PTY 记录(ptyManaged=false)
   * 但进程还活着(endable=true)——结束入口必须还在,否则用户只能手动 taskkill。
   */
  it("ptyManaged=false 但进程仍活(endable)的孤儿会话仍有结束入口", async () => {
    window.history.replaceState({}, "", "/?sessionId=93");
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 93, title: "孤儿会话", status: "running", provider: "kimi", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [],
        connected: true, ptyManaged: false, endable: true,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi("kimi"));
      if (command === "confirm_dialog") return Promise.resolve(true);
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const button = await screen.findByRole("button", { name: "结束会话" });
    fireEvent.click(button);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("stop_managed_terminal", { sessionId: 93 }));
  });

  /**
   * 存活的 running 会话:运行行是对话流的最后一行(对照 Kimi,不再是输入框上方的常驻栏),
   * 本轮还没有任何输出时说「请求中」;窗口标题带 ▶ 前缀让任务栏可感知。标题栏状态徽标
   * 已移除,断言它不复活。
   */
  it("存活 running 会话:对话末尾的运行行+窗口标题记号", async () => {
    window.history.replaceState({}, "", "/?sessionId=91");
    respondWithHistory({
      sessionId: 91, title: "跑着的会话", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 1, reset: false, pendingReview: null, connected: true,
      currentActivity: "› cargo test",
      items: [{ type: "user_text", id: "u1", timestamp: null, text: "开始" }],
    });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("跑着的会话")).toBeTruthy());
    expect(document.querySelector(".chat-live")).toBeNull();
    const strip = document.querySelector(".chat-running");
    expect(strip).toBeTruthy();
    expect(strip!.closest(".chat-scroll")).toBeTruthy();
    expect(strip!.textContent).toContain("请求中…");
    expect(strip!.textContent).toContain("› cargo test");
    await waitFor(() => expect(setTitleMock).toHaveBeenCalledWith("▶ 跑着的会话 · Meowo"));
  });

  /** 压缩（/compact）进行期间后端把 current_activity 写成哨兵 __meowo_compacting__:
   *  运行条显示本地化「正在压缩上下文…」,哨兵原值不上屏、不进 tooltip(data-tip)。 */
  it("压缩哨兵:运行条映射为本地化文案,原值不进 tooltip", async () => {
    window.history.replaceState({}, "", "/?sessionId=96");
    respondWithHistory({
      sessionId: 96, title: "压缩中", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 1, reset: false, pendingReview: null, connected: true,
      currentActivity: "__meowo_compacting__",
      items: [{ type: "user_text", id: "u1", timestamp: null, text: "开始" }],
    });
    render(<ChatWindow />);
    const strip = await waitFor(() => {
      const el = document.querySelector(".chat-running");
      expect(el).toBeTruthy();
      return el!;
    });
    expect(strip.textContent).toContain("正在压缩上下文…");
    expect(strip.textContent).not.toContain("__meowo_compacting__");
    expect(strip.querySelector("[data-tip]")).toBeNull();
  });

  /** 运行指示是盲文贪吃蛇（用户指定形态）：帧字符随时间轮转，不再是原地呼吸的圆点。 */
  it("运行指示:盲文蛇帧随时间轮转", async () => {
    window.history.replaceState({}, "", "/?sessionId=95");
    respondWithHistory({
      sessionId: 95, title: "蛇", status: "running", provider: "claude", cwd: "C:/repo",
      supported: true, offset: 1, reset: false, pendingReview: null, connected: true,
      items: [{ type: "user_text", id: "u1", timestamp: null, text: "开始" }],
    });
    render(<ChatWindow />);
    await waitFor(() => expect(document.querySelector(".chat-running-snake")).toBeTruthy());
    const snake = document.querySelector(".chat-running-snake")!;
    const first = snake.textContent;
    await waitFor(() => expect(snake.textContent).not.toBe(first), { timeout: 1_000 });
  });
});

describe("submitGapMs", () => {
  it("短输入保持 250ms 基础间隔", () => {
    expect(submitGapMs("")).toBe(250);
    expect(submitGapMs("帮我跑一下测试")).toBe(250);
    expect(submitGapMs("x".repeat(1023))).toBe(250);
  });

  it("超长粘贴按长度追加消化时间(固定 250ms 时 TUI 还没消化完正文)", () => {
    expect(submitGapMs("x".repeat(1024))).toBe(300);
    expect(submitGapMs("x".repeat(10 * 1024))).toBe(750);
  });

  it("封顶 2s:再长的粘贴也不干等", () => {
    expect(submitGapMs("x".repeat(1024 * 1024))).toBe(2000);
  });
});
