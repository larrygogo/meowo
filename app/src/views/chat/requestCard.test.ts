import { describe, expect, it } from "vitest";
import type { RuntimeRequest } from "../../api";
import { en } from "../../i18n/en";
import { zh } from "../../i18n/zh";
import { terminalAttention, type TerminalAttention } from "../../terminalAttention";
import { grammar } from "../../test/agents";
import {
  CLAUDE_APPROVAL_SCREEN,
  CLAUDE_BROKER_APPROVAL,
  CODEX_BROKER_APPROVAL,
  GEMINI_APPROVAL_SCREEN,
  KIMI_APPROVAL_SCREEN,
  OPENCODE_PERMISSION_SCREEN,
} from "../../test/requestSamples";
import {
  approvalCardModel,
  approvalEndedNotice,
  cardBadge,
  cardKindOfAttention,
  cardStateOf,
  cardTitle,
  denyReasonOf,
  terminalApprovalOptions,
  terminalFeedbackFor,
  terminalInputForChoice,
  type CardKind,
} from "./requestCard";
import { asTerminalRuntimeRequest } from "./runtimeRequest";

/// 屏幕样本 → 识别 → 请求，与生产路径同一条链。
function screenRequest(provider: string, screen: string, writable = true): { request: RuntimeRequest; attention: TerminalAttention } {
  const attention = terminalAttention(screen, [], false, false, grammar(provider));
  if (!attention) throw new Error(`${provider} 的样本没认出来`);
  return { attention, request: asTerminalRuntimeRequest({ sessionId: 7, provider, attention, writable }) };
}

const SCREENS: [string, string][] = [
  ["claude", CLAUDE_APPROVAL_SCREEN],
  ["kimi", KIMI_APPROVAL_SCREEN],
  ["gemini", GEMINI_APPROVAL_SCREEN],
  ["opencode", OPENCODE_PERMISSION_SCREEN],
];

/// 手写一屏选项时用：按这家插件声明的规则走一遍识别，拿到带角色的选项（不手填角色）。
function recognized(provider: string, screen: string): TerminalAttention {
  const attention = terminalAttention(screen, [], false, false, grammar(provider));
  if (!attention) throw new Error(`${provider} 的屏幕没认出来`);
  return attention;
}

describe("审批卡模型：五家各一份样本，能力集合对得上", () => {
  it("claude（broker）：允许一次、拒绝、说明并拒绝、一个持久放行；不能仅收起", () => {
    const model = approvalCardModel(CLAUDE_BROKER_APPROVAL, zh);
    expect(model.channel).toBe("broker");
    expect(model.actions).toEqual({
      allowOnce: true,
      deny: true,
      denyWithReason: true,
      persistent: [{ label: "允许并记住（此项目、本机） · cargo test", tip: "允许并记住（此项目、本机） · cargo test" }],
      others: [],
      collapse: false,
    });
    // Bash 参数只摆命令本身。
    expect(model.detail).toEqual({ label: "command", text: "cargo test" });
    expect(model.tool).toBe("Bash");
  });

  it("codex（broker）：允许一次、拒绝、说明并拒绝；没有持久放行建议就不出那组按钮", () => {
    const model = approvalCardModel(CODEX_BROKER_APPROVAL, zh);
    expect(model.actions).toEqual({ allowOnce: true, deny: true, denyWithReason: true, persistent: [], others: [], collapse: false });
    expect(model.detail).toEqual({ label: "command", text: "git push origin main" });
  });

  it("claude（屏幕，2.1.325）：命令取自两条虚线之间，说明在命令上面，Tip 行不进卡片", () => {
    const model = approvalCardModel(screenRequest("claude", CLAUDE_APPROVAL_SCREEN).request, zh);
    expect(model.channel).toBe("terminal");
    expect(model.tool).toBe("Bash");
    expect(model.detail).toEqual({ label: "command", text: "curl -s https://example.com -o out.html" });
    expect(model.description).toBe("Download example.com homepage to out.html");
    expect(model.question).toBe("Do you want to proceed?");
    expect(model.actions).toEqual({
      allowOnce: true,
      deny: true,
      // 屏幕通道也有「说明并拒绝」：No 那一项按 Tab 可以追加说明（真机走通，插件声明了打字流程）。
      denyWithReason: true,
      persistent: [
        { label: "允许并记住 · curl *", tip: "Yes, and don’t ask again for: curl *" },
        { label: "允许并切到自动模式", tip: "Yes, and switch to auto mode · auto mode handles these prompts for you" },
      ],
      others: [],
      collapse: true,
    });
  });

  it("kimi（屏幕）：本会话内允许是持久放行；Reject with feedback 是「说明并拒绝」，不另出按钮", () => {
    const model = approvalCardModel(screenRequest("kimi", KIMI_APPROVAL_SCREEN).request, zh);
    expect(model.tool).toBe("Bash");
    expect(model.detail?.label).toBe("command");
    expect(model.detail?.text).toContain("echo meowo-approval-probe-1234");
    expect(model.actions).toEqual({
      allowOnce: true,
      deny: true,
      denyWithReason: true,
      persistent: [{ label: "本会话内允许", tip: "Approve for this session" }],
      others: [],
      collapse: true,
    });
  });

  it("gemini（屏幕）：Allow for this session 是持久放行；No, suggest changes 是拒绝，没有说明并拒绝", () => {
    const model = approvalCardModel(screenRequest("gemini", GEMINI_APPROVAL_SCREEN).request, zh);
    expect(model.tool).toBe("Shell");
    expect(model.detail).toEqual({ label: "command", text: "echo meowo-probe > probe-out.txt" });
    expect(model.actions).toEqual({
      allowOnce: true,
      deny: true,
      // 那一项的值是 cancel，与 Esc 相同，没有说明输入框（0.68.0 bundle）；接口不作答也没法真机验证。
      denyWithReason: false,
      persistent: [{ label: "本会话内允许", tip: "Allow for this session" }],
      others: [],
      collapse: true,
    });
  });

  it("opencode（屏幕，1.19.5）：Allow always 有按钮，按它实际的范围说「本会话内允许」", () => {
    const model = approvalCardModel(screenRequest("opencode", OPENCODE_PERMISSION_SCREEN).request, zh);
    // opencode 屏上不单列工具名：工具行留空，第二行当说明。
    expect(model.tool).toBe("");
    expect(model.description).toBe("Access external directory C:\\Windows");
    expect(model.detail).toEqual({ label: "input", text: "Patterns\n- C:\\Windows\\*" });
    expect(model.actions).toEqual({
      allowOnce: true,
      deny: true,
      // 主会话里 Reject 直接拒绝，没有说明页。
      denyWithReason: false,
      persistent: [{ label: "本会话内允许", tip: "Allow always" }],
      others: [],
      collapse: true,
    });
  });

  it.each(SCREENS)("%s（屏幕）：三种决定各自换成终端里对应那一项的按键", (provider, screen) => {
    const { attention } = screenRequest(provider, screen);
    const options = terminalApprovalOptions(attention);
    expect(terminalInputForChoice(attention, "allow_once")).toBe(options.allowOnce?.input);
    expect(terminalInputForChoice(attention, "deny")).toBe(options.deny?.input);
    expect(terminalInputForChoice(attention, "suggestion:0")).toBe(options.persistent[0].input);
    // 「说明并拒绝」不是一串按键（走分步的打字流程），超出范围的选项也没有对应项：不写任何按键。
    expect(terminalInputForChoice(attention, "deny:理由")).toBeNull();
    expect(terminalInputForChoice(attention, "suggestion:9")).toBeNull();
    expect(terminalInputForChoice(attention, "option:0")).toBeNull();
  });

  it("屏幕通道的按键逐家核对：允许一次、拒绝、持久放行落在正确的选项上", () => {
    const keys = (provider: string, screen: string) => {
      const { attention } = screenRequest(provider, screen);
      return ["allow_once", "suggestion:0", "deny"].map((choice) => terminalInputForChoice(attention, choice));
    };
    // claude：焦点在第一项，从它往下移。拒绝是第 4 项 No。
    expect(keys("claude", CLAUDE_APPROVAL_SCREEN)).toEqual(["\r", "\x1b[B\r", "\x1b[B\x1b[B\x1b[B\r"]);
    // kimi：数字键直选。
    expect(keys("kimi", KIMI_APPROVAL_SCREEN)).toEqual(["1", "2", "3"]);
    // gemini：● 焦点在第一项。
    expect(keys("gemini", GEMINI_APPROVAL_SCREEN)).toEqual(["\r", "\x1b[B\r", "\x1b[B\x1b[B\r"]);
    // opencode：→ 切换，Esc 拒绝。
    expect(keys("opencode", OPENCODE_PERMISSION_SCREEN)).toEqual(["\r", "\x1b[C\r", "\x1b"]);
  });

  it("外部终端里跑的会话：屏幕请求不可作答（live=false），内容照常给", () => {
    const model = approvalCardModel(screenRequest("kimi", KIMI_APPROVAL_SCREEN, false).request, zh);
    expect(model.live).toBe(false);
    expect(model.detail?.text).toContain("echo meowo-approval-probe-1234");
  });

  it("破坏性命令两条通道都标出来", () => {
    const broker = approvalCardModel({ ...CODEX_BROKER_APPROVAL, input: JSON.stringify({ command: "rm -rf build" }) }, zh);
    expect(broker.risky).toBe(true);
    const screen = KIMI_APPROVAL_SCREEN.replace("echo meowo-approval-probe-1234", "rm -rf build");
    expect(approvalCardModel(screenRequest("kimi", screen).request, zh).risky).toBe(true);
    expect(approvalCardModel(CLAUDE_BROKER_APPROVAL, zh).risky).toBe(false);
  });
});

/// 做空审查（2026-11-14）的三条：选项只认出一部分时「允许一次」被别的选项顶替；gemini 的外部编辑器项
/// 被归进持久放行还带数字键；屏幕通道的「说明并拒绝」按通道写死为没有。
describe("选项按角色归位：角色由插件规则定，卡片模型不认文案、不顶替", () => {
  const model = (provider: string, attention: TerminalAttention) =>
    approvalCardModel(asTerminalRuntimeRequest({ sessionId: 7, provider, attention, writable: true }), zh);

  it("这一屏没有「允许一次」那一项：卡上不出允许一次，也不拿 Allow always 顶替", () => {
    // opencode 的选项行被窄窗裁掉了第一项。
    const screen = OPENCODE_PERMISSION_SCREEN.replace("Allow once   Allow always   Reject", "Allow always   Reject");
    const attention = recognized("opencode", screen);
    expect(attention.options?.map((option) => [option.label, option.role])).toEqual([["Allow always", "allow_persistent"], ["Reject", "deny"]]);
    const card = model("opencode", attention);
    expect(card.actions.allowOnce).toBe(false);
    expect(card.actions.persistent).toEqual([{ label: "本会话内允许", tip: "Allow always" }]);
    expect(card.actions.deny).toBe(true);
    // 「允许一次」这个决定写不出任何按键（数字键 1 走的也是它）。
    expect(terminalInputForChoice(attention, "allow_once")).toBeNull();
    expect(terminalInputForChoice(attention, "suggestion:0")).toBe("\r");
  });

  it.each([
    ["kimi", KIMI_APPROVAL_SCREEN.replace("1. Approve once", "1. Something new")],
    ["gemini", GEMINI_APPROVAL_SCREEN.replace("1. Allow once", "1. Something new")],
    ["claude", CLAUDE_APPROVAL_SCREEN.replace(" ❯ 1. Yes\n", " ❯ 1. Yes, and something new\n")],
  ])("%s 的第一项换成了别的：同样不出允许一次", (provider, screen) => {
    const attention = recognized(provider, screen);
    expect(attention.options?.some((option) => option.role === "allow_once")).toBe(false);
    expect(model(provider, attention).actions.allowOnce).toBe(false);
    expect(terminalInputForChoice(attention, "allow_once")).toBeNull();
  });

  it("gemini 的 Modify with external editor：归其他选项，按终端原文列出，不进持久放行", () => {
    const screen = [
      "╭────────────────────────────────────────────╮",
      "│ ? Edit  src/app.ts                         │",
      "│ Apply this change?                         │",
      "│                                            │",
      "│ ● 1. Allow once                            │",
      "│   2. Allow for this session                │",
      "│   3. Modify with external editor           │",
      "│   4. No, suggest changes (esc)             │",
      "╰────────────────────────────────────────────╯",
    ].join("\n");
    const attention = recognized("gemini", screen);
    expect(attention.options?.map((option) => option.role)).toEqual(["allow_once", "allow_persistent", "other", "deny"]);
    const card = model("gemini", attention);
    expect(card.actions.persistent).toEqual([{ label: "本会话内允许", tip: "Allow for this session" }]);
    expect(card.actions.others).toEqual([{ label: "Modify with external editor", tip: "Modify with external editor" }]);
    // 它有自己的决定词，按的是它在屏幕上那一项的键；数字键 3 不会落到它上面（数字键只管允许一次与持久放行）。
    expect(terminalInputForChoice(attention, "option:0")).toBe("\x1b[B\x1b[B\r");
    expect(terminalInputForChoice(attention, "suggestion:1")).toBeNull();
  });

  it("gemini 写进设置的放行项：说「允许并记住」，与只管本会话的分开", () => {
    const screen = GEMINI_APPROVAL_SCREEN.replace(
      "│   3. No, suggest changes (esc)            │",
      "│   3. Allow this command for all future sessions │\n│   4. No, suggest changes (esc)            │",
    );
    const card = model("gemini", recognized("gemini", screen));
    expect(card.actions.persistent.map((entry) => entry.label)).toEqual(["本会话内允许", "允许并记住"]);
  });

  it("claude 放行目录的建议：说「允许并记住」并带上目录，折行截掉的句尾不进按钮", () => {
    const screen = CLAUDE_APPROVAL_SCREEN.replace(
      "   2. Yes, and don’t ask again for: curl *",
      "   2. Yes, and always allow access to C:\\Users\\me\\AppData\\Local\\Temp\\probe from this\n      project",
    );
    const card = model("claude", recognized("claude", screen));
    expect(card.actions.persistent[0]).toEqual({
      label: "允许并记住 · C:\\Users\\me\\AppData\\Local\\Temp\\probe",
      tip: "Yes, and always allow access to C:\\Users\\me\\AppData\\Local\\Temp\\probe from this",
    });
  });

  it("kimi 计划审阅：计划自带的选项认不出角色，照原文列成其他选项；Revise 是说明并拒绝", () => {
    const screen = [
      "────────────────────────",
      "  ▶ Ready to build with this plan?",
      "",
      "  ▶ 1. Build with auto-accept",
      "    2. Build, ask before edits",
      "    3. Reject",
      "    4. Revise",
      "",
      "  ↑/↓ select · 1/2/3/4 choose · ↵ confirm",
      "────────────────────────",
    ].join("\n");
    const attention = recognized("kimi", screen);
    const card = model("kimi", attention);
    expect(card.actions.allowOnce).toBe(false);
    expect(card.actions.others.map((entry) => entry.label)).toEqual(["Build with auto-accept", "Build, ask before edits"]);
    expect(card.actions.deny).toBe(true);
    expect(card.actions.denyWithReason).toBe(true);
    expect(terminalInputForChoice(attention, "option:1")).toBe("2");
    expect(terminalFeedbackFor(attention)?.openInput).toBe("4");
  });

  it("旧后端没有下发选项规则（选项不带角色）：全部列成其他选项，不猜哪个是允许", () => {
    const attention: TerminalAttention = {
      id: "x:command-approval", kind: "approval", text: "Run?", details: "proceed_box",
      options: [{ label: "Yes", input: "\r" }, { label: "No", input: "\x1b[B\r" }],
    };
    const card = model("claude", attention);
    expect(card.actions).toMatchObject({ allowOnce: false, deny: false, denyWithReason: false, persistent: [] });
    expect(card.actions.others.map((entry) => entry.label)).toEqual(["Yes", "No"]);
  });
});

describe("屏幕通道的说明并拒绝：由拒绝项的打字流程决定，不看通道", () => {
  it("claude：焦点移到 No 上按 Tab，认出追加说明的那一行才打字，回车提交", () => {
    const { attention } = screenRequest("claude", CLAUDE_APPROVAL_SCREEN);
    expect(terminalFeedbackFor(attention)).toEqual({
      openInput: "\x1b[B\x1b[B\x1b[B\t",
      readyPattern: String.raw`^\s*❯\s*(?:\d+\.\s*)?no, and tell claude what to do differently\s*$`,
      submitInput: "\r",
    });
  });

  it("claude 焦点不在第一项：移动的步数跟着焦点算", () => {
    const screen = CLAUDE_APPROVAL_SCREEN.replace(" ❯ 1. Yes\n", "   1. Yes\n").replace("   4. No", " ❯ 4. No");
    expect(terminalFeedbackFor(recognized("claude", screen))?.openInput).toBe("\t");
  });

  it("kimi：按 Reject with feedback 的数字键进入输入，认出页脚才打字", () => {
    const { attention } = screenRequest("kimi", KIMI_APPROVAL_SCREEN);
    expect(terminalFeedbackFor(attention)).toEqual({
      openInput: "4",
      readyPattern: String.raw`^\s*type feedback · ↵ submit\.?\s*$`,
      submitInput: "\r",
    });
    // 这一项不另出按钮：它就是卡上的「说明并拒绝」。
    expect(terminalApprovalOptions(attention).others).toEqual([]);
  });

  it("kimi 面板里没有 Reject with feedback（只有两项）：不出说明并拒绝", () => {
    const screen = KIMI_APPROVAL_SCREEN.replace("    2. Approve for this session\n    3. Reject\n    4. Reject with feedback\n", "    2. Reject\n");
    const attention = recognized("kimi", screen);
    expect(terminalFeedbackFor(attention)).toBeNull();
    expect(approvalCardModel(asTerminalRuntimeRequest({ sessionId: 7, provider: "kimi", attention, writable: true }), zh).actions.denyWithReason).toBe(false);
  });

  it.each([
    ["gemini", GEMINI_APPROVAL_SCREEN],
    ["opencode", OPENCODE_PERMISSION_SCREEN],
  ])("%s 没有声明打字流程：不出说明并拒绝", (provider, screen) => {
    expect(terminalFeedbackFor(screenRequest(provider, screen).attention)).toBeNull();
  });

  it("决定词里的理由原样取出；别的决定不是带理由的拒绝", () => {
    expect(denyReasonOf("deny:改用 pnpm")).toBe("改用 pnpm");
    expect(denyReasonOf("deny:")).toBe("");
    expect(denyReasonOf("deny")).toBeNull();
    expect(denyReasonOf("allow_once")).toBeNull();
  });
});

describe("选项没认出来时的按键：只用插件声明过的，不猜", () => {
  const bare = (provider: string, screen: string) => recognized(provider, screen);

  it("opencode 选项行还没画出来：只有拒绝（Esc），不给允许一次（回车选的是焦点项，焦点看不出）", () => {
    const attention = bare("opencode", "  ┃  △ Permission required\n  ┃    ← Access external directory C:\\Windows");
    expect(attention.options).toBeUndefined();
    const card = approvalCardModel(asTerminalRuntimeRequest({ sessionId: 7, provider: "opencode", attention, writable: true }), zh);
    expect(card.actions).toMatchObject({ allowOnce: false, deny: true, denyWithReason: false, persistent: [], others: [] });
    expect(terminalInputForChoice(attention, "allow_once")).toBeNull();
    expect(terminalInputForChoice(attention, "deny")).toBe("\x1b");
  });

  it("gemini 没认出焦点（不出选项）：只有拒绝（Esc），不给允许一次", () => {
    // 焦点标记 ● 没抓到：回车选的是哪一项不知道。
    const attention = bare("gemini", GEMINI_APPROVAL_SCREEN.replace("│ ● 1. Allow once", "│   1. Allow once"));
    expect(attention.options).toBeUndefined();
    const card = approvalCardModel(asTerminalRuntimeRequest({ sessionId: 7, provider: "gemini", attention, writable: true }), zh);
    expect(card.actions).toMatchObject({ allowOnce: false, deny: true });
    expect(terminalInputForChoice(attention, "allow_once")).toBeNull();
    expect(terminalInputForChoice(attention, "deny")).toBe("\x1b");
  });

  /// 兄弟场景：claude 的框式、kimi 的面板式认不出选项时（面板滚出了屏幕）没有声明兜底按键，
  /// 卡上不出允许与拒绝，数字键与 Esc 也不生效。
  it.each([
    ["claude 框式", "claude", " Bash command\n ls -la\n This command requires approval\n Do you want to proceed?"],
    ["kimi 面板式", "kimi", "  ▶ Run this command?\n  $ ls -la\n  ↑/↓ select · 1/2 choose · ↵ confirm"],
  ] as const)("%s 选项没认出来：不猜按键，不出允许与拒绝", (_name, provider, screen) => {
    const attention = bare(provider, screen);
    expect(attention.options).toBeUndefined();
    const card = approvalCardModel(asTerminalRuntimeRequest({ sessionId: 7, provider, attention, writable: true }), zh);
    expect(card.actions).toMatchObject({ allowOnce: false, deny: false, denyWithReason: false, persistent: [], others: [], collapse: true });
    expect(terminalInputForChoice(attention, "allow_once")).toBeNull();
    expect(terminalInputForChoice(attention, "deny")).toBeNull();
  });

  it("认出了选项就不再用兜底按键：按的是屏幕上那一项的键", () => {
    const { attention } = screenRequest("opencode", OPENCODE_PERMISSION_SCREEN);
    expect(attention.unparsedKeys).toBeUndefined();
    expect(terminalInputForChoice(attention, "suggestion:0")).toBe("\x1b[C\r");
  });
});

describe("卡片状态：过期；在别处了结的说明", () => {
  it("broker 请求倒计时归零算过期；后端标了过期的照后端的", () => {
    expect(cardStateOf({ status: "pending" })).toBe("pending");
    expect(cardStateOf({ status: "pending" }, true)).toBe("expired");
    expect(cardStateOf({ status: "expired" })).toBe("expired");
    expect(approvalCardModel(CLAUDE_BROKER_APPROVAL, zh, true).state).toBe("expired");
    expect(approvalCardModel(CODEX_BROKER_APPROVAL, zh).state).toBe("pending");
  });

  /// 审批在卡片之外了结（终端里答了、回合被中断、超时）：卡片收走，提示条照实说原因。
  /// 此前三种情况是同一句「已在别处处理或已超时」。
  it("了结的原因后端给了就照实说：过期、已取消各一句；没给用分不清原因的那一句", () => {
    expect(approvalEndedNotice("expired", zh)).toBe("该审批已过期，已交回终端处理");
    expect(approvalEndedNotice("cancelled", zh)).toBe("该审批已取消");
    expect(approvalEndedNotice("resolved", zh)).toBe("该审批已在别处处理或已过期");
    expect(approvalEndedNotice(undefined, zh)).toBe("该审批已在别处处理或已过期");
    for (const status of ["expired", "cancelled", "resolved", undefined] as const) expect(approvalEndedNotice(status, en)).toBeTruthy();
    // 状态词与卡片徽章是同一个词。
    expect(approvalEndedNotice("expired", zh)).toContain(cardBadge("approval", "expired", zh));
  });
});

describe("标题与徽章只由种类决定", () => {
  const KINDS: CardKind[] = ["approval", "plan", "question", "choice", "trust", "startup"];

  it("每个种类一个标题、一个待处理徽章；过期是一个词，所有种类共用", () => {
    expect(KINDS.map((kind) => [cardTitle(kind, zh), cardBadge(kind, "pending", zh)])).toEqual([
      ["Agent 请求权限", "待批准"],
      ["Agent 正在等待计划审批", "待批准"],
      ["Agent 正在等待你的回答", "待回答"],
      ["终端正在等待你的选择", "待选择"],
      ["是否信任此文件夹？", "待确认"],
      ["Agent 需要你完成一个启动步骤", "待确认"],
    ]);
    for (const kind of KINDS) {
      expect(cardBadge(kind, "expired", zh)).toBe("已过期");
      // 过期后不再带倒计时。
      expect(cardBadge(kind, "expired", zh, "0:05")).toBe("已过期");
    }
    expect(cardBadge("approval", "pending", zh, "4:32")).toBe("待批准 · 4:32");
  });

  it("英文同样齐全，且每个种类的标题互不相同", () => {
    const titles = KINDS.map((kind) => cardTitle(kind, en));
    expect(new Set(titles).size).toBe(KINDS.length);
    for (const kind of KINDS) expect(cardBadge(kind, "pending", en)).toBeTruthy();
    expect(cardBadge("question", "expired", en)).toBe("Expired");
  });

  /// 走查 输入 A3：kimi 提问卡徽章曾是「待批准」，gemini 额度耗尽曾被说成「启动步骤」。
  it("kimi 提问是「待回答」，gemini 额度耗尽是「终端正在等待你的选择」而不是启动步骤", () => {
    const kimi = terminalAttention([
      " ────────────────────────────────────────",
      "  question",
      "  ? 喜欢哪种颜色？",
      "   → [1] 红色",
      "     [2] 绿色",
      "   ↑↓ select  1-2 / ↵ choose  ←/→/tab switch  esc cancel",
    ].join("\n"), [], false, false, grammar("kimi"));
    expect(kimi?.kind).toBe("question");
    expect(cardBadge(cardKindOfAttention(kimi!.kind), "pending", zh)).toBe("待回答");
    expect(asTerminalRuntimeRequest({ sessionId: 7, provider: "kimi", attention: kimi!, writable: true }).kind).toBe("question");

    const gemini = terminalAttention([
      "╭──────────────────────────────────────────────╮",
      "│ Usage limit reached for gemini-3.8-flash.    │",
      "│ /stats model for usage details               │",
      "│ ● 1. Keep trying                             │",
      "│   2. Stop                                    │",
      "╰──────────────────────────────────────────────╯",
    ].join("\n"), [], false, false, grammar("gemini"));
    const kind = cardKindOfAttention(gemini!.kind);
    expect(cardTitle(kind, zh)).toBe("终端正在等待你的选择");
    expect(cardTitle(kind, zh)).not.toBe(zh.chat.terminalPromptTitle);
    expect(cardBadge(kind, "pending", zh)).toBe("待选择");
  });

  it("种类不看规则 id：同一个 id 换了种类，标题跟着种类走", () => {
    const base = { id: "anything:command-approval", text: "x" };
    expect(asTerminalRuntimeRequest({ sessionId: 1, provider: "x", writable: true, attention: { ...base, kind: "question" } }).kind).toBe("question");
    expect(asTerminalRuntimeRequest({ sessionId: 1, provider: "x", writable: true, attention: { ...base, kind: "choice" } }).kind).toBe("prompt");
    expect(asTerminalRuntimeRequest({ sessionId: 1, provider: "x", writable: true, attention: { ...base, kind: "approval" } }).kind).toBe("approval");
    expect(cardKindOfAttention("menu")).toBe("choice");
  });
});
