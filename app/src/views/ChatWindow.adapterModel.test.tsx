// 适配回归：模型与思考档位切换、撤回消息、/btw 侧问。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emptyComposerFrame, invoke, terminalProps, adapter, chatUiReady } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";
import { chatUi } from "../test/agents";
import { terminalAttention } from "../terminalAttention";

describe("ChatWindow 适配回归", () => {
  const { snapshot, setup, box, writes } = adapter;

  it.each([true, false])("模型保存需要 CLI 新确认行：confirmed=%s", async (confirmed) => {
    const base = setup(509);
    let submitted = false;
    invoke.mockImplementation((command, args) => {
      if (command === "write_managed_terminal" && args?.data === "\r") submitted = true;
      if (command === "managed_terminal_snapshot") return Promise.resolve(snapshot(509,
        submitted && confirmed ? "Set model to Sonnet 5 (claude-sonnet-5)" : ""));
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-509");
    await chatUiReady();
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: /^Sonnet/ }));
    if (confirmed) {
      await waitFor(() => expect(invoke.mock.calls.some(([command]) => command === "set_session_launch_selection")).toBe(true));
    } else {
      await screen.findByText(zh.chat.modelSwitchUnconfirmed, {}, { timeout: 4000 });
      expect(invoke.mock.calls.some(([command]) => command === "set_session_launch_selection")).toBe(false);
    }
  });

  /// claude 2.1.325 真机（tests/probe_claude_model.rs）：会话里有过回合之后 `/model haiku` 不直接切，
  /// 先弹 `Switch model?`。此前等不到确认行就报「未确认」，确认框留在终端里吞掉之后的消息。
  /// 现在认出框里要切到的正是所选的模型，替用户按一次确认，等到 `Set model to` 才算切成。
  const SWITCH_DIALOG = [
    "   Switch model?",
    "   Your next response will be slower and use more tokens",
    "   This conversation is cached for the current model. Switching to Haiku 5.5 means the full history gets re-read.",
    "   ❯ 1. Yes, switch to Haiku 5.5",
    "     2. No, go back",
  ].join("\r\n");

  it("claude 对话中途切模型：认出确认框后替用户确认，等到确认行才记住选择", async () => {
    const base = setup(520);
    let enters = 0;
    invoke.mockImplementation((command, args) => {
      if (command === "write_managed_terminal" && args?.data === "\r") enters += 1;
      if (command === "managed_terminal_snapshot") return Promise.resolve(snapshot(520,
        enters === 0 ? "" : enters === 1 ? SWITCH_DIALOG
          : `${SWITCH_DIALOG}\r\n  ⎿  Set model to Haiku 5.5 and saved as your default for new sessions`));
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-520");
    await chatUiReady();
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: /^Haiku/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("set_session_launch_selection", expect.objectContaining({ sessionId: 520, option: "model", choice: "haiku" })), { timeout: 6000 });
    // 命令一次回车，确认框一次回车，不多按。
    expect(writes(520).filter((data) => data === "\r")).toHaveLength(2);
    expect(writes(520)).toContain("/model haiku");
    expect(screen.queryByText(zh.chat.modelSwitchUnconfirmed)).toBeNull();
  }, 15_000);

  it("claude 确认框里要切到的不是所选模型：不按确认键，报未确认并给看终端的出口", async () => {
    const base = setup(521);
    let enters = 0;
    invoke.mockImplementation((command, args) => {
      if (command === "write_managed_terminal" && args?.data === "\r") enters += 1;
      // 选的是 Sonnet，框里写的却是 Haiku（屏幕上是别的切换留下的框）。
      if (command === "managed_terminal_snapshot") return Promise.resolve(snapshot(521, enters === 0 ? "" : SWITCH_DIALOG));
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-521");
    await chatUiReady();
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: /^Sonnet/ }));
    await screen.findByText(zh.chat.modelSwitchUnconfirmed, {}, { timeout: 6000 });
    expect(writes(521).filter((data) => data === "\r")).toHaveLength(1);
    expect(invoke.mock.calls.some(([command]) => command === "set_session_launch_selection")).toBe(false);
  }, 15_000);

  /// 菜单型 CLI（kimi 的 /model 不接受参数）：模型目录直接读配置列出，不必先读清单；
  /// 点选后发 /model，菜单画出来就按第一列名称替用户选中——K3 不能落到 K3-256k 上。
  /// 主路线（任务 20261114-tui-model-switch）：清单里的模型交给后端按机制切（Kimi 空闲时 -r <id> -m 重启接回），
  /// 不往终端写 /model，也不开菜单识别。
  it("kimi 点选清单里的模型走切换机制，不往终端敲 /model", async () => {
    const base = setup(513, "kimi");
    invoke.mockImplementation((command, args) => {
      if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame("kimi"));
      if (command === "apply_session_settings") return Promise.resolve({ kind: "restarted", sessionId: 513, confirmed: false, reason: null });
      if (command === "agent_model_catalog") return Promise.resolve([
        { id: "kimi-code/k3", label: "K3", group: "managed:kimi-code", efforts: [], default_effort: null, is_default: false, uncertain: false },
      ]);
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-513");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_model_catalog", expect.anything()));
    await act(async () => { await Promise.resolve(); });
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "K3" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("apply_session_settings", expect.objectContaining({ sessionId: 513, model: "kimi-code/k3" })));
    expect(writes(513).join("")).not.toContain("/model");
  });

  /// 做空 H2：带参重启会杀掉 TUI 进程。终端输入框里没发的字先搬进对话页输入框，再重启。
  it("kimi 重启前把终端输入框里的草稿搬进对话页", async () => {
    const base = setup(514, "kimi");
    const frame = emptyComposerFrame("kimi")!;
    const body = frame.lines[1].slice(" │ ".length, -1);
    const draftLine = ` │ ${"> 半句没发完的话".padEnd(body.length)}│`;
    invoke.mockImplementation((command, args) => {
      if (command === "managed_terminal_frame") return Promise.resolve({ ...frame, lines: [frame.lines[0], draftLine, frame.lines[2]] });
      if (command === "apply_session_settings") return Promise.resolve({ kind: "restarted", sessionId: 514, confirmed: false, reason: null });
      if (command === "agent_model_catalog") return Promise.resolve([
        { id: "kimi-code/k3", label: "K3", group: "Kimi", efforts: [], default_effort: null, is_default: false, uncertain: false },
      ]);
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-514");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_model_catalog", expect.anything()));
    await act(async () => { await Promise.resolve(); });
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "K3" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("apply_session_settings", expect.objectContaining({ sessionId: 514, tui: expect.objectContaining({ composerEmpty: true }) })));
    await waitFor(() => expect([...document.querySelectorAll("textarea")].some((box) => box.value.includes("半句没发完的话"))).toBe(true));
  });

  it("kimi 模型直接列出,点选后在 CLI 菜单里自动选中", async () => {
    const base = setup(511, "kimi");
    invoke.mockImplementation((command, args) => {
      if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame("kimi"));
      // 会话够不着切换机制时（unsupported）退回旧的终端菜单路线：这里钉住兜底仍可用。
      if (command === "apply_session_settings") return Promise.resolve({ kind: "unsupported", sessionId: 511, confirmed: false, reason: null });
      if (command === "agent_model_catalog") return Promise.resolve([
        { id: "kimi-code/k3-256k", label: "K3-256k", group: "managed:kimi-code", efforts: [] },
        { id: "kimi-code/k3.1-pro", label: "K3.1 Pro", group: "managed:kimi-code", efforts: ["low", "high", "max"] },
        { id: "kimi-code/k3", label: "K3", group: "managed:kimi-code", efforts: [] },
      ]);
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-511");
    // 目录随会话打开就去取（读配置文件，几毫秒）；等它到位再点，模拟真实时序。
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_model_catalog", expect.anything()));
    await act(async () => { await Promise.resolve(); });
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    // 只有一组时不写组名（组名就是当前引擎）。
    expect(await screen.findByRole("menuitem", { name: "K3.1 Pro" })).toBeTruthy();
    expect(screen.queryByText("managed:kimi-code")).toBeNull();
    expect(screen.queryByRole("menuitem", { name: "读取模型清单" })).toBeNull();
    fireEvent.click(screen.getByRole("menuitem", { name: "K3" }));
    await waitFor(() => expect(writes(511)).toContain("/model"));
    await waitFor(() => expect(terminalProps.current?.onAttention).toBeTypeOf("function"));
    const menu = terminalAttention([
      "\x1b[2JSelect a model  (type to search)",
      "Tab toggle provider · ↑↓ navigate · Enter select · Esc cancel",
      "  K3-256k            Kimi Code",
      "❯ K3.1 Pro           Kimi Code ← current",
      "  K3                 Kimi Code",
    ].join("\r\n"), [], false, true);
    expect(menu?.id).toBe("interactive:cursor-menu");
    act(() => (terminalProps.current!.onAttention as (value: unknown) => void)(menu));
    await waitFor(() => expect(writes(511)).toContain("\x1b[B\r"));
    // 全程静默：CLI 的菜单卡与「正在识别」条都不上屏（用户实拍反馈）。
    expect(screen.queryByText(zh.chat.slashMenuOpened)).toBeNull();
    expect(screen.queryByText("Select a model  (type to search)")).toBeNull();
  });

  /// 当前屏没有点选的模型（kimi 列表分页、分 provider 标签页）：把名字打进菜单搜索框，
  /// 过滤后的菜单重画再认一次；搜过仍没有才收掉（Esc 两下：先清搜索词再关菜单）并报一句。
  /// 全程不把 CLI 菜单卡甩上来。
  it.each(["found", "missing"])("kimi 点选的模型不在当前屏时先搜索再选：%s", async (mode) => {
    const base = setup(512, "kimi");
    invoke.mockImplementation((command, args) => {
      if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame("kimi"));
      if (command === "apply_session_settings") return Promise.resolve({ kind: "unsupported", sessionId: 512, confirmed: false, reason: null });
      if (command === "agent_model_catalog") return Promise.resolve([
        { id: "反向推理/DeepSeek-V4.1-Flash-1017", label: "DeepSeek V4.1 Flash", group: "反向推理", efforts: [] },
      ]);
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-512");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_model_catalog", expect.anything()));
    await act(async () => { await Promise.resolve(); });
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "DeepSeek V4.1 Flash" }));
    await waitFor(() => expect(writes(512)).toContain("/model"));
    await waitFor(() => expect(terminalProps.current?.onAttention).toBeTypeOf("function"));
    const show = (rows: string[]) => {
      const menu = terminalAttention([
        "\x1b[2JSelect a model",
        "Tab toggle provider · ↑↓ navigate · Enter select · Esc cancel",
        ...rows,
      ].join("\r\n"), [], false, true);
      act(() => (terminalProps.current!.onAttention as (value: unknown) => void)(menu));
    };
    show(["❯ K3.1 Pro           Kimi Code ← current", "  K3                 Kimi Code"]);
    await waitFor(() => expect(writes(512)).toContain("DeepSeek V4.1 Flash"));
    if (mode === "found") {
      show(["❯ DeepSeek V4.1 Flash (local TensorFold)   反向推理", "  DeepSeek V4.1 Pro   反向推理"]);
      await waitFor(() => expect(writes(512).filter((data) => data === "\r").length).toBeGreaterThan(1));
    } else {
      show(["❯ K3.1 Pro           Kimi Code ← current", "  K3                 Kimi Code"]);
      await waitFor(() => expect(writes(512).filter((data) => data === "\x1b")).toHaveLength(2));
      expect(await screen.findByText(zh.chat.modelPickNotFound("DeepSeek V4.1 Flash"))).toBeTruthy();
    }
    expect(screen.queryByText("Select a model")).toBeNull();
  });

  /// kimi 的思考档位随模型（模型目录的 support_efforts，前面补 off）；切换走 /effort <档位>，
  /// 认 `Thinking set to <档位>.`。
  it("kimi 思考档位按当前模型列出并切换", async () => {
    const base = setup(513, "kimi");
    let submitted = false;
    invoke.mockImplementation((command, args) => {
      if (command === "get_chat_history") return base(command, args).then((h: object) => ({ ...h, model: "K3.1 Pro", effort: "high" }));
      if (command === "agent_model_catalog") return Promise.resolve([
        { id: "kimi-code/k3.1-pro", label: "K3.1 Pro", group: "managed:kimi-code", efforts: ["off", "low", "high", "max"] },
      ]);
      if (command === "write_managed_terminal" && args?.data === "\r") submitted = true;
      // 往终端打命令前先认输入框（与发消息同一条规则）：给出实拍的空输入框。
      if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame("kimi"));
      if (command === "managed_terminal_snapshot") return Promise.resolve(snapshot(513, submitted ? "Thinking set to max." : ""));
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-513");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_model_catalog", expect.anything()));
    expect(await screen.findByText("· High")).toBeTruthy();
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    expect(screen.getByRole("radio", { name: "Off" })).toBeTruthy();
    fireEvent.click(screen.getByRole("radio", { name: "Max" }));
    await waitFor(() => expect(writes(513)).toContain("/effort max"));
    expect(await screen.findByText("· Max", {}, { timeout: 4000 })).toBeTruthy();
  });

  /// 撤回（对照 Kimi 用户消息上的「撤回」）：轮数 = 这条起往后的非插话用户消息数；
  /// 被撤的消息从对话里消失才算成（undone 的空 reset 也要照单清屏），原文放回输入框；
  /// CLI 拒绝时把原话带出来，输入框不动。
  it.each(["done", "rejected"])("kimi 撤回消息：%s", async (mode) => {
    const base = setup(514, "kimi");
    let submitted = false;
    const items = [
      { type: "user_text", id: "u1", timestamp: null, text: "先做甲" },
      { type: "assistant_text", id: "a1", timestamp: null, text: "甲做完了" },
      { type: "user_text", id: "u2", timestamp: null, text: "再做乙" },
      { type: "user_text", id: "u3", timestamp: null, text: "顺便看看", interjected: true },
      { type: "assistant_text", id: "a2", timestamp: null, text: "乙做完了" },
    ];
    invoke.mockImplementation((command, args) => {
      if (command === "get_chat_history") return base(command, args).then((h: object) => submitted && mode === "done"
        ? { ...h, reset: true, undone: true, offset: 10, items: [] }
        : { ...h, offset: 5, items });
      if (command === "confirm_dialog") return Promise.resolve(true);
      if (command === "write_managed_terminal" && args?.data === "\r") submitted = true;
      if (command === "managed_terminal_snapshot") return Promise.resolve(snapshot(514,
        submitted && mode === "rejected" ? "Nothing to undo: would cross a compaction boundary" : ""));
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("甲做完了");
    // 插话不单独给撤回（它不是一轮）：两条正常用户消息各一颗。
    const undoButtons = await screen.findAllByRole("button", { name: zh.chat.undoTip });
    expect(undoButtons).toHaveLength(2);
    fireEvent.click(undoButtons[0]);
    await waitFor(() => expect(writes(514)).toContain("/undo 2"));
    if (mode === "done") {
      await screen.findByText(zh.chat.undoDone, {}, { timeout: 4000 });
      expect(screen.queryByText("甲做完了")).toBeNull();
      expect(box().value).toBe("先做甲");
    } else {
      await screen.findByText(zh.chat.undoRejected("Nothing to undo: would cross a compaction boundary"), {}, { timeout: 4000 });
      expect(box().value).toBe("");
    }
  });

  /// claude 撤回（2.1.314 probe_claude_rewind）：`/rewind` 打开菜单，上移 N 格、核对选中行与
  /// 确认页引文、按「Restore conversation」的数字（有代码改动时是第 2 项）；不写 transcript，
  /// 以原文回到 CLI 输入框为准，本地收掉被撤的消息，再双 Esc 清 CLI 输入框。选中的不是要撤的
  /// 那条就退出菜单报错，什么都不撤。
  it.each(["done", "mismatch"])("claude 撤回消息：%s", async (mode) => {
    const base = setup(516, "claude");
    const items = [
      { type: "user_text", id: "u1", timestamp: null, text: "先做甲" },
      { type: "assistant_text", id: "a1", timestamp: null, text: "甲做完了" },
      { type: "user_text", id: "u2", timestamp: null, text: "再做乙" },
      { type: "assistant_text", id: "a2", timestamp: null, text: "乙做完了" },
    ];
    // 模拟 claude 整屏：列表从旧到新，光标初始在最底的 (current)。
    let stage: "idle" | "list" | "confirm" | "restored" | "cleared" = "idle";
    let ups = 0;
    let escs = 0;
    let typed = "";
    const listed = mode === "done" ? ["先做甲", "再做乙"] : ["别的消息", "再做乙"];
    const composer = (text: string) => ["────────", `❯ ${text}`, "────────"];
    const screenNow = () => {
      if (stage === "list") {
        const rows = [...listed, "(current)"];
        const at = rows.length - 1 - ups;
        return ["❯ 先做甲", "● 甲做完了", "▔▔▔▔", "   Rewind", ...rows.map((row, index) => (index === at ? "   ❯ " : "     ") + row), "   Enter to continue · Esc to cancel"];
      }
      if (stage === "confirm") return ["   Rewind", "   │ 先做甲", "   │ (9s ago)", "   ❯ 1. Restore code and conversation", "     2. Restore conversation", "     3. Restore code"];
      if (stage === "restored") return ["❯ 先做甲", ...composer("先做甲")];
      return ["❯ 先做甲", "● 甲做完了", ...composer("")];
    };
    invoke.mockImplementation((command, args) => {
      // 真实后端的增量只带新内容：首读给全量，之后空增量（撤回不写 transcript）。
      if (command === "get_chat_history") return base(command, args).then((h: object) => ({ ...h, offset: 5, items: args?.offset ? [] : items }));
      if (command === "confirm_dialog") return Promise.resolve(true);
      if (command === "managed_terminal_screen") return Promise.resolve(screenNow());
      if (command === "write_managed_terminal") {
        const data = String(args?.data);
        if (data === "\r" && typed.endsWith("/rewind")) { stage = "list"; typed = ""; }
        else if (data === "\r" && stage === "list") stage = "confirm";
        else if (data === "\u001b[A" && stage === "list") ups += 1;
        else if (data === "2" && stage === "confirm") stage = "restored";
        else if (data === "\u001b") {
          escs += 1;
          if (stage === "list" || stage === "confirm") stage = "idle";
          else if (stage === "restored" && escs >= 2) stage = "cleared";
        } else typed += data;
      }
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("甲做完了");
    // 菜单式能撤任意一条：两条用户消息各一颗。
    const undoButtons = await screen.findAllByRole("button", { name: zh.chat.undoTip });
    expect(undoButtons).toHaveLength(2);
    fireEvent.click(undoButtons[0]);
    await waitFor(() => expect(writes(516)).toContain("/rewind"));
    if (mode === "done") {
      await screen.findByText(zh.chat.undoDone, {}, { timeout: 6000 });
      expect(ups).toBe(2);
      expect(writes(516)).toContain("2");
      expect(stage).toBe("cleared");
      expect(screen.queryByText("甲做完了")).toBeNull();
      expect(box().value).toBe("先做甲");
    } else {
      await screen.findByText(zh.chat.undoMenuMismatch, {}, { timeout: 6000 });
      expect(stage).toBe("idle");
      expect(screen.queryAllByText("甲做完了").length).toBeGreaterThan(0);
      expect(box().value).toBe("");
    }
  }, 15000);

  /// claude 撤回时终端输入框里有草稿：以前直接报「清空后再撤回」让用户去终端清（用户反馈：不要
  /// 让用户去终端清）。现在读出整段草稿、双 Esc 清掉、搬进对话页输入框，再照常撤回。
  /// 认不出清空（双 Esc 后框里还有字）才退回原来的提示，且不发 /rewind。
  it.each(["moved", "stuck"])("claude 撤回时终端输入框有草稿：%s", async (mode) => {
    const base = setup(517, "claude");
    const items = [
      { type: "user_text", id: "u1", timestamp: null, text: "先做甲" },
      { type: "assistant_text", id: "a1", timestamp: null, text: "甲做完了" },
    ];
    let stage: "idle" | "list" | "confirm" | "restored" | "cleared" = "idle";
    let draft: string[] = ["终端里写了一半", "  第二行"];
    let draftEscs = 0;
    let escs = 0;
    let ups = 0;
    let typed = "";
    const composer = (lines: string[]) => ["────────", `❯ ${lines[0] ?? ""}`, ...lines.slice(1), "────────"];
    const screenNow = () => {
      if (stage === "list") {
        const rows = ["先做甲", "(current)"];
        const at = rows.length - 1 - ups;
        return ["   Rewind", ...rows.map((row, index) => (index === at ? "   ❯ " : "     ") + row), "   Enter to continue · Esc to cancel"];
      }
      if (stage === "confirm") return ["   Rewind", "   │ 先做甲", "   ❯ 1. Restore conversation"];
      if (stage === "restored") return ["❯ 先做甲", ...composer(["先做甲"])];
      return ["❯ 先做甲", "● 甲做完了", ...composer(draft)];
    };
    invoke.mockImplementation((command, args) => {
      if (command === "get_chat_history") return base(command, args).then((h: object) => ({ ...h, offset: 5, items: args?.offset ? [] : items }));
      if (command === "confirm_dialog") return Promise.resolve(true);
      if (command === "managed_terminal_screen") return Promise.resolve(screenNow());
      // 生产宿主的帧带终端列数：claude 的输入框以终端右沿为界，多行草稿要靠它确认两行之间是换行
      // 而不是折行，确认不了就不清（保不住就不清）。
      if (command === "managed_terminal_frame") return Promise.resolve({ lines: screenNow(), cursorRow: 0, cursorCol: 0, cursorVisible: false, cursorDim: false, cols: 120 });
      if (command === "write_managed_terminal") {
        const data = String(args?.data);
        if (data === "\u001b" && stage === "idle" && draft.length > 0) {
          // claude：第一下 Esc 只提示「再按一次清空」，第二下才清；stuck 模式清不掉。
          draftEscs += 1;
          if (draftEscs >= 2 && mode === "moved") draft = [];
        } else if (data === "\r" && typed.endsWith("/rewind")) { stage = "list"; typed = ""; }
        else if (data === "\r" && stage === "list") stage = "confirm";
        else if (data === "\u001b[A" && stage === "list") ups += 1;
        else if (data === "1" && stage === "confirm") stage = "restored";
        else if (data === "\u001b") {
          escs += 1;
          if (stage === "restored" && escs >= 2) stage = "cleared";
        } else typed += data;
      }
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("甲做完了");
    fireEvent.click(await screen.findByRole("button", { name: zh.chat.undoTip }));
    if (mode === "moved") {
      await screen.findByText(zh.chat.undoDone, {}, { timeout: 8000 });
      expect(draftEscs).toBe(2);
      expect(stage).toBe("cleared");
      // 撤回的原文在前，终端草稿接在后面，一个字不丢。
      expect(box().value).toBe("先做甲\n\n终端里写了一半\n第二行");
      expect(screen.queryByText(zh.chat.undoComposerBusy)).toBeNull();
    } else {
      await screen.findByText(zh.chat.undoComposerBusy, {}, { timeout: 8000 });
      expect(writes(517)).not.toContain("/rewind");
      expect(box().value).toBe("");
    }
  }, 20000);

  /// claude 侧问（2.1.314 probe_claude_rewind 实测 /btw）：答案只在终端覆盖层、不写 transcript。
  /// 对话页拦下 `/btw <问题>`，读下覆盖层里的答案挂成侧问卡，读完按 Esc 收起覆盖层。
  it("claude 侧问：/btw 的答案挂成卡片并收起覆盖层", async () => {
    const base = setup(517, "claude");
    let stage: "idle" | "answering" | "answered" = "idle";
    let polls = 0;
    let typed = "";
    const screenNow = () => {
      if (stage === "idle") return ["❯ 先做甲", "● 甲做完了", "────────", "❯ ", "────────"];
      if (stage === "answering" && polls++ < 1) return ["▔▔▔▔", "    /btw 刚才做了什么？", "      · Answering…", "    Esc to close"];
      stage = "answered";
      return ["▔▔▔▔", "    /btw 刚才做了什么？", "      做了甲。", "      没做乙。", "    ↑/↓ to scroll · c to copy · f to fork · Esc to close"];
    };
    invoke.mockImplementation((command, args) => {
      if (command === "managed_terminal_screen") return Promise.resolve(screenNow());
      if (command === "write_managed_terminal") {
        const data = String(args?.data);
        if (data === "\r" && typed.includes("/btw 刚才做了什么？")) { stage = "answering"; typed = ""; }
        else if (data === "\u001b") stage = "idle";
        else typed += data;
      }
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-517");
    // 侧问通道依赖 chatUi 声明：等它到位再发，否则走的是普通发送。
    await waitFor(() => expect(invoke.mock.calls.some(([command]) => command === "agent_chat_ui")).toBe(true));
    await act(async () => {});
    fireEvent.change(box(), { target: { value: "/btw 刚才做了什么？" } });
    fireEvent.keyDown(box(), { key: "Enter" });
    expect(await screen.findByText(/做了甲。\s*没做乙。/, {}, { timeout: 6000 })).toBeTruthy();
    expect(stage).toBe("idle");
    expect(box().value).toBe("");
    fireEvent.click(screen.getByRole("button", { name: zh.chat.sideQuestionClose }));
    expect(screen.queryByText(/做了甲/)).toBeNull();
  }, 15000);

  /// opencode 撤回（1.19.3 probe_opencode_undo）：`/undo` 不带参、一次一轮，只挂最后一条；
  /// 确认框说清会还原文件；撤唯一一轮靠换代重导（reset 不带 undone）也要清屏；原文被 CLI
  /// 放回它自己的输入框，认出后按 Ctrl-C 清掉——认不出输入框则不按（空框 Ctrl-C 是退出）。
  // stubborn：认得出输入框，但第一次按清空键没清掉（CLI 重画慢），错误条的「清空终端输入框」再按一次就干净了。
  it.each(["cleared", "unrecognized", "stubborn"])("opencode 撤回消息：%s", async (mode) => {
    const base = setup(515, "opencode");
    let submitted = false;
    let cleared = false;
    let presses = 0;
    const items = [
      { type: "user_text", id: "u1", timestamp: null, text: "先做甲" },
      { type: "assistant_text", id: "a1", timestamp: null, text: "甲做完了" },
    ];
    const confirms: unknown[] = [];
    invoke.mockImplementation((command, args) => {
      if (command === "get_chat_history") return base(command, args).then((h: object) => submitted
        ? { ...h, reset: true, offset: 0, items: [] }
        : { ...h, offset: 5, items });
      if (command === "confirm_dialog") { confirms.push(args); return Promise.resolve(true); }
      if (command === "write_managed_terminal" && args?.data === "\r") submitted = true;
      if (command === "write_managed_terminal" && args?.data === "\u0003") { presses += 1; cleared = mode !== "stubborn" || presses >= 2; }
      // 真机形态（1.19.5）：历史区的用户消息块与输入框之间隔着回合状态行；输入框 = 顶线、正文、空行、状态行、底边。
      if (command === "managed_terminal_screen") return Promise.resolve(mode === "unrecognized"
        ? ["  先做甲"]
        : ["  ┃  先做甲", "     ▣  Build · model", "  ┃", submitted && !cleared ? "  ┃  先做甲" : "  ┃", "  ┃", "  ┃  Build · model", "  ╹▀▀▀▀"]);
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("甲做完了");
    const undoButtons = await screen.findAllByRole("button", { name: zh.chat.undoTip });
    expect(undoButtons).toHaveLength(1);
    fireEvent.click(undoButtons[0]);
    await waitFor(() => expect(writes(515)).toContain("/undo"));
    expect(writes(515)).not.toContain("/undo 1");
    expect(JSON.stringify(confirms)).toContain(zh.chat.undoConfirmFiles);
    // 一次只能撤最近一轮，确认框里写明（走查输入 C3）。
    expect((confirms[0] as { message: string }).message.endsWith(zh.chat.undoLatestOnly)).toBe(true);
    await waitFor(() => expect(screen.queryByText("甲做完了")).toBeNull(), { timeout: 4000 });
    // 旧消息消失与输入框回填是先后两步：CI 慢机器上断言落在两步之间就挂（v0.6.19 发版实遇）。
    await waitFor(() => expect(box().value).toBe("先做甲"), { timeout: 4000 });
    const barButtons = () => [...document.querySelector(".chat-send-error")!.querySelectorAll("button")].map((button) => button.textContent);
    if (mode === "cleared") {
      await screen.findByText(zh.chat.undoDone, {}, { timeout: 4000 });
      expect(cleared).toBe(true);
    } else if (mode === "unrecognized") {
      // 认不出输入框：按清空键不安全（空框 Ctrl-C 是退出），写明原因，只给看终端画面，不给注定失败的重试。
      await screen.findByText(zh.chat.undoInputUnreadable, {}, { timeout: 4000 });
      expect(cleared).toBe(false);
      expect(barButtons()).toEqual([zh.chat.goTerminal, zh.chat.slashMenuDismiss]);
    } else {
      // 走查 C-4 / 用户 2026-11-10「不要让用户去终端去清除」：认得出却没清掉时，错误条上给
      // 「清空终端输入框」（再按一次 CLI 的清空键）与看终端画面，不再叫人去终端清。
      await screen.findByText(zh.chat.undoInputResidue, {}, { timeout: 4000 });
      expect(cleared).toBe(false);
      expect(barButtons()).toEqual([zh.chat.undoClearInput, zh.chat.goTerminal, zh.chat.slashMenuDismiss]);
      fireEvent.click(screen.getByRole("button", { name: zh.chat.undoClearInput }));
      await screen.findByText(zh.chat.undoDone, {}, { timeout: 4000 });
      expect(cleared).toBe(true);
      expect(presses).toBe(2);
    }
  }, 15000);

  /// 思考程度（对照 Kimi 模型菜单的「思考」分段）：statusline 报了档位才显示；切换写
  /// `/effort <档位>`，只认 CLI 的确认行，被拒时把 CLI 原话带出来。
  it.each(["confirmed", "rejected"])("思考程度切换需要 CLI 确认行：%s", async (mode) => {
    const base = setup(510);
    let submitted = false;
    invoke.mockImplementation((command, args) => {
      if (command === "get_chat_history") return base(command, args).then((h: object) => ({ ...h, effort: "high" }));
      if (command === "write_managed_terminal" && args?.data === "\r") submitted = true;
      if (command === "managed_terminal_snapshot") return Promise.resolve(snapshot(510,
        !submitted ? "" : mode === "confirmed" ? "Set effort level to Max: deepest reasoning" : "Effort 'max' isn't supported by this model"));
      return base(command, args);
    });
    render(<ChatWindow />);
    await screen.findByText("适配-510");
    expect(await screen.findByText("· High")).toBeTruthy();
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    fireEvent.click(await screen.findByRole("radio", { name: "Max" }));
    await waitFor(() => expect(writes(510)).toContain("/effort max"));
    if (mode === "confirmed") {
      expect(await screen.findByText("· Max", {}, { timeout: 4000 })).toBeTruthy();
    } else {
      await screen.findByText(zh.chat.effortRejected("Effort 'max' isn't supported by this model"), {}, { timeout: 4000 });
    }
  });
});
