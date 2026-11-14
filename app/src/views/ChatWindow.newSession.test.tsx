// 复制到新会话与分叉、导出对话、新建会话页第一句话、接续换代、同步中断。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emptyComposerFrame, invoke, saveDialog, eventListeners, b64utf8, respondWithHistory } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";
import { chatUi, descriptors } from "../test/agents";

describe("ChatWindow", () => {
  /**
   * 复制到其他引擎(用户要求:转移不如直接复制):模型市场出现「复制到新会话」分组,点档位
   * 直接调 copy_session_to_provider——原会话不结束,所以不弹确认、也不发结束命令;
   * 窗口切到返回的临时负 id(对话页给启动占位)。
   */
  it("复制到新会话:下拉分组→调复制命令并切到临时会话,原会话不结束", async () => {
    window.history.replaceState({}, "", "/?sessionId=61");
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 61, title: "换引擎", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: "Opus",
        connected: true, predecessorId: null, supersededBy: null,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      if (command === "list_agents") return Promise.resolve(descriptors(["claude", "codex"]));
      if (command === "copy_session_to_provider") {
        return Promise.resolve({ tempId: -5, handoffPath: "C:/tmp/meowo-handoff/1-61/handoff.md" });
      }
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 61, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);

    // claude 有静态预设:模型按钮先开就地菜单,「更多模型…」进模型市场;其他已装 agent 按引擎分组。
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "更多模型…" }));
    expect(await screen.findByText("复制到新会话 · Codex")).toBeTruthy();
    // 未安装的 agent 不列(descriptors 里 kimi/gemini/opencode 均未装)。
    expect(screen.queryByText(/Kimi/)).toBeNull();

    // Codex 无已学预设 → 分组内只提供「默认模型」。
    fireEvent.click(await screen.findByRole("menuitem", { name: "默认模型" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("copy_session_to_provider", {
      sessionId: 61, targetProvider: "codex", options: undefined,
    }));
    expect(await screen.findByText("会话正在启动…")).toBeTruthy();
    expect(invoke.mock.calls.some(([command]) => command === "confirm_dialog")).toBe(false);
    expect(invoke.mock.calls.some(([command]) => command === "stop_managed_terminal")).toBe(false);
  });

  /// 交接/第一句话不等认领：有的 CLI 要收到第一条消息才报 SessionStart（认领靠它），等认领就
  /// 卡死，用户自己敲一句后又补发一遍（2026-11-05 Mac 实测）。临时 id 的终端就绪就发，
  /// 发送途中认领成真 id 也不再发第二遍。
  it("复制到新会话：终端就绪就用临时 id 发交接语，认领后不重发", async () => {
    window.history.replaceState({}, "", "/?sessionId=61");
    let claimed = false;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame("kimi"));
      const sid = Number(args?.sessionId);
      if (command === "get_chat_history") {
        if (sid === 61) return Promise.resolve({
          sessionId: 61, title: "换引擎", status: "running", provider: "claude", cwd: "C:/repo",
          supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: "Opus",
          connected: true, predecessorId: null, supersededBy: null,
        });
        if (sid === 90) return Promise.resolve({
          sessionId: 90, title: "认领后", status: "running", provider: "kimi", cwd: "C:/repo",
          supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: null,
          connected: true, predecessorId: null, supersededBy: null,
        });
        return Promise.reject(new Error("no session"));
      }
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      if (command === "list_agents") return Promise.resolve(descriptors(["claude", "kimi"]));
      if (command === "copy_session_to_provider") {
        return Promise.resolve({ tempId: -5, handoffPath: "C:/tmp/meowo-handoff/1-61/handoff.md" });
      }
      if (command === "managed_terminal_pending_launch") return Promise.resolve(sid === -5 ? { provider: "kimi", cwd: "C:/repo" } : null);
      if (command === "managed_terminal_binding") return Promise.resolve(claimed ? 90 : null);
      if (command === "managed_terminal_snapshot") {
        // 首帧画出输入框，之后安静：700ms 后判就绪。
        const data = Number(args?.since ?? 0) === 0 ? btoa("kimi> ") : "";
        return Promise.resolve({ sessionId: sid, active: true, managed: true, data, startOffset: 0, endOffset: 7, exited: false, exitCode: null });
      }
      if (command === "write_managed_terminal") {
        // 这一句会触发 CLI 的 hook，认领随即完成。
        claimed = true;
        return Promise.resolve();
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "更多模型…" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "默认模型" }));

    const handoffWrites = () => invoke.mock.calls.filter(([command, args]) =>
      command === "write_managed_terminal" && String((args as { data: string }).data).includes("handoff.md"));
    await waitFor(() => expect(handoffWrites().length).toBeGreaterThan(0), { timeout: 4_000 });
    expect((handoffWrites()[0][1] as { sessionId: number }).sessionId).toBe(-5);
    await waitFor(() => expect(screen.getByText("认领后")).toBeTruthy());
    await new Promise((resolve) => setTimeout(resolve, 1_200));
    expect(handoffWrites()).toHaveLength(1);
  });

  /// 分叉会话（对照 Kimi 会话菜单）：标题菜单里给入口，调 fork_session 后切到返回的临时会话；
  /// 运行中置灰（回合到一半分出去的上下文是残缺的）。
  it.each(["waiting", "running"])("分叉会话:标题菜单入口(%s)", async (status) => {
    window.history.replaceState({}, "", "/?sessionId=63");
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 63, title: "想分叉", status, provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: "Opus",
        connected: true, predecessorId: null, supersededBy: null,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      if (command === "fork_session") return Promise.resolve(-7);
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 63, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "会话操作" }));
    const item = await screen.findByRole("menuitem", { name: "分叉会话" }) as HTMLButtonElement;
    if (status === "running") {
      await waitFor(() => expect(item.disabled).toBe(true));
      return;
    }
    expect(item.disabled).toBe(false);
    fireEvent.click(item);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("fork_session", { sessionId: 63, cols: 100, rows: 30 }));
    expect(await screen.findByText("会话正在启动…")).toBeTruthy();
  });

  /// 复制恢复命令（对照 Kimi「复制 Session ID」）：带 cd 的完整命令，到终端粘贴即可接着跑。
  /// 会话还在这里跑时回执换成「先结束」的提醒，免得两个进程同写一份会话。
  it.each([
    [false, "已复制恢复命令"],
    [true, "已复制，先在这里结束会话再运行"],
  ])("复制恢复命令:标题菜单复制 cd + resume 命令并给回执(connected=%s)", async (connected, notice) => {
    window.history.replaceState({}, "", "/?sessionId=64");
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 64, title: "接着跑", status: "waiting", provider: "claude", cwd: "C:/repo",
        ccSessionId: "b730cb79-2542", supported: true, offset: 0, reset: false, pendingReview: null,
        items: [], model: "Opus", connected, predecessorId: null, supersededBy: null,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 64, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "会话操作" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "复制恢复命令" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('cd "C:/repo"; claude --resume b730cb79-2542'));
    expect(await screen.findByText(notice)).toBeTruthy();
  });

  /// 会话菜单导出（对照 Kimi「复制全部对话为 Markdown / 仅复制最终总结」）。
  it("会话菜单复制整段对话为 Markdown，或只复制最后一条回答", async () => {
    window.history.replaceState({}, "", "/?sessionId=66");
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      // 增量语义：追平后的轮询回空（同 respondWithHistory）。原先每轮都回整批，跨过 650ms
      // 轮询间隔就把两条消息追加第二遍，导出的 Markdown 里出现重复段（并发全量跑时实测）。
      if (command === "get_chat_history" && ((args?.offset as number) ?? 0) > 0) return Promise.resolve({
        sessionId: 66, title: "修登录页", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 2, reset: false, pendingReview: null, model: "Opus",
        connected: true, predecessorId: null, supersededBy: null, items: [], hasMore: false,
      });
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 66, title: "修登录页", status: "waiting", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 2, reset: false, pendingReview: null, model: "Opus",
        connected: true, predecessorId: null, supersededBy: null,
        items: [
          { type: "user_text", id: "u1", timestamp: null, text: "表单校验坏了" },
          { type: "assistant_text", id: "a1", timestamp: null, text: "已修好。" },
        ],
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      if (command === "list_agents") return Promise.resolve(descriptors(["claude"]));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 66, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findAllByText("已修好。");
    fireEvent.click(await screen.findByRole("button", { name: "会话操作" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "复制对话为 Markdown" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(
      ["# 修登录页", "---", "**你**", "表单校验坏了", "---", "**Claude Code**", "已修好。"].join("\n\n") + "\n",
    ));
    fireEvent.click(await screen.findByRole("button", { name: "会话操作" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "仅复制最终回答" }));
    await waitFor(() => expect(writeText).toHaveBeenLastCalledWith("已修好。"));
    expect(await screen.findByText("已复制最终回答")).toBeTruthy();
    // 导出会话：系统保存框默认文件名取会话标题，同一份 Markdown 交给后端写盘。
    saveDialog.mockResolvedValueOnce("D:/out/修登录页.md");
    fireEvent.click(await screen.findByRole("button", { name: "会话操作" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "导出会话…" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("export_text_file", {
      path: "D:/out/修登录页.md",
      text: (writeText.mock.calls[0] as unknown as [string])[0],
    }));
    expect(saveDialog).toHaveBeenCalledWith(expect.objectContaining({ defaultPath: "修登录页.md" }));
  });

  it("复制到新会话:来源不支持导出(supports_chat_export=false)时不显示分组", async () => {
    window.history.replaceState({}, "", "/?sessionId=62");
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 62, title: "不可导出", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: "Opus",
        connected: true, predecessorId: null, supersededBy: null,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      // 能力位由后端下发;这里模拟一个历史不可导出的来源(其余字段与真实矩阵一致)。
      if (command === "list_agents") {
        return Promise.resolve(descriptors(["claude", "codex"]).map((d) =>
          d.id === "claude" ? { ...d, supports_chat_export: false } : d));
      }
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 62, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    // 模型档照常;「复制到新会话」分组整个不出现——没有可交接的历史,给入口就是给假承诺。
    expect((await screen.findAllByRole("menuitem", { name: /Sonnet/ })).length).toBeGreaterThan(0);
    expect(screen.queryByText(/复制到新会话 ·/)).toBeNull();
  });

  /**
   * 交接注入:复制出来的新会话(binding 认领出的真 id,没有接续链)在终端就绪后,
   * 自动把「请读交接文件」写进 PTY 并回车;注入语引用复制命令返回的文件路径。
   * 注入语落进 transcript 后不以用户气泡出现,换成一句来历说明。
   */
  it("复制到新会话:新会话就绪后自动注入交接提示", async () => {
    window.history.replaceState({}, "", "/?sessionId=63");
    const handoffPath = "C:/tmp/meowo-handoff/1-63/handoff.md";
    const injectPrompt = zh.chat.handoffPrompt(handoffPath);
    let switched = false;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame("kimi"));
      if (command === "get_chat_history") {
        const id = (args as { sessionId: number }).sessionId;
        if (id === 63) return Promise.resolve({
          sessionId: 63, title: "旧会话", status: "running", provider: "claude", cwd: "C:/repo",
          supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: "Opus",
          connected: true, predecessorId: null, supersededBy: null,
        });
        // 复制出来的新会话:目标 provider,独立会话没有接续链;注入语已落进 transcript。
        return Promise.resolve({
          sessionId: 64, title: "新会话", status: "running", provider: "kimi", cwd: "C:/repo",
          supported: true, offset: 0, reset: false, pendingReview: null,
          items: [{ type: "user_text", id: "h1", timestamp: null, text: injectPrompt }], model: null,
          connected: true, predecessorId: null, supersededBy: null,
        });
      }
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      if (command === "list_agents") return Promise.resolve(descriptors(["claude", "kimi"]));
      if (command === "copy_session_to_provider") {
        switched = true;
        return Promise.resolve({ tempId: -7, handoffPath });
      }
      // 临时 id 认领成真 id 后,binding 轮询把窗口带到新会话。
      if (command === "managed_terminal_binding") return Promise.resolve(switched ? 64 : null);
      if (command === "managed_terminal_snapshot") {
        // 新终端有可见输出且随即安静(waitForTerminalReady 的就绪判据);注入后的回显
        // 验证从同一份画面里读到注入语——mock 无条件带上它即可。
        return Promise.resolve({
          sessionId: 64, active: true, managed: true,
          data: b64utf8(`codex ready\n${injectPrompt}`),
          startOffset: 0, endOffset: 64, exited: false, exitCode: null,
        });
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "更多模型…" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "默认模型" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("copy_session_to_provider", expect.anything()));
    expect(await screen.findByText("上下文从另一段会话复制而来")).toBeTruthy();
    expect(screen.queryByText(new RegExp(handoffPath))).toBeNull();

    // 注入语作为正文写进 PTY(与回车分两次写,先到正文即可断言)。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", {
      sessionId: 64, data: injectPrompt,
    }), { timeout: 8_000 });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", {
      sessionId: 64, data: "\r",
    }), { timeout: 3_000 });
  }, 15_000);

  /**
   * 新建会话不弹窗(对照 Kimi):对话窗主区就是新建页,写下第一句话发出去 → 起会话、
   * 切到临时 id,认领成真 id 且终端就绪后把这句话写进 PTY(与复制交接同一条注入路)。
   */
  it("新建会话页:第一句话随新会话一起发出", async () => {
    window.history.replaceState({}, "", "/?newSession=1");
    let launched = false;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "recent_cwds") return Promise.resolve(["C:/repo"]);
      if (command === "list_agents") return Promise.resolve(descriptors(["claude", "codex"]));
      if (command === "check_provider_hooks") return Promise.resolve("installed");
      if (command === "new_session") { launched = true; return Promise.resolve(-9); }
      if (command === "managed_terminal_binding") return Promise.resolve(launched ? 71 : null);
      if (command === "managed_terminal_pending_launch") return Promise.resolve({ provider: "claude", cwd: "C:/repo" });
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 71, title: "新会话", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: null,
        connected: true, predecessorId: null, supersededBy: null,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 71, active: true, managed: true, data: b64utf8("claude ready\n修一下登录页"),
        startOffset: 0, endOffset: 64, exited: false, exitCode: null,
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByTestId("nsp-input");
    await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("repo"));
    // 新建页开着时侧栏高亮的是「新建会话」这一行（对照 Kimi）。
    const newRow = document.querySelector(".chat-sidebar-new");
    expect(newRow?.className).toContain("is-active");
    fireEvent.change(input, { target: { value: "修一下登录页" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("new_session", expect.objectContaining({ provider: "claude" })));
    // 起完会话新建页让位给对话主区。
    await waitFor(() => expect(screen.queryByTestId("new-session-page")).toBeNull());
    expect(document.querySelector(".chat-sidebar-new")?.className).not.toContain("is-active");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", {
      sessionId: 71, data: "修一下登录页",
    }), { timeout: 8_000 });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", {
      sessionId: 71, data: "\r",
    }), { timeout: 3_000 });
  }, 15_000);

  /// 先上屏(用户要求):发出第一句话跳到新会话,终端还没就绪、甚至还没认领成真 id 时,
  /// 这句话就以「发送中」气泡出现,不再对着「会话正在启动…」干等。粘进来的附件随正文走
  /// 对话页同一条发送路(非图片附件走附路径)。
  it("新建会话页:第一句话先上屏,认领后不丢不重复,就绪后连同附件发进去", async () => {
    window.history.replaceState({}, "", "/?newSession=1");
    let launched = false;
    let claimed = false;
    let ready = false;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "recent_cwds") return Promise.resolve(["C:/repo"]);
      if (command === "list_agents") return Promise.resolve(descriptors(["claude", "codex"]));
      if (command === "check_provider_hooks") return Promise.resolve("installed");
      if (command === "new_session") { launched = true; return Promise.resolve(-9); }
      if (command === "managed_terminal_binding") return Promise.resolve(launched && claimed ? 71 : null);
      if (command === "managed_terminal_pending_launch") return Promise.resolve({ provider: "claude", cwd: "C:/repo" });
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 71, title: "新会话", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: null,
        connected: true, predecessorId: null, supersededBy: null,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      if (command === "save_pasted_attachment") return Promise.resolve(`C:/tmp/meowo-paste/1-0/${(args as { fileName: string }).fileName}`);
      if (command === "managed_terminal_snapshot") return Promise.resolve(ready
        ? { sessionId: 71, active: true, managed: true, data: b64utf8("claude ready"), startOffset: 0, endOffset: 64, exited: false, exitCode: null }
        : { sessionId: 71, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByTestId("nsp-input");
    await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("repo"));
    const file = { name: "notes.txt", size: 3, arrayBuffer: () => Promise.resolve(new Uint8Array([1, 2, 3]).buffer) } as unknown as File;
    fireEvent.paste(input, { clipboardData: { files: [file] } });
    await waitFor(() => expect(screen.getByTestId("nsp-attachments").textContent).toContain("notes.txt"));
    fireEvent.change(input, { target: { value: "修一下登录页" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(screen.queryByTestId("new-session-page")).toBeNull());
    // 还是临时 id、终端也没画出东西:气泡已经在了,空态文案不出现。
    expect(await screen.findByText("修一下登录页")).toBeTruthy();
    expect(screen.queryByText(zh.chat.emptyStarting)).toBeNull();
    expect(invoke).not.toHaveBeenCalledWith("write_managed_terminal", expect.anything());
    // 认领成真 id:回执跟着搬过去,只有一条。
    claimed = true;
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_chat_history", expect.objectContaining({ sessionId: 71 })));
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(screen.getAllByText("修一下登录页")).toHaveLength(1);
    ready = true;
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", {
      sessionId: 71, data: "@C:/tmp/meowo-paste/1-0/notes.txt 修一下登录页",
      // claude 声明了 attachment_mention:附件以 @路径 原生附加在正文前。
    }), { timeout: 8_000 });
    expect(screen.getAllByText("修一下登录页")).toHaveLength(1);
  }, 15_000);

  it("新建会话页:第一句话发不出去时收掉气泡,原文退回输入框并说原因", async () => {
    window.history.replaceState({}, "", "/?newSession=1");
    let launched = false;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "recent_cwds") return Promise.resolve(["C:/repo"]);
      if (command === "list_agents") return Promise.resolve(descriptors(["claude", "codex"]));
      if (command === "check_provider_hooks") return Promise.resolve("installed");
      if (command === "new_session") { launched = true; return Promise.resolve(-9); }
      if (command === "managed_terminal_binding") return Promise.resolve(launched ? 71 : null);
      if (command === "managed_terminal_pending_launch") return Promise.resolve({ provider: "claude", cwd: "C:/repo" });
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 71, title: "新会话", status: "running", provider: "claude", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: null,
        connected: true, predecessorId: null, supersededBy: null,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      // CLI 一起来就退出:等就绪那一步报错。
      if (command === "managed_terminal_snapshot") return Promise.resolve({
        sessionId: 71, active: false, managed: true, data: "", startOffset: 0, endOffset: 0, exited: true, exitCode: 1, exitTail: "boom",
      });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    const input = await screen.findByTestId("nsp-input");
    await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("repo"));
    fireEvent.change(input, { target: { value: "修一下登录页" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(screen.queryByTestId("new-session-page")).toBeNull());
    const composer = await screen.findByRole("combobox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement;
    await waitFor(() => expect(composer.value).toBe("修一下登录页"), { timeout: 8_000 });
    expect(screen.queryByText("修一下登录页", { selector: ".chat-user-text, .chat-user-text *" })).toBeNull();
    expect(document.querySelector(".chat-send-error")?.textContent ?? "").toContain("boom");
  }, 15_000);

  /// 做空复现过的几种时序:认领发生在写入途中、等就绪时又发一句、启动中切走再回来、
  /// 切走后过期。共用一套 mock:binding/就绪/历史由各用例的开关控制。
  function mockFirstPromptLaunch(state: {
    claimed: () => boolean;
    ready: () => boolean;
    items?: () => unknown[];
    onWrite?: (args: { sessionId: number; data: string }) => void;
  }) {
    let launched = false;
    // 每次新建依次拿 -9、-10…；只有 -9 会认领成 71。
    let nextTemp = -9;
    // 屏幕回显写进去的正文：提交前会核对正文确实落进了输入框。
    let typed = "";
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "recent_cwds") return Promise.resolve(["C:/repo"]);
      if (command === "list_agents") return Promise.resolve(descriptors(["claude", "codex"]));
      if (command === "check_provider_hooks") return Promise.resolve("installed");
      if (command === "new_session") { launched = true; return Promise.resolve(nextTemp--); }
      if (command === "managed_terminal_binding") {
        return Promise.resolve(launched && (args as { sessionId: number }).sessionId === -9 && state.claimed() ? 71 : null);
      }
      if (command === "managed_terminal_pending_launch") return Promise.resolve({ provider: "claude", cwd: "C:/repo" });
      if (command === "get_chat_history") {
        const id = (args as { sessionId: number }).sessionId;
        return Promise.resolve({
          sessionId: id, title: id === 71 ? "新会话" : `别的会话${id}`, status: id === 71 ? "running" : "waiting", provider: "claude", cwd: "C:/repo",
          supported: true, offset: 0, reset: false, pendingReview: null, items: id === 71 ? (state.items?.() ?? []) : [], model: null,
          connected: true, predecessorId: null, supersededBy: null,
        });
      }
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      if (command === "write_managed_terminal") {
        const write = args as { sessionId: number; data: string };
        if (write.data.length > 1) typed += " " + write.data;
        state.onWrite?.(write);
        return Promise.resolve();
      }
      if (command === "managed_terminal_snapshot") {
        const sid = Number((args as { sessionId: number }).sessionId);
        return Promise.resolve(state.ready()
          ? { sessionId: sid, active: true, managed: true, data: b64utf8("claude ready" + typed), startOffset: 0, endOffset: 64 + typed.length, exited: false, exitCode: null }
          : { sessionId: sid, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      }
      return Promise.resolve();
    });
  }
  async function sendFirstPrompt(text: string) {
    render(<ChatWindow />);
    const input = await screen.findByTestId("nsp-input");
    await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("repo"));
    fireEvent.change(input, { target: { value: text } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(screen.queryByTestId("new-session-page")).toBeNull());
  }
  const writesOf = () => invoke.mock.calls
    .filter(([command]) => command === "write_managed_terminal")
    .map(([, args]) => (args as { data: string }).data);

  it("新建会话页:写入途中认领成真 id,气泡照样结算,落盘后只剩一条", async () => {
    window.history.replaceState({}, "", "/?newSession=1");
    let wrote = false;
    let submitted = false;
    mockFirstPromptLaunch({
      claimed: () => wrote,
      ready: () => true,
      items: () => (submitted ? [{ type: "user_text", id: "u1", timestamp: null, text: "修一下登录页" }] : []),
      onWrite: ({ data }) => { wrote = true; if (data === "\r") submitted = true; },
    });
    await sendFirstPrompt("修一下登录页");
    await waitFor(() => expect(writesOf()).toContain("\r"), { timeout: 8_000 });
    await waitFor(() => expect(document.querySelectorAll(".chat-user-text")).toHaveLength(1), { timeout: 5_000 });
    expect(document.querySelector(".is-sending")).toBeNull();
  }, 20_000);

  /// 实拍（2026-11-09）：新会话还是临时 id 时又发一句，写入途中认领成真 id——回执随认领搬到真 id 下，
  /// 发送结束却按临时 id 去结算，找不到它，气泡永远停在「发送中」（消息其实已经送达）。
  it("新建会话页:临时 id 下发的第二句在写入途中认领,回执照样结算,不停在发送中", async () => {
    window.history.replaceState({}, "", "/?newSession=1");
    let claimed = false;
    mockFirstPromptLaunch({
      claimed: () => claimed,
      ready: () => true,
      onWrite: ({ data }) => { if (data === "第二句") claimed = true; },
    });
    await sendFirstPrompt("第一句");
    await waitFor(() => expect(writesOf()).toContain("第一句"), { timeout: 8_000 });
    const composer = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    fireEvent.change(composer, { target: { value: "第二句" } });
    fireEvent.keyDown(composer, { key: "Enter" });
    await waitFor(() => expect(writesOf()).toContain("第二句"), { timeout: 8_000 });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_chat_history", expect.objectContaining({ sessionId: 71 })), { timeout: 5_000 });
    await waitFor(() => expect(document.querySelector(".is-sending")).toBeNull(), { timeout: 8_000 });
    // 对话里只有一条「第二句」气泡（消息导航里另有一个标签，不算）。
    expect([...document.querySelectorAll(".chat-user-text")].filter((el) => el.textContent?.includes("第二句"))).toHaveLength(1);
  }, 30_000);

  it("新建会话页:等就绪时又发一句,第二句排在第一句话后面", async () => {
    window.history.replaceState({}, "", "/?newSession=1");
    let ready = false;
    mockFirstPromptLaunch({ claimed: () => true, ready: () => ready });
    await sendFirstPrompt("第一句");
    const composer = await screen.findByRole("combobox", { name: "发送消息给 Agent" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_chat_history", expect.objectContaining({ sessionId: 71 })));
    fireEvent.change(composer, { target: { value: "第二句" } });
    fireEvent.keyDown(composer, { key: "Enter" });
    await new Promise((resolve) => setTimeout(resolve, 1_000));
    expect(writesOf()).not.toContain("第二句");
    ready = true;
    await waitFor(() => expect(writesOf()).toContain("第二句"), { timeout: 10_000 });
    const writes = writesOf();
    expect(writes.indexOf("第一句")).toBeGreaterThanOrEqual(0);
    expect(writes.indexOf("第一句")).toBeLessThan(writes.indexOf("第二句"));
  }, 25_000);

  it("新建会话页:启动中切走、认领后再回来,第一句话照样上屏并发出去", async () => {
    window.history.replaceState({}, "", "/?newSession=1");
    let claimed = false;
    mockFirstPromptLaunch({ claimed: () => claimed, ready: () => claimed });
    await sendFirstPrompt("修一下登录页");
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 5 }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_chat_history", expect.objectContaining({ sessionId: 5 })));
    claimed = true;
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 71 }));
    expect(await screen.findByText("修一下登录页")).toBeTruthy();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("write_managed_terminal", { sessionId: 71, data: "修一下登录页" }), { timeout: 8_000 });
  }, 20_000);

  it("新建会话页:写入途中切走、随后发送失败,回到认领后的真 id 能看到未确认送达回执", async () => {
    window.history.replaceState({}, "", "/?newSession=1");
    let claimed = false;
    mockFirstPromptLaunch({ claimed: () => claimed, ready: () => true });
    // 正文写进去了，提交那一下过一会儿失败（PTY 断了）。
    const base = invoke.getMockImplementation()!;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      const data = (args as { data?: string } | undefined)?.data;
      if (command === "write_managed_terminal" && data !== undefined && data.length === 1 && data.charCodeAt(0) === 13) {
        return new Promise((_, reject) => setTimeout(() => reject("pty/closed"), 800));
      }
      return base(command, args);
    });
    await sendFirstPrompt("修一下登录页");
    await waitFor(() => expect(writesOf()).toContain("修一下登录页"), { timeout: 8_000 });
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 5 }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_chat_history", expect.objectContaining({ sessionId: 5 })));
    await new Promise((resolve) => setTimeout(resolve, 1_500));
    // 人在会话 5：不往它的输入框里塞。
    expect((screen.getByRole("combobox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement).value).toBe("");
    // 错误条也不该挂在不相干的会话 5 上。
    expect(document.querySelector(".chat-send-error")).toBeNull();
    claimed = true;
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 71 }));
    // 回到真 id 先查一次认领关系，回执才搬过来。
    expect(await screen.findByText(zh.chat.echoUncertain, undefined, { timeout: 3_000 })).toBeTruthy();
    expect(screen.getByText("修一下登录页")).toBeTruthy();
  }, 20_000);

  it("新建会话页:第一个还没发出去就开第二个,第一个的第一句话转成失败气泡而不是一直发送中", async () => {
    window.history.replaceState({}, "", "/?newSession=1");
    mockFirstPromptLaunch({ claimed: () => false, ready: () => false });
    await sendFirstPrompt("A的第一句");
    expect(await screen.findByText("A的第一句")).toBeTruthy();
    act(() => eventListeners.get("chat-new-session")?.({ payload: null }));
    const input = await screen.findByTestId("nsp-input");
    await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("repo"));
    fireEvent.change(input, { target: { value: "B的第一句" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(screen.queryByTestId("new-session-page")).toBeNull());
    act(() => eventListeners.get("chat-session-changed")?.({ payload: -9 }));
    await waitFor(() => expect(screen.getByText("A的第一句")).toBeTruthy());
    expect(await screen.findByText(zh.chat.echoFailed)).toBeTruthy();
  }, 20_000);

  it("新建会话页:切走后过期,不把第一句话塞进正在看的别的会话", async () => {
    window.history.replaceState({}, "", "/?newSession=1");
    mockFirstPromptLaunch({ claimed: () => false, ready: () => false });
    await sendFirstPrompt("修一下登录页");
    act(() => eventListeners.get("chat-session-changed")?.({ payload: 5 }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_chat_history", expect.objectContaining({ sessionId: 5 })));
    const realNow = Date.now();
    const clock = vi.spyOn(Date, "now").mockImplementation(() => realNow + 11 * 60_000);
    try {
      act(() => eventListeners.get("chat-session-changed")?.({ payload: 6 }));
      await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_chat_history", expect.objectContaining({ sessionId: 6 })));
      await new Promise((resolve) => setTimeout(resolve, 300));
      const composer = screen.getByRole("combobox", { name: "发送消息给 Agent" }) as HTMLTextAreaElement;
      expect(composer.value).toBe("");
      expect(writesOf()).not.toContain("修一下登录页");
    } finally {
      clock.mockRestore();
    }
  }, 20_000);

  /// 已被接替的旧段只读:composer 禁用、上方挂「已切换引擎」横幅(向它续话会让接续链
  /// 分叉),唯一动作是「前往新会话」——直接切到链尾。
  it("已被接替的会话禁发,横幅引导前往新会话", async () => {
    window.history.replaceState({}, "", "/?sessionId=65");
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") {
        const id = (args as { sessionId: number }).sessionId;
        if (id === 65) return Promise.resolve({
          sessionId: 65, title: "被接替的", status: "ended", provider: "claude", cwd: "C:/repo",
          supported: true, offset: 1, reset: false, pendingReview: null, model: null,
          connected: false, predecessorId: null, supersededBy: 66,
          items: [{ type: "user_text", id: "u1", timestamp: null, text: "旧内容" }],
        });
        return Promise.resolve({
          sessionId: 66, title: "接替者", status: "running", provider: "codex", cwd: "C:/repo",
          supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: null,
          connected: true, predecessorId: 65, supersededBy: null,
        });
      }
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 65, active: false, managed: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    // 旧内容仍可回看;composer 禁用而非卸载(输入框在、发不出去),「恢复会话」也不该
    // 出现——恢复它会分叉接续链。
    expect(await screen.findByText("旧内容")).toBeTruthy();
    expect(await screen.findByText("本会话已切换引擎，在新会话中继续。此处仅供回看")).toBeTruthy();
    const roBox = screen.getByRole("combobox", { name: "发送消息给 Agent" });
    expect((roBox as HTMLTextAreaElement).disabled).toBe(true);
    expect(screen.queryByRole("button", { name: "恢复会话" })).toBeNull();
    // 「前往新会话」切到链尾。
    fireEvent.click(screen.getByRole("button", { name: "前往新会话" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_chat_history", expect.objectContaining({ sessionId: 66 })));
  });

  /// /clear 换代:正开着的会话被同一 PTY 的新段接替(supersededBy 由空变有)时自动跳到
  /// 新段——终端进程还是同一个,留在旧段只剩定格画面。冷打开旧段回看(首帧就带
  /// supersededBy)不跳,由上一用例覆盖。
  it("眼前发生的换代自动跟随到新段", async () => {
    window.history.replaceState({}, "", "/?sessionId=71");
    let superseded: number | null = null;
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") {
        const id = (args as { sessionId: number }).sessionId;
        if (id === 71) return Promise.resolve({
          sessionId: 71, title: "旧段", status: superseded == null ? "running" : "ended", provider: "claude", cwd: "C:/repo",
          supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: null,
          connected: superseded == null, predecessorId: null, supersededBy: superseded,
        });
        return Promise.resolve({
          sessionId: 72, title: "新段", status: "running", provider: "claude", cwd: "C:/repo",
          supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: null,
          connected: true, predecessorId: 71, supersededBy: null,
        });
      }
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 71, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    // 首帧 supersededBy 为空:只记录基线,不跳。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_chat_history", expect.objectContaining({ sessionId: 71 })));
    superseded = 72;
    // 下一轮轮询看到「由空变有」→ 自动切到新段。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_chat_history", expect.objectContaining({ sessionId: 72 })), { timeout: 3_000 });
  });

  /**
   * 复制到新会话入口不与模型清单就绪耦合:codex/kimi 这类无内联预设的 CLI,模型清单要先
   * 静默探测 /model 菜单学到标签才拼得出;若「复制到新会话」分组躲在同一道门后,这些来源
   * 就永远切不回去(点按钮只会触发探测)。有切换目标时菜单照开、不自动探测,读清单给显式入口。
   */
  it("复制到新会话:模型清单未学到时下拉仍直开,分组可达且不触发探测", async () => {
    window.history.replaceState({}, "", "/?sessionId=67");
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") return Promise.resolve({
        sessionId: 67, title: "codex 来源", status: "running", provider: "codex", cwd: "C:/repo",
        supported: true, offset: 0, reset: false, pendingReview: null, items: [], model: null,
        connected: true, predecessorId: null, supersededBy: null,
      });
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      if (command === "list_agents") return Promise.resolve(descriptors(["claude", "codex"]));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 67, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    // 就地菜单照开:清单没学到时给「读取模型清单」显式入口,「更多模型…」进市场切引擎。
    expect(await screen.findByRole("menuitem", { name: "读取模型清单" })).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: "更多模型…" }));
    expect(await screen.findByText("复制到新会话 · Claude Code")).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "默认模型" })).toBeTruthy();
    // 没学到清单不该触发 /model 探测(那会往终端里写命令、弹 TUI 菜单)。
    expect(invoke.mock.calls.some(([command, args]) =>
      command === "write_managed_terminal" && (args as { data: string }).data === "/model")).toBe(false);
    // 显式入口在回合运行中同样受「先等任务结束」保护，不往忙着的终端里写命令。
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "读取模型清单" }));
    expect(await screen.findByText(zh.chat.menuBusyOpen)).toBeTruthy();
    expect(invoke.mock.calls.some(([command, args]) =>
      command === "write_managed_terminal" && (args as { data: string }).data === "/model")).toBe(false);
  });

  /**
   * 接续会话的时间线不从空白开始:前序段(已 ended、内容静态)的完整消息内联在上方,
   * 段间以「切换至 X」分隔条衔接;注入的那条「请读交接文件」机器消息不再以用户气泡
   * 重复出现(分隔条已表达同一事实)。
   */
  it("接续会话内联展示前序段历史,交接注入语不重复上屏", async () => {
    window.history.replaceState({}, "", "/?sessionId=70");
    const handoffPath = "C:/tmp/meowo-handoff/1-70/handoff.md";
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") {
        const id = (args as { sessionId: number }).sessionId;
        if (id === 69) return Promise.resolve({
          sessionId: 69, title: "前序段", status: "ended", provider: "claude", cwd: "C:/repo",
          supported: true, offset: 2, reset: false, pendingReview: null, model: "Opus",
          connected: false, predecessorId: null, supersededBy: 70,
          items: [
            { type: "user_text", id: "u1", timestamp: null, text: "旧问题" },
            { type: "assistant_text", id: "a1", timestamp: null, text: "旧回答" },
          ],
        });
        return Promise.resolve({
          sessionId: 70, title: "接续段", status: "running", provider: "kimi", cwd: "C:/repo",
          supported: true, offset: 2, reset: false, pendingReview: null, model: null,
          connected: true, predecessorId: 69, supersededBy: null,
          items: [
            { type: "user_text", id: "u2", timestamp: null, text: zh.chat.handoffPrompt(handoffPath) },
            { type: "assistant_text", id: "a2", timestamp: null, text: "新回答" },
          ],
        });
      }
      if (command === "get_session_lineage") return Promise.resolve([
        { id: 69, provider: "claude", startedAt: 1, endedAt: 2, model: "Opus" },
        { id: 70, provider: "kimi", startedAt: 3, endedAt: null, model: null },
      ]);
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      if (command === "list_agents") return Promise.resolve(descriptors(["claude", "kimi"]));
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 70, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    // 前序段消息与段间分隔条上屏(引擎展示名来自 list_agents)。
    expect(await screen.findByText("旧问题")).toBeTruthy();
    expect(await screen.findByText("旧回答")).toBeTruthy();
    expect(await screen.findByText("切换至 Kimi Code 继续，完整上下文已交接")).toBeTruthy();
    expect(await screen.findByText("新回答")).toBeTruthy();
    // 注入语被显示层滤掉:不以用户气泡重复出现。
    expect(screen.queryByText(/请先完整阅读文件/)).toBeNull();
  });

  /**
   * 未读徽章口径:快照与差值都只数真实内容项。脱离底部期间到达的 handoff 分隔条
   * (meta 行)与前序段旧历史都不算未读(快照同步抬高,同 loadEarlier 前插);只有
   * 真正的新消息才 +1。工具调用/结果与思考块也不算(屏幕上折成一行「已运行 N 条
   * 命令」,按条数算徽章一跳几十,用户反馈数字不准)。
   */
  it("脱离底部后:接续分隔条、旧历史、工具与思考不计未读,新消息才 +1", async () => {
    window.history.replaceState({}, "", "/?sessionId=80");
    // lineage 拉到一半挂起:先把「脱离底部」的快照落下来,再放行前序段到达。
    let resolveLineage!: (value: unknown) => void;
    const lineagePending = new Promise((resolve) => { resolveLineage = resolve; });
    let newMessageArrived = false;
    const currentItems = () => [
      { type: "user_text", id: "u1", timestamp: null, text: "当前段的问题" },
      { type: "assistant_text", id: "a1", timestamp: null, text: "当前段的回答" },
      ...(newMessageArrived
        ? [
          { type: "reasoning", id: "r2", timestamp: null, text: "想一想" },
          { type: "tool_use", id: "t2", timestamp: null, name: "Bash", summary: "cargo test" },
          { type: "tool_result", id: "tr2", timestamp: null, tool_use_id: "t2", text: "ok", is_error: false },
          { type: "assistant_text", id: "a2", timestamp: null, text: "刚落地的新消息" },
        ]
        : []),
    ];
    invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
      if (command === "get_chat_history") {
        const id = (args as { sessionId: number }).sessionId;
        if (id === 79) return Promise.resolve({
          sessionId: 79, title: "前序段", status: "ended", provider: "claude", cwd: "C:/repo",
          supported: true, offset: 2, reset: false, pendingReview: null, model: null,
          connected: false, predecessorId: null, supersededBy: 80,
          items: [
            { type: "user_text", id: "p1", timestamp: null, text: "前序段旧问题" },
            { type: "assistant_text", id: "p2", timestamp: null, text: "前序段旧回答" },
          ],
        });
        const items = currentItems();
        const cursor = ((args as { offset?: number }).offset) ?? 0;
        const base = {
          sessionId: 80, title: "接续段", status: "running", provider: "kimi", cwd: "C:/repo",
          supported: true, offset: items.length, reset: false, pendingReview: null, model: null,
          connected: true, predecessorId: 79, supersededBy: null, hasMore: false,
        };
        // 增量语义:轮询只回 offset 之后的新条目(回整批会被组件再追加一遍);
        // 元信息字段必须全量保留——增量响应缺字段会把 history 里的 provider/status 冲掉。
        if (cursor > 0) {
          return Promise.resolve({ ...base, items: cursor >= items.length ? [] : items.slice(cursor) });
        }
        return Promise.resolve({ ...base, items });
      }
      if (command === "get_session_lineage") return lineagePending;
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "agent_chat_ui") return Promise.resolve(chatUi((args as { provider: string }).provider));
      if (command === "list_agents") return Promise.resolve(descriptors(["claude", "kimi"]));
      if (command === "managed_terminal_binding") return Promise.resolve(null);
      if (command === "managed_terminal_snapshot") return Promise.resolve({ sessionId: 80, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null });
      return Promise.resolve();
    });
    render(<ChatWindow />);
    expect(await screen.findByText("当前段的回答")).toBeTruthy();
    // 滚离底部:jsdom 的滚动度量恒 0,直接定义出「视口 500、内容 2000、停在顶部」。
    const scroller = document.querySelector(".chat-scroll") as HTMLElement;
    Object.defineProperty(scroller, "scrollHeight", { value: 2000, configurable: true });
    Object.defineProperty(scroller, "clientHeight", { value: 500, configurable: true });
    scroller.scrollTop = 0;
    fireEvent.scroll(scroller);
    const jumpButton = await screen.findByRole("button", { name: /回到最新/ });
    // 脱离瞬间没有未读。
    expect(jumpButton.querySelector(".chat-jump-latest-count")).toBeNull();

    // 前序段到达:2 条旧消息 + 分隔条(meta 行)前插进时间线——都不算未读。
    resolveLineage([
      { id: 79, provider: "claude", startedAt: 1, endedAt: 2, model: null },
      { id: 80, provider: "kimi", startedAt: 3, endedAt: null, model: null },
    ]);
    expect(await screen.findByText("切换至 Kimi Code 继续，完整上下文已交接")).toBeTruthy();
    expect(await screen.findByText("前序段旧问题", { selector: ".chat-text" })).toBeTruthy();
    expect(screen.getByRole("button", { name: /回到最新/ }).querySelector(".chat-jump-latest-count")).toBeNull();

    // 当前段来了思考 + 一次工具往返 + 一条正文:只有正文算,徽章 +1。
    newMessageArrived = true;
    await waitFor(() => {
      const badge = screen.getByRole("button", { name: /回到最新/ }).querySelector(".chat-jump-latest-count");
      expect(badge?.textContent).toBe("1");
    }, { timeout: 3000 });
  });

  /// 断线语言（C-18）：一次瞬时 IPC 抖动只配通用「读取失败，正在重试」；连续失败（≥3 次，
  /// ≈2s）单列为「同步中断」（role=alert）——它只说窗口与后端的通道断了，与状态徽标讲的
  /// 「agent 进程没了」（已断开）是两种事实，措辞与样式都分开。
  it("轮询连续失败后升级为「同步中断」横幅", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    respondWithHistory({
      sessionId: 7, title: "会断线的会话", status: "running", provider: "claude", cwd: null,
      supported: true, offset: 1, reset: false, pendingReview: null,
      items: [{ type: "user_text", id: "u1", timestamp: null, text: "缓存的对话" }],
    });
    render(<ChatWindow />);
    expect(await screen.findByText("缓存的对话")).toBeTruthy();
    // 通道断掉：之后每轮 650ms 轮询都失败。
    invoke.mockImplementation((command: string) => {
      if (command === "get_chat_history") return Promise.reject(new Error("ipc down"));
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      return Promise.resolve();
    });
    // 第一次失败：通用重试文案（role=status），缓存内容继续渲染。
    await waitFor(() => expect(screen.getByText(zh.chat.loadError)).toBeTruthy());
    expect(screen.getByText("缓存的对话")).toBeTruthy();
    // 连续失败升级为「同步中断」（role=alert），措辞不再与 loadError 混用。
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain(zh.chat.syncInterrupted), { timeout: 5000 });
    expect(screen.queryByText(zh.chat.loadError)).toBeNull();
  });
});
