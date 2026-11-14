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
  cardBadge,
  cardKindOfAttention,
  cardStateOf,
  cardTitle,
  terminalApprovalOptions,
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

describe("审批卡模型：五家各一份样本，能力集合对得上", () => {
  it("claude（broker）：允许一次、拒绝、说明并拒绝、一个持久放行；不能仅收起", () => {
    const model = approvalCardModel(CLAUDE_BROKER_APPROVAL, zh);
    expect(model.channel).toBe("broker");
    expect(model.actions).toEqual({
      allowOnce: true,
      deny: true,
      denyWithReason: true,
      persistent: [{ label: "允许并记住（此项目、本机） · cargo test", tip: "允许并记住（此项目、本机） · cargo test" }],
      collapse: false,
    });
    // Bash 参数只摆命令本身。
    expect(model.detail).toEqual({ label: "command", text: "cargo test" });
    expect(model.tool).toBe("Bash");
  });

  it("codex（broker）：允许一次、拒绝、说明并拒绝；没有持久放行建议就不出那组按钮", () => {
    const model = approvalCardModel(CODEX_BROKER_APPROVAL, zh);
    expect(model.actions).toEqual({ allowOnce: true, deny: true, denyWithReason: true, persistent: [], collapse: false });
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
      denyWithReason: false,
      persistent: [
        { label: "允许并记住 · curl *", tip: "Yes, and don’t ask again for: curl *" },
        { label: "允许并切到自动模式", tip: "Yes, and switch to auto mode · auto mode handles these prompts for you" },
      ],
      collapse: true,
    });
  });

  it("kimi（屏幕）：本会话内允许是持久放行；要打字的 Reject with feedback 不出按钮", () => {
    const model = approvalCardModel(screenRequest("kimi", KIMI_APPROVAL_SCREEN).request, zh);
    expect(model.tool).toBe("Bash");
    expect(model.detail?.label).toBe("command");
    expect(model.detail?.text).toContain("echo meowo-approval-probe-1234");
    expect(model.actions).toEqual({
      allowOnce: true,
      deny: true,
      denyWithReason: false,
      persistent: [{ label: "本会话内允许", tip: "Approve for this session" }],
      collapse: true,
    });
  });

  it("gemini（屏幕）：Allow for this session 是持久放行；No, suggest changes 是拒绝", () => {
    const model = approvalCardModel(screenRequest("gemini", GEMINI_APPROVAL_SCREEN).request, zh);
    expect(model.tool).toBe("Shell");
    expect(model.detail).toEqual({ label: "command", text: "echo meowo-probe > probe-out.txt" });
    expect(model.actions).toEqual({
      allowOnce: true,
      deny: true,
      denyWithReason: false,
      persistent: [{ label: "本会话内允许", tip: "Allow for this session" }],
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
      denyWithReason: false,
      persistent: [{ label: "本会话内允许", tip: "Allow always" }],
      collapse: true,
    });
  });

  it.each(SCREENS)("%s（屏幕）：三种决定各自换成终端里对应那一项的按键", (provider, screen) => {
    const { attention } = screenRequest(provider, screen);
    const options = terminalApprovalOptions(attention);
    expect(terminalInputForChoice(attention, "allow_once")).toBe(options.allowOnce?.input);
    expect(terminalInputForChoice(attention, "deny")).toBe(options.deny?.input);
    expect(terminalInputForChoice(attention, "suggestion:0")).toBe(options.persistent[0].input);
    // 屏幕通道没有「说明并拒绝」，超出范围的持久放行也没有对应项：不写任何按键。
    expect(terminalInputForChoice(attention, "deny:理由")).toBeNull();
    expect(terminalInputForChoice(attention, "suggestion:9")).toBeNull();
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

  it("选项没认出来的审批框：按钮名称不变，按键按「回车允许一次、Esc 拒绝」兜底", () => {
    const attention: TerminalAttention = { id: "opencode:command-approval", kind: "approval", text: "△ Permission required", details: "option_row" };
    const model = approvalCardModel(asTerminalRuntimeRequest({ sessionId: 7, provider: "opencode", attention, writable: true }), zh);
    expect(model.actions.allowOnce).toBe(true);
    expect(model.actions.deny).toBe(true);
    expect(model.actions.persistent).toEqual([]);
    expect(terminalInputForChoice(attention, "allow_once")).toBe("\r");
    expect(terminalInputForChoice(attention, "deny")).toBe("\x1b");
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

describe("卡片状态：过期与已取消", () => {
  it("broker 请求倒计时归零算过期；后端标了过期或已取消的照后端的", () => {
    expect(cardStateOf({ status: "pending" })).toBe("pending");
    expect(cardStateOf({ status: "pending" }, true)).toBe("expired");
    expect(cardStateOf({ status: "expired" })).toBe("expired");
    expect(cardStateOf({ status: "cancelled" })).toBe("cancelled");
    // 已在别处了结的请求不再可答，同样按已取消显示。
    expect(cardStateOf({ status: "resolved" })).toBe("cancelled");
    expect(approvalCardModel(CLAUDE_BROKER_APPROVAL, zh, true).state).toBe("expired");
    expect(approvalCardModel({ ...CODEX_BROKER_APPROVAL, status: "cancelled" }, zh).state).toBe("cancelled");
  });
});

describe("标题与徽章只由种类决定", () => {
  const KINDS: CardKind[] = ["approval", "plan", "question", "choice", "trust", "startup"];

  it("每个种类一个标题、一个待处理徽章；过期与已取消各一个词，所有种类共用", () => {
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
      expect(cardBadge(kind, "cancelled", zh)).toBe("已取消");
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
    expect(cardBadge("question", "cancelled", en)).toBe("Cancelled");
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
