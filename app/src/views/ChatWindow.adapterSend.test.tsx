// 适配回归：排队键、送达回执、点发送即上屏、贴图占位符。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emptyComposerFrame, invoke, openDialog, eventListeners, terminalProps, pickAttachments, adapter } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";
import { terminalAttention } from "../terminalAttention";

describe("ChatWindow 适配回归", () => {
  const { history, snapshot, setup, box, writes } = adapter;

  // 2026-11-13 改定：认出输入框、只看到占位时照常发送（严格口径下 codex/gemini/opencode 永远
  // 判不出空框，对话页就永远发不出去，main 上可发）。草稿与认不出画面的拦截见 manualSendVerdict 单测。
  it("codex 原生只有占位符时照常发送", async () => {
    const base = setup(501, "codex", "running");
    invoke.mockImplementation((command, args) => command === "managed_terminal_frame" ? Promise.resolve(emptyComposerFrame("codex")) : base(command, args));
    render(<ChatWindow />);
    await screen.findByText("适配-501");
    await waitFor(() => expect(document.querySelector(".chat-model-label")).toBeTruthy());
    fireEvent.change(box(), { target: { value: "下一回合处理" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(writes(501).some((text) => String(text).includes("下一回合处理"))).toBe(true));
  });

  // 插话进了对话记录后，排队区不再挂它的回执：对话里已经有「回合中插入」的那条，
  // 回执再挂一份就是同一句话显示两遍（用户实拍）。
  it("插话出现在对话记录后，排队区不再重复显示", async () => {
    const base = setup(508, "claude", "running");
    let landed = false;
    invoke.mockImplementation((command, args) => {
      if (command === "get_chat_history" && landed) {
        return Promise.resolve({
          ...history(508, "claude", "running"),
          items: [{ type: "user_text", id: "u-in", timestamp: null, text: "顺手改个名", interjected: true }],
          offset: 1,
        });
      }
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-508");
    await waitFor(() => expect(document.querySelector(".chat-model-label")).toBeTruthy());
    fireEvent.change(box(), { target: { value: "顺手改个名" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(document.querySelector(".chat-queued-bubble")?.textContent).toBe("顺手改个名"));
    landed = true;
    // 时限短于旧版「已插入」回执的 2.5s 停留：对话里一出现，排队区就该同时没有它。
    await waitFor(() => {
      expect(document.querySelector(".chat-message.is-user")).toBeTruthy();
      expect(document.querySelector(".chat-queued-stack")).toBeNull();
    }, { timeout: 1_500 });
    expect(screen.getAllByText("顺手改个名")).toHaveLength(1);
  });

  // 回执 reducer 与 Provider 无关；用已支持原生草稿暂存的 Claude 验证入队回执标「已收到」，
  // 回执本身不出现在对话里（它不是一句话，正文之后会以用户消息再出现）。
  it("CLI 的送达回执把排队消息标成已收到，回执不进对话", async () => {
    const base = setup(509, "claude", "running");
    let acked = false;
    invoke.mockImplementation((command, args) => {
      if (command === "get_chat_history" && acked) {
        return Promise.resolve({
          ...history(509, "claude", "running"),
          items: [{ type: "prompt_queued", id: "queued:t:6", timestamp: "2026-11-01T08:00:00Z", text: "顺手改个名" }],
          offset: 1,
        });
      }
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-509");
    await waitFor(() => expect(document.querySelector(".chat-model-label")).toBeTruthy());
    fireEvent.change(box(), { target: { value: "顺手改个名" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(document.querySelector(".chat-queued-bubble")?.textContent).toBe("顺手改个名"));
    expect(screen.queryByText(zh.chat.queuedAccepted)).toBeNull();
    acked = true;
    await screen.findByText(zh.chat.queuedAccepted, {}, { timeout: 4000 });
    expect(document.querySelector(".chat-message.is-user")).toBeNull();
    expect(screen.getAllByText("顺手改个名")).toHaveLength(1);
  });

  // 状态空闲只能决定提交键，不能证明CLI输入框没有草稿；2026-11-12实拍占位符仍须保守拒发。
  it("gemini 屏幕空闲且仅有占位符时照常发送，不用 Tab", async () => {
    const base = setup(507, "gemini", "running", "idle");
    invoke.mockImplementation((command, args) => command === "managed_terminal_frame" ? Promise.resolve(emptyComposerFrame("gemini")) : base(command, args));
    render(<ChatWindow />);
    await screen.findByText("适配-507");
    await waitFor(() => expect(document.querySelector(".chat-model-label")).toBeTruthy());
    fireEvent.change(box(), { target: { value: "测试测试" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(writes(507).some((text) => String(text).includes("测试测试"))).toBe(true));
    expect(writes(507)).not.toContain("\t");
  });

  it("OpenCode 原生模型入口认不出输入框（画面是菜单）时不发送 slash 或 Esc", async () => {
    const base = setup(502, "opencode");
    const menu = { lines: ["  Select model", "  > anthropic/claude-sonnet"], cursorRow: 1, cursorCol: 0, cursorVisible: false };
    invoke.mockImplementation((command, args) => command === "managed_terminal_frame" ? Promise.resolve(menu) : command === "agent_models"
      ? Promise.resolve(["anthropic/claude-sonnet"]) : base(command, args));
    render(<ChatWindow />);
    await screen.findByText("适配-502");
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    await waitFor(() => expect(document.querySelector(".chat-send-error")).toBeTruthy());
    expect(writes(502)).toEqual([]);
    expect(writes(502).some((text) => String(text).startsWith("/model "))).toBe(false);
    expect(invoke.mock.calls.some(([command]) => command === "set_session_launch_selection")).toBe(false);
    expect(writes(502)).not.toContain("\x1b");
  });

  // 先上屏再发送（用户要求：切回列表不该取消发送）：切走再切回，在途的那条照样写完并回车，
  // 输入框里返回后新写的草稿不受影响（那条早在点发送时就从输入框挪进了气泡）。
  it("发送期间 A→B→A，旧提交照常写完，不清除新草稿", async () => {
    const base = setup(503);
    let release!: () => void;
    invoke.mockImplementation((command, args) => command === "write_managed_terminal" && args?.data === "旧内容"
      ? new Promise<void>((resolve) => { release = resolve; }) : base(command, args));
    render(<ChatWindow />);
    await screen.findByText("适配-503");
    fireEvent.change(box(), { target: { value: "旧内容" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(release).toBeTypeOf("function"));
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 504 }));
    await screen.findByText("适配-504");
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 503 }));
    await screen.findByText("适配-503");
    fireEvent.change(box(), { target: { value: "返回后新写的草稿" } });
    await act(async () => { release(); await new Promise((resolve) => setTimeout(resolve, 400)); });
    expect(box().value).toBe("返回后新写的草稿");
    expect(writes(503)).toContain("\r");
    expect(writes(503)).not.toContain("\x15");
  });

  // 先上屏再发送（用户要求，对照微信）：点发送即出气泡并标「发送中」、输入框立刻清空，
  // 不再让输入框转圈；写进终端后「发送中」消失。
  it("点发送即上屏：气泡标发送中、输入框立刻清空，写完后标注消失", async () => {
    const base = setup(511);
    let release!: () => void;
    invoke.mockImplementation((command, args) => command === "write_managed_terminal" && args?.data === "先上屏"
      ? new Promise<void>((resolve) => { release = resolve; }) : base(command, args));
    render(<ChatWindow />);
    await screen.findByText("适配-511");
    fireEvent.change(box(), { target: { value: "先上屏" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    expect((await screen.findAllByText("先上屏")).length).toBeGreaterThan(0);
    expect(screen.getByText(zh.chat.echoSending)).toBeTruthy();
    expect(box().value).toBe("");
    await waitFor(() => expect(release).toBeTypeOf("function"));
    await act(async () => { release(); });
    await waitFor(() => expect(writes(511)).toContain("\r"));
    await waitFor(() => expect(screen.queryByText(zh.chat.echoSending)).toBeNull());
  });

  // 失败时输入框里已经在写新内容：不覆盖它，留一个「发送失败」气泡，放回输入框接在新内容前面。
  it("发送失败且已在写新内容时留失败气泡，可放回输入框", async () => {
    const base = setup(512);
    let fail!: (reason: string) => void;
    invoke.mockImplementation((command, args) => command === "write_managed_terminal" && args?.data === "会失败的话"
      ? new Promise<void>((_, reject) => { fail = reject; }) : base(command, args));
    render(<ChatWindow />);
    await screen.findByText("适配-512");
    fireEvent.change(box(), { target: { value: "会失败的话" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(fail).toBeTypeOf("function"));
    fireEvent.change(box(), { target: { value: "新草稿" } });
    await act(async () => { fail("写入终端失败"); });
    expect(await screen.findByText(zh.chat.echoFailed)).toBeTruthy();
    expect(box().value).toBe("新草稿");
    expect(writes(512)).not.toContain("\r");
    fireEvent.click(screen.getByRole("button", { name: zh.chat.echoRestore }));
    expect(box().value).toBe("会失败的话\n\n新草稿");
    expect(screen.queryByText(zh.chat.echoFailed)).toBeNull();
  });

  // 终端没起来时发送、拉起终端期间切走：照样拉起、等就绪、写完回车（用户要求切走不取消）；
  // 等待进度这类界面提示不跟到别的会话上。
  it("终端未启动时发送后切走，照样拉起终端并发出", async () => {
    const base = setup(514);
    let started = false;
    let wrote = false;
    let releaseStart!: () => void;
    invoke.mockImplementation((command, args) => {
      if (Number(args?.sessionId) === 514) {
        if (command === "start_managed_terminal") {
          return new Promise<void>((resolve) => { releaseStart = () => { started = true; resolve(); }; });
        }
        if (command === "managed_terminal_snapshot") {
          return Promise.resolve(started
            ? snapshot(514, wrote ? "ready ❯ 切走也要发" : "ready ❯")
            : { ...snapshot(514), active: false, managed: false });
        }
        if (command === "write_managed_terminal" && args?.data === "切走也要发") wrote = true;
      }
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-514");
    fireEvent.change(box(), { target: { value: "切走也要发" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(releaseStart).toBeTypeOf("function"));
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 515 }));
    await screen.findByText("适配-515");
    await act(async () => { releaseStart(); });
    await waitFor(() => expect(writes(514)).toContain("\r"), { timeout: 5_000 });
    expect(writes(514)).toContain("切走也要发");
    expect(screen.queryByText(zh.chat.terminalNeedsAttention)).toBeNull();
  });

  /// 「+」菜单（对照 Kimi 的 addMenu）：命令 / 提及原来只能靠打字触发，这里给入口。
  /// 「@」插在光标处（前面补空格）；「/」只在开头生效，输入框有字时置灰。
  it("「+」菜单把提及 / 命令的触发符放进输入框", async () => {
    const base = setup(520);
    invoke.mockImplementation(async (command, args) => {
      const result = await base(command, args);
      return command === "agent_chat_ui"
        ? { ...(result as object), slash_commands: [{ name: "/help", description: "help" }], attachment_mention: true }
        : result;
    });
    render(<ChatWindow />);
    await screen.findByText("适配-520");
    // 命令清单随 agent_chat_ui 异步到达，到了「+」才变成菜单。
    const plus = await screen.findByRole("button", { name: "添加" });
    fireEvent.change(box(), { target: { value: "看看" } });
    box().setSelectionRange(2, 2);
    fireEvent.click(plus);
    expect((screen.getByRole("menuitem", { name: "命令" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("menuitem", { name: "提及" }));
    expect(box().value).toBe("看看 @");
    fireEvent.change(box(), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "命令" }));
    expect(box().value).toBe("/");
  });

  /// 展开编辑（对照 Kimi）：写长了才出展开钮，展开后编辑区拉高，清空即收回。
  it("长提示词可以展开输入框编辑，清空后自动收回", async () => {
    setup(521);
    render(<ChatWindow />);
    await screen.findByText("适配-521");
    expect(screen.queryByRole("button", { name: "展开输入框" })).toBeNull();
    fireEvent.change(box(), { target: { value: ["一", "二", "三", "四"].join(String.fromCharCode(10)) } });
    fireEvent.click(screen.getByRole("button", { name: "展开输入框" }));
    expect(box().closest(".chat-compose")?.className).toContain("is-expanded");
    expect(screen.getByRole("button", { name: "收起输入框" })).toBeTruthy();
    fireEvent.change(box(), { target: { value: "" } });
    await waitFor(() => expect(box().closest(".chat-compose")?.className).not.toContain("is-expanded"));
    expect(screen.queryByRole("button", { name: /输入框$/ })).toBeNull();
  });

  it("同一会话发送期间继续编辑，成功后保留新内容", async () => {
    const base = setup(505);
    let release!: () => void;
    invoke.mockImplementation((command, args) => command === "write_managed_terminal" && args?.data === "已提交内容"
      ? new Promise<void>((resolve) => { release = resolve; }) : base(command, args));
    render(<ChatWindow />);
    await screen.findByText("适配-505");
    fireEvent.change(box(), { target: { value: "已提交内容" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(release).toBeTypeOf("function"));
    fireEvent.change(box(), { target: { value: "后来输入的草稿" } });
    await act(async () => { release(); });
    await waitFor(() => expect(writes(505)).toContain("\r"));
    expect(box().value).toBe("后来输入的草稿");
  });

  it("图片占位符超时不提交，保留原稿并阻止自动重贴", async () => {
    setup(506);
    openDialog.mockResolvedValueOnce(["C:\\repo\\shot.png"]);
    render(<ChatWindow />);
    await screen.findByText("适配-506");
    pickAttachments();
    await screen.findByAltText("shot.png");
    fireEvent.change(box(), { target: { value: "看图" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await screen.findByText(zh.chat.imagePasteUnconfirmed, {}, { timeout: 4000 });
    expect(writes(506)).not.toContain("\r");
    expect(box().value).toBe("看图");
    expect(screen.getByAltText("shot.png")).toBeTruthy();
    fireEvent.keyDown(box(), { key: "Enter" });
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(writes(506).filter((text) => text === "\x16")).toHaveLength(1);
  });

  it("上次发图残留的占位符，重发时自动 Ctrl-U 清掉再重贴，不再拦截", async () => {
    const base = setup(510);
    let pastes = 0;
    let cleared = false;
    // 首发时整屏仿真认不出 composer(清不了残图、不能降级 → 报错);重发时才认得出。
    let recognizable = false;
    invoke.mockImplementation((command, args) => {
      if (command === "write_managed_terminal" && args?.data === "\x16") pastes += 1;
      if (command === "write_managed_terminal" && args?.data === "\x15") cleared = true;
      // 第一次贴图「没认出来」(占位符不进增量流)，第二次正常落出。
      if (command === "managed_terminal_snapshot") return Promise.resolve(snapshot(510, pastes >= 2 ? "[Image #1]" : ""));
      // 空 composer 显示的是灰色提示语而不是空行（2.1.303 实拍）。
      if (command === "managed_terminal_screen") return Promise.resolve(!recognizable ? null : ["─".repeat(40), cleared ? "❯ Try \"refactor <filepath>\"" : "❯ [Image #1]"]);
      return base(command, args);
    });
    openDialog.mockResolvedValueOnce(["C:\\repo\\shot.png"]);
    render(<ChatWindow />);
    await screen.findByText("适配-510");
    pickAttachments();
    await screen.findByAltText("shot.png");
    fireEvent.change(box(), { target: { value: "看图" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await screen.findByText(zh.chat.imagePasteUnconfirmed, {}, { timeout: 4000 });
    expect(writes(510)).not.toContain("\r");
    recognizable = true;
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(writes(510)).toContain("\r"), { timeout: 4000 });
    const sent = writes(510);
    expect(sent.indexOf("\x15")).toBeLessThan(sent.lastIndexOf("\x16"));
    expect(sent.filter((text) => text === "\x16")).toHaveLength(2);
    expect(screen.queryByText(zh.chat.imagePasteUnconfirmed)).toBeNull();
  });

  it("占位符超时但残图能清掉：自动清理并改走文本路径发出，不打扰用户", async () => {
    const base = setup(511);
    let cleared = false;
    invoke.mockImplementation((command, args) => {
      if (command === "write_managed_terminal" && args?.data === "\x15") cleared = true;
      // 占位符始终不进增量流(超时),但整屏仿真里能看到 composer 上的残图。
      if (command === "managed_terminal_snapshot") return Promise.resolve(snapshot(511, ""));
      if (command === "managed_terminal_screen") return Promise.resolve(["─".repeat(40), cleared ? "❯ Try \"refactor <filepath>\"" : "❯ [Image #1]"]);
      return base(command, args);
    });
    openDialog.mockResolvedValueOnce(["C:\\repo\\shot.png"]);
    render(<ChatWindow />);
    await screen.findByText("适配-511");
    pickAttachments();
    await screen.findByAltText("shot.png");
    fireEvent.change(box(), { target: { value: "看图" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(writes(511)).toContain("\r"), { timeout: 4000 });
    const sent = writes(511);
    // 先清残图,再写带路径的文本正文,最后回车。
    const body = sent.findIndex((text) => String(text).includes("C:\\repo\\shot.png"));
    expect(body).toBeGreaterThan(sent.indexOf("\x15"));
    expect(String(sent[body])).toContain("看图");
    expect(screen.queryByText(zh.chat.imagePasteUnconfirmed)).toBeNull();
    await waitFor(() => expect(box().value).toBe(""));
  });

  // claude 运行中贴图走增量重绘，占位符里的空格被光标定位跳过（2.1.303 实测原始流）。
  it("占位符空格被光标定位跳过时仍认作贴图成功并提交", async () => {
    const base = setup(509);
    let pasted = false;
    invoke.mockImplementation((command, args) => {
      if (command === "write_managed_terminal" && args?.data === "\x16") pasted = true;
      if (command === "managed_terminal_snapshot") return Promise.resolve(snapshot(509, pasted ? "\x1b[2C\x1b[37B[Image\x1b[10G#1]\r" : ""));
      return base(command, args);
    });
    openDialog.mockResolvedValueOnce(["C:\\repo\\shot.png"]);
    render(<ChatWindow />);
    await screen.findByText("适配-509");
    pickAttachments();
    await screen.findByAltText("shot.png");
    fireEvent.change(box(), { target: { value: "看图" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(writes(509)).toContain("\r"));
    expect(screen.queryByText(zh.chat.imagePasteUnconfirmed)).toBeNull();
  });

  it("第一张已附加、第二张剪贴板失败时，不降级重发全部路径", async () => {
    const base = setup(507);
    let pasted = false;
    let clipboardCalls = 0;
    invoke.mockImplementation((command, args) => {
      if (command === "clipboard_set_image") return ++clipboardCalls === 2
        ? Promise.reject(new Error("clipboard occupied")) : Promise.resolve();
      if (command === "write_managed_terminal" && args?.data === "\x16") pasted = true;
      if (command === "managed_terminal_snapshot") return Promise.resolve(snapshot(507, pasted ? "[Image #1]" : ""));
      return base(command, args);
    });
    openDialog.mockResolvedValueOnce(["C:\\repo\\one.png", "C:\\repo\\two.png"]);
    render(<ChatWindow />);
    await screen.findByText("适配-507");
    pickAttachments();
    await screen.findByAltText("two.png");
    fireEvent.change(box(), { target: { value: "比较两张图" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await screen.findByText(zh.chat.imagePasteUnconfirmed);
    expect(writes(507)).toEqual(["\x16"]);
    expect(box().value).toBe("比较两张图");
  });

  it("交互卡仅收起后可编辑，但不能把消息和回车写进选择器", async () => {
    setup(508);
    render(<ChatWindow />);
    await screen.findByText("适配-508");
    await waitFor(() => expect(terminalProps.current?.onAttention).toBeTypeOf("function"));
    const attention = terminalAttention("Which model?\n❯ 1. Sonnet\n  2. Opus\nEnter to select · Esc to cancel", [], false, true);
    expect(attention).toBeTruthy();
    act(() => (terminalProps.current!.onAttention as (value: unknown) => void)(attention));
    fireEvent.click(await screen.findByRole("button", { name: "仅收起" }));
    fireEvent.change(box(), { target: { value: "不应该替我选择模型" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await screen.findByText(zh.chat.terminalNeedsAttention);
    expect(writes(508)).toHaveLength(0);
    expect(box().value).toBe("不应该替我选择模型");
  });
});
