import { describe, expect, it } from "vitest";
import { leaveSubagentView, readSubagentScreen } from "./subagentView";
import { SUBAGENT_VIEW } from "../../test/agents";

const COMPOSER = { input: "\u0013", composer_prompt: "❯", composer_border: "─", stashed_marker: "› stashed" };
const RULE = "─".repeat(118);

// 以下屏幕取自 2.1.322 真机探针（tests/probe_claude_subagent_view.rs）的末屏，只截底部、
// 子代理描述换成用户实拍里的那条。
const VIEWING_PANEL_FOCUSED = [
  "● 已派出",
  "✻ Wandering… (4s · ↑ 33.9k tokens)",
  `${"─".repeat(96)} 后端实现 Excel 附件解析 ─`,
  "❯ Message @general-purpose…",
  RULE,
  "  ↑/↓ to select",
  "  ◯ main",
  "❯ ● general-purpose  后端实现 Excel 附件解析                                       4s · ↓ 33.9k tokens",
];
const VIEWING_MAIN_SELECTED = [
  `${"─".repeat(96)} 后端实现 Excel 附件解析 ─`,
  "❯ Message @general-purpose…",
  RULE,
  "  ↑/↓ to select · Enter to view",
  "❯ ◯ main",
  "  ● general-purpose  后端实现 Excel 附件解析                                      11s · ↓ 36.9k tokens",
];
const MAIN_PANEL_FOCUSED = [
  "✻ Waiting for 1 background agent to finish",
  RULE,
  "❯",
  RULE,
  "  ↑/↓ to select",
  "❯ ● main",
  "  ◯ general-purpose  后端实现 Excel 附件解析                                      13s · ↓ 37.0k tokens",
];
const MAIN_IDLE = [
  "● 已派出",
  RULE,
  "❯",
  RULE,
  "  ⏵⏵ auto mode on (shift+tab to cycle) · ← for agents",
];

describe("readSubagentScreen", () => {
  it("认出子代理视图，名字取边框上的描述", () => {
    expect(readSubagentScreen(VIEWING_PANEL_FOCUSED, SUBAGENT_VIEW, COMPOSER)).toEqual({ viewing: true, name: "后端实现 Excel 附件解析", main: 1, selected: 2 });
    expect(readSubagentScreen(VIEWING_MAIN_SELECTED, SUBAGENT_VIEW, COMPOSER)).toEqual({ viewing: true, name: "后端实现 Excel 附件解析", main: 1, selected: 1 });
  });

  it("主会话不算视图，面板焦点照样读出来", () => {
    expect(readSubagentScreen(MAIN_PANEL_FOCUSED, SUBAGENT_VIEW, COMPOSER)).toEqual({ viewing: false, name: null, main: 1, selected: 1 });
    expect(readSubagentScreen(MAIN_IDLE, SUBAGENT_VIEW, COMPOSER)).toEqual({ viewing: false, name: null, main: -1, selected: -1 });
  });

  it("composer 有字时占位不画，靠边框描述加 main 行记号认", () => {
    const typed = VIEWING_PANEL_FOCUSED.map((line) => line.startsWith("❯ Message") ? "❯ 帮我看下这个报错" : line);
    expect(readSubagentScreen(typed, SUBAGENT_VIEW, COMPOSER).viewing).toBe(true);
    // 边框挂了字但 main 行是「正在看」的记号：不是子代理视图。
    const labeledMain = MAIN_PANEL_FOCUSED.map((line, index) => index === 1 ? `${"─".repeat(100)} 会话名 ─` : line);
    expect(readSubagentScreen(labeledMain, SUBAGENT_VIEW, COMPOSER).viewing).toBe(false);
  });

  it("没有边框描述时名字退回占位里的类型", () => {
    const plain = VIEWING_PANEL_FOCUSED.map((line, index) => index === 2 ? RULE : line);
    expect(readSubagentScreen(plain, SUBAGENT_VIEW, COMPOSER).name).toBe("general-purpose");
  });

  it("历史区里回显的同样文字不算", () => {
    const echoed = ["❯ Message @general-purpose…", "● 好的", RULE, "❯", RULE];
    expect(readSubagentScreen(echoed, SUBAGENT_VIEW, COMPOSER).viewing).toBe(false);
  });
});

/// 照 2.1.322 探针实测行为搭的假终端：
/// - composer 焦点下 ↓ 进面板、落在 main 行；面板里 ↑/↓ 移动；main 行回车 = 退出视图，
///   子代理行回车 = 打开它；打开或切回后面板保持焦点；面板里 Esc 取消选中。
/// - composer 焦点下 Esc = 中断（被看的子代理在跑时掐掉它的工作），记下来当事故。
/// - 回车提交 composer 里的字：在视图里交给子代理，否则交给主会话。
function fakeClaude(start: { viewing: boolean; focus: "composer" | "panel"; cursor?: number; panelShown?: boolean; ignoreKeys?: boolean }) {
  const rows = ["main", "后端实现 Excel 附件解析"];
  const state = { ...start, cursor: start.cursor ?? 1, typed: "", panelShown: start.panelShown ?? true };
  const log = { main: [] as string[], subagent: [] as string[], interrupts: 0 };
  const screen = (): string[] => {
    const lines = [
      state.viewing ? `${"─".repeat(96)} ${rows[1]} ─` : RULE,
      state.typed ? `❯ ${state.typed}` : state.viewing ? "❯ Message @general-purpose…" : "❯",
      RULE,
    ];
    if (!state.panelShown) return lines;
    lines.push("  ↑/↓ to select");
    rows.forEach((label, index) => {
      const pointer = state.focus === "panel" && state.cursor === index ? "❯ " : "  ";
      const viewed = (index === 0) !== state.viewing ? "●" : "◯";
      lines.push(index === 0 ? `${pointer}${viewed} main` : `${pointer}${viewed} general-purpose  ${label}     3s · ↓ 1k tokens`);
    });
    return lines;
  };
  const write = async (data: string) => {
    if (state.ignoreKeys) return;
    if (data === "\u001b[B") {
      if (state.focus === "composer" && state.panelShown) { state.focus = "panel"; state.cursor = 0; }
      else if (state.focus === "panel") state.cursor = Math.min(state.cursor + 1, rows.length - 1);
    } else if (data === "\u001b[A") {
      if (state.focus === "panel") state.cursor = Math.max(state.cursor - 1, 0);
    } else if (data === "\u001b") {
      if (state.focus === "panel") state.focus = "composer";
      else log.interrupts += 1;
    } else if (data === "\r") {
      if (state.focus === "panel") state.viewing = state.cursor !== 0;
      else { (state.viewing ? log.subagent : log.main).push(state.typed); state.typed = ""; }
    } else if (state.focus === "composer") {
      state.typed += data;
    } else {
      state.focus = "composer";
      state.typed += data;
    }
  };
  return { io: { screen: async () => screen(), write, sleep: async () => {} }, state, log };
}

describe("leaveSubagentView", () => {
  const send = async (fake: ReturnType<typeof fakeClaude>, text: string) => {
    const result = await leaveSubagentView(fake.io, SUBAGENT_VIEW, COMPOSER);
    if (result.ok) {
      await fake.io.write(text);
      await fake.io.write("\r");
    }
    return result;
  };

  it("面板焦点停在被看的子代理行（刚打开它）：上移到 main、回车，消息进主会话", async () => {
    const fake = fakeClaude({ viewing: true, focus: "panel", cursor: 1 });
    expect(await send(fake, "继续")).toEqual({ ok: true });
    expect(fake.log).toEqual({ main: ["继续"], subagent: [], interrupts: 0 });
    expect(fake.state.focus).toBe("composer");
  });

  it("焦点在 composer：下移进面板落在 main、回车，消息进主会话", async () => {
    const fake = fakeClaude({ viewing: true, focus: "composer" });
    expect(await send(fake, "继续")).toEqual({ ok: true });
    expect(fake.log).toEqual({ main: ["继续"], subagent: [], interrupts: 0 });
  });

  it("不在视图里不按任何键", async () => {
    const fake = fakeClaude({ viewing: false, focus: "composer" });
    const writes: string[] = [];
    const io = { ...fake.io, write: async (data: string) => { writes.push(data); await fake.io.write(data); } };
    expect(await leaveSubagentView(io, SUBAGENT_VIEW, COMPOSER)).toEqual({ ok: true });
    expect(writes).toEqual([]);
  });

  it("面板没画出来：切不回，报出子代理名，不按 Esc（那是中断）", async () => {
    const fake = fakeClaude({ viewing: true, focus: "composer", panelShown: false });
    expect(await leaveSubagentView(fake.io, SUBAGENT_VIEW, COMPOSER)).toEqual({ ok: false, name: "后端实现 Excel 附件解析" });
    expect(fake.log.interrupts).toBe(0);
  });

  it("按键没反应：有限步内放弃，面板焦点取消掉", async () => {
    const fake = fakeClaude({ viewing: true, focus: "panel", cursor: 1, ignoreKeys: true });
    const writes: string[] = [];
    const io = { ...fake.io, write: async (data: string) => { writes.push(data); } };
    expect(await leaveSubagentView(io, SUBAGENT_VIEW, COMPOSER)).toEqual({ ok: false, name: "后端实现 Excel 附件解析" });
    expect(writes.length).toBeLessThanOrEqual(7);
    expect(writes.at(-1)).toBe("\u001b");
  });
});
