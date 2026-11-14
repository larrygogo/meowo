/// 真机样本回放：tests/probe_approval.rs 用本机真实 CLI 抓到的审批屏幕与 broker 请求
/// （test/fixtures/approval-frames.json），过一遍生产的识别、请求归一、卡片模型与卡片组件。
///
/// 别的卡片测试用的样本有一部分取自源码或既有夹具（见 requestSamples.ts 各条的出处）。这里只放
/// 真机抓的：CLI 改版后重跑探针换样本，五家的结果在这里一起比对。
///
/// 探针作答「允许一次」时，屏幕通道写进终端的按键读的是样本里的 `allowOnceInput`。这里断言它等于
/// 生产卡片模型算出来的按键，所以探针按下去的就是对话页会按的那个键。

import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { RuntimeRequest } from "../../api";
import { zh } from "../../i18n/zh";
import { terminalAttention } from "../../terminalAttention";
import { grammar } from "../../test/agents";
import fixture from "../../test/fixtures/approval-frames.json";
import { ApprovalRequestCard } from "./ApprovalRequestCard";
import { approvalCardModel, cardBadge, cardKindOfAttention, cardTitle, terminalFeedbackFor, terminalInputForChoice } from "./requestCard";
import { asTerminalRuntimeRequest } from "./runtimeRequest";
import { typeAnswerIntoTerminal, typedAnswerReady } from "./terminalTypedAnswer";

type BrokerSample = {
  sessionId: number;
  requestId: string;
  provider: string;
  toolName: string;
  description?: string | null;
  input: string;
  permissionSuggestions?: unknown[];
};
type Frame = {
  version: string;
  channel: "broker" | "terminal";
  rule: string | null;
  allowOnceInput: string | null;
  denyFeedback?: FeedbackFlow;
  screen: string[];
  brokerRequest: BrokerSample | null;
};
type FeedbackFlow = { openInput: string; readyPattern: string; submitInput: string };
/// tests/probe_deny_feedback.rs 走「说明并拒绝」时四个时刻的真机屏幕。
type FeedbackFrame = {
  version: string;
  feedback: string;
  denyFeedback: FeedbackFlow;
  screen: string[];
  readyScreen: string[];
  typedScreen: string[];
  finalScreen: string[];
};
const FRAMES = fixture.frames as unknown as Record<string, Frame>;
const FEEDBACK = fixture.feedbackFrames as unknown as Record<string, FeedbackFrame>;

afterEach(cleanup);

/// 屏幕样本 → 识别 → 请求，与生产路径同一条链。
function screenRequest(agent: string) {
  const attention = terminalAttention(FRAMES[agent].screen.join("\n"), [], false, false, grammar(agent));
  if (!attention) throw new Error(`${agent} 的真机屏幕没认出来`);
  return { attention, request: asTerminalRuntimeRequest({ sessionId: 7, provider: agent, attention, writable: true }) };
}

/// reporter 发给 broker 的请求 → 后端下发给前端的那一份（字段一一对应，状态是待处理、broker 还握着）。
function brokerRequest(agent: string): RuntimeRequest {
  const sample = FRAMES[agent].brokerRequest;
  if (!sample) throw new Error(`${agent} 没有 broker 样本`);
  return {
    sessionId: sample.sessionId,
    requestId: sample.requestId,
    provider: sample.provider,
    kind: "approval",
    status: "pending",
    toolName: sample.toolName,
    description: sample.description ?? null,
    input: sample.input,
    permissionSuggestions: sample.permissionSuggestions ?? [],
    responseCapability: { type: "broker_live" },
  } as RuntimeRequest;
}

const base = { countdown: null, resolving: false, error: null, escArmed: false, dirs: [], returnFocusTo: { current: null } };

/// 卡片底部按钮的文字，按 DOM 序，去掉键盘徽标。
function buttonsOf(request: RuntimeRequest): string[] {
  const model = approvalCardModel(request, zh);
  render(<ApprovalRequestCard {...base} model={model} onDecide={() => {}} onCollapse={() => {}} />);
  const names = Array.from(document.querySelectorAll<HTMLButtonElement>(".chat-approval-actions button")).map((button) => {
    const clone = button.cloneNode(true) as HTMLElement;
    clone.querySelectorAll("kbd").forEach((kbd) => kbd.remove());
    return clone.textContent?.trim() ?? "";
  });
  cleanup();
  return names;
}

describe("真机审批样本：通道与识别", () => {
  it("四家都有真机样本，通道与探针记下的一致", () => {
    expect(Object.fromEntries(Object.entries(FRAMES).map(([agent, frame]) => [agent, frame.channel]))).toMatchObject({
      claude: "broker",
      codex: "broker",
      kimi: "terminal",
      opencode: "terminal",
    });
  });

  it.each(["claude", "kimi", "opencode"])("%s：真机屏幕被生产规则认成审批，规则与后端探针认出的是同一条", (agent) => {
    const { attention } = screenRequest(agent);
    expect(attention.id).toBe(FRAMES[agent].rule);
    expect(attention.kind).toBe("approval");
    const kind = cardKindOfAttention(attention.kind);
    expect(cardTitle(kind, zh)).toBe("Agent 请求权限");
    expect(cardBadge(kind, "pending", zh)).toBe("待批准");
  });

  it("codex：hook 握着请求时终端不画权限框，屏幕上认不出任何提示", () => {
    expect(FRAMES.codex.rule).toBeNull();
    expect(terminalAttention(FRAMES.codex.screen.join("\n"), [], false, false, grammar("codex"))).toBeNull();
  });
});

describe("真机审批样本：按钮集合与顺序五家同一套规则", () => {
  it("本次决定的两颗钮恒在最后，其余按能力出现", () => {
    expect({
      "claude（broker）": buttonsOf(brokerRequest("claude")),
      "codex（broker）": buttonsOf(brokerRequest("codex")),
      "kimi（屏幕）": buttonsOf(screenRequest("kimi").request),
      "opencode（屏幕）": buttonsOf(screenRequest("opencode").request),
      "claude（屏幕兜底）": buttonsOf(screenRequest("claude").request),
    }).toEqual({
      // broker：理由能送回 Agent，有「说明并拒绝」；没有「仅收起」。claude 这次给的持久放行是一个目录。
      "claude（broker）": ["本会话内允许 · C:\\Users\\me\\AppData\\Local\\Temp\\meowo-appr…", "说明并拒绝", "拒绝", "允许一次"],
      // codex 的请求不带持久放行建议。
      "codex（broker）": ["说明并拒绝", "拒绝", "允许一次"],
      // 屏幕：有「仅收起」，持久放行按终端里实际有的选项出。kimi 的 Reject with feedback、claude 的
      // No 加 Tab 真机走通了，所以屏幕通道也有「说明并拒绝」；opencode 主会话没有说明页。
      "kimi（屏幕）": ["仅收起", "本会话内允许", "说明并拒绝", "拒绝", "允许一次"],
      "opencode（屏幕）": ["仅收起", "本会话内允许", "拒绝", "允许一次"],
      "claude（屏幕兜底）": ["仅收起", "允许并记住 · C:\\Users\\me\\AppData\\Local\\Temp\\meowo-approval-probe-29232", "允许并切到自动模式", "说明并拒绝", "拒绝", "允许一次"],
    });
  });

  it("claude（broker）：目录放行建议写明是哪个目录，悬停提示是全文", () => {
    const model = approvalCardModel(brokerRequest("claude"), zh);
    expect(model.tool).toBe("Bash");
    expect(model.detail).toEqual({ label: "command", text: "echo meowo-probe > probe-out.txt" });
    expect(model.description).toBe("Run the exact echo command to write probe-out.txt");
    expect(model.actions.persistent).toEqual([{
      label: "本会话内允许 · C:\\Users\\me\\AppData\\Local\\Temp\\meowo-appr…",
      tip: "本会话内允许 · C:\\Users\\me\\AppData\\Local\\Temp\\meowo-approval-probe-29232",
    }]);
  });

  it("codex（broker）：真机的工具名是 Bash，命令从参数里取出来", () => {
    const model = approvalCardModel(brokerRequest("codex"), zh);
    expect(model.channel).toBe("broker");
    expect(model.tool).toBe("Bash");
    expect(model.detail).toEqual({ label: "command", text: "echo meowo-probe > probe-out.txt" });
    expect(model.actions.persistent).toEqual([]);
  });

  it("kimi 2.6.1（屏幕）：面板里的 cwd 行进说明，不混进命令", () => {
    const model = approvalCardModel(screenRequest("kimi").request, zh);
    expect(model.tool).toBe("Bash");
    expect(model.question).toBe("Run this command?");
    expect(model.description).toBe("cwd: C:\\Users\\me\\AppData\\Local\\Temp\\meowo-approval-probe-15712");
    expect(model.detail).toEqual({ label: "command", text: "$ echo meowo-probe > probe-out.txt" });
  });

  it("opencode 1.19.5（屏幕）：屏上不单列工具名，第二行当说明", () => {
    const model = approvalCardModel(screenRequest("opencode").request, zh);
    expect(model.tool).toBe("");
    expect(model.description).toBe("Access external directory C:\\Windows");
    expect(model.detail).toEqual({ label: "input", text: "Patterns\n- C:\\Windows\\*" });
  });
});

describe("真机审批样本：「允许一次」写进终端的按键", () => {
  it.each(["kimi", "opencode"])("%s：卡片算出来的按键就是探针真机按下、CLI 接着执行的那个", (agent) => {
    const { attention } = screenRequest(agent);
    expect(FRAMES[agent].allowOnceInput).not.toBeNull();
    expect(terminalInputForChoice(attention, "allow_once")).toBe(FRAMES[agent].allowOnceInput);
  });

  it("三家屏幕样本的允许、持久放行、拒绝按键", () => {
    const keys = (agent: string) => {
      const { attention } = screenRequest(agent);
      const persistent = approvalCardModel(screenRequest(agent).request, zh).actions.persistent;
      return [
        terminalInputForChoice(attention, "allow_once"),
        ...persistent.map((_, index) => terminalInputForChoice(attention, `suggestion:${index}`)),
        terminalInputForChoice(attention, "deny"),
      ];
    };
    // kimi：数字键直选，拒绝是第 3 项。
    expect(keys("kimi")).toEqual(["1", "2", "3"]);
    // opencode：回车选第一项，→ 切到 Allow always，Esc 拒绝。
    expect(keys("opencode")).toEqual(["\r", "\x1b[C\r", "\x1b"]);
    // claude 屏幕兜底：焦点在第一项，从它往下移；拒绝是第 4 项 No。
    expect(keys("claude")).toEqual(["\r", "\x1b[B\r", "\x1b[B\x1b[B\r", "\x1b[B\x1b[B\x1b[B\r"]);
  });
});

/// 屏幕通道的「说明并拒绝」：按键与就绪标志取自插件声明，探针照着走通（命令没有执行、模型收到了说明）。
/// 这里用探针抓下的四个时刻的屏幕回放生产逻辑。
describe("真机样本：屏幕通道的说明并拒绝", () => {
  const AGENTS = ["claude", "kimi"] as const;
  const attentionOf = (lines: string[], agent: string) => terminalAttention(lines.join("\n"), [], false, false, grammar(agent));

  it.each(AGENTS)("%s：卡片算出来的打字流程就是探针真机走通的那一套", (agent) => {
    // 探针读的是 frames 里的 denyFeedback；两处屏幕（允许一次那次、说明并拒绝那次）算出来的都得是它。
    expect(terminalFeedbackFor(screenRequest(agent).attention)).toEqual(FRAMES[agent].denyFeedback);
    const attention = attentionOf(FEEDBACK[agent].screen, agent);
    expect(terminalFeedbackFor(attention!)).toEqual(FEEDBACK[agent].denyFeedback);
    expect(FEEDBACK[agent].denyFeedback).toEqual(FRAMES[agent].denyFeedback);
  });

  it.each(AGENTS)("%s：就绪标志只在进入等输入之后的屏幕上命中", (agent) => {
    const frame = FEEDBACK[agent];
    const flow = frame.denyFeedback;
    // 审批框刚出现、还没按键：不能算就绪（否则正文会被当成按键，数字 1 就是允许）。
    expect(typedAnswerReady(frame.screen, flow)).toBe(false);
    expect(typedAnswerReady(frame.readyScreen, flow)).toBe(true);
    // 了结之后的屏幕上没有它。
    expect(typedAnswerReady(frame.finalScreen, flow)).toBe(false);
    // 被拒的命令没有执行的字样、模型按说明给出的答案都在最后一屏上。
    expect(frame.finalScreen.join("\n")).toContain("3702");
  });

  it.each(AGENTS)("%s：进入等输入之后屏幕仍被认成同一条审批，卡片不会中途换成别的卡", (agent) => {
    const before = attentionOf(FEEDBACK[agent].screen, agent);
    for (const lines of [FEEDBACK[agent].readyScreen, FEEDBACK[agent].typedScreen]) {
      const during = attentionOf(lines, agent);
      expect(during?.id).toBe(before?.id);
      expect(during?.kind).toBe("approval");
    }
  });

  it.each(AGENTS)("%s：按真机屏幕回放三步，写进终端的依次是进入键、说明、提交键", async (agent) => {
    const frame = FEEDBACK[agent];
    const writes: string[] = [];
    let clock = 0;
    const outcome = await typeAnswerIntoTerminal(7, frame.denyFeedback, frame.feedback, {
      write: async (_session, data) => { writes.push(data); },
      // 第一次读屏还是按键前的画面（重绘没跟上），第二次才是进入等输入的画面。
      screen: async () => (clock < 300 ? frame.screen : frame.readyScreen),
      sleep: async (ms) => { clock += ms; },
      now: () => clock,
    });
    expect(outcome).toBe("sent");
    expect(writes).toEqual([frame.denyFeedback.openInput, frame.feedback, frame.denyFeedback.submitInput]);
  });

  it.each(AGENTS)("%s：屏幕一直没进入等输入（按键没生效）：只写了进入键，说明一个字都不写", async (agent) => {
    const frame = FEEDBACK[agent];
    const writes: string[] = [];
    let clock = 0;
    const outcome = await typeAnswerIntoTerminal(7, frame.denyFeedback, frame.feedback, {
      write: async (_session, data) => { writes.push(data); },
      screen: async () => frame.screen,
      sleep: async (ms) => { clock += ms; },
      now: () => clock,
    });
    expect(outcome).toBe("not_ready");
    expect(writes).toEqual([frame.denyFeedback.openInput]);
  });

  it("opencode 的真机屏幕上没有能带说明的拒绝项", () => {
    expect(terminalFeedbackFor(screenRequest("opencode").attention)).toBeNull();
    expect(FRAMES.opencode.denyFeedback).toBeUndefined();
  });
});
