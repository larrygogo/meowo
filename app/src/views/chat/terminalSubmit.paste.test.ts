/// 单行正文什么时候也走 bracketed paste。
///
/// 起因是 gemini 0.68.0 真机（tests/probe_tui_script.rs，2026-11-14）：一次写进输入框的一串字里的半角问号
/// 被吞掉，同一句包成粘贴则原样。gemini 随 provider 移除后现役几家都不声明这条规则，写法本身留着：
/// 规则由插件声明、随对话页能力下发；不声明的单行照旧直接写。这里用合成规则 `\?` 测。
import { beforeEach, describe, expect, it, vi } from "vitest";

const writes: { sessionId: number; data: string }[] = [];
vi.mock("../../api", () => ({
  writeManagedTerminal: vi.fn((sessionId: number, data: string) => { writes.push({ sessionId, data }); return Promise.resolve(); }),
  managedTerminalSnapshot: vi.fn(() => Promise.resolve({ sessionId: 1, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null })),
}));

import { chatUi } from "../../test/agents";
import { setSingleLinePaste, submitToTerminal, terminalInput } from "./terminalSubmit";

const QUESTION = "What is 17+25? Reply with just the number.";
const pasted = (text: string) => `\x1b[200~${text}\x1b[201~`;

beforeEach(() => { writes.length = 0; });

describe("terminalInput：单行正文的写法", () => {
  it("没有声明规则：单行直接写，多行走粘贴（原有行为）", () => {
    expect(terminalInput(QUESTION)).toBe(QUESTION);
    expect(terminalInput("第一行\n第二行")).toBe(pasted("第一行\n第二行"));
  });

  it("声明了规则：命中的单行走粘贴，没命中的照旧直接写", () => {
    const rule = new RegExp(String.raw`\?`);
    expect(terminalInput(QUESTION, rule)).toBe(pasted(QUESTION));
    expect(terminalInput("a?b", rule)).toBe(pasted("a?b"));
    // 不带半角问号：不改写法。全角问号真机上不丢，同样不改。
    expect(terminalInput("把这个函数改成异步的", rule)).toBe("把这个函数改成异步的");
    expect(terminalInput("这样可以吗？", rule)).toBe("这样可以吗？");
    expect(terminalInput("/model", rule)).toBe("/model");
  });
});

describe("submitToTerminal：按会话登记的规则生效", () => {
  it.each(["claude", "codex", "kimi", "antigravity", "opencode"])("%s：带半角问号的单行消息", async (provider) => {
    const sessionId = 900 + ["claude", "codex", "kimi", "antigravity", "opencode"].indexOf(provider);
    setSingleLinePaste(sessionId, chatUi(provider)?.single_line_paste_pattern);
    await submitToTerminal(sessionId, QUESTION);
    const mine = writes.filter((write) => write.sessionId === sessionId).map((write) => write.data);
    // 正文一次、回车一次。现役几家都没声明规则，正文直接写。
    expect(mine).toEqual([QUESTION, "\r"]);

  });

  it("规则只管登记的那个会话；清掉登记后恢复直接写", async () => {
    setSingleLinePaste(910, String.raw`\?`);
    await submitToTerminal(911, QUESTION);
    expect(writes[0].data).toBe(QUESTION);
    writes.length = 0;
    setSingleLinePaste(910, null);
    await submitToTerminal(910, QUESTION);
    expect(writes[0].data).toBe(QUESTION);
  });

  it("坏掉的规则按没声明处理，不让发送停摆", async () => {
    setSingleLinePaste(912, "(");
    await submitToTerminal(912, QUESTION);
    expect(writes.map((write) => write.data)).toEqual([QUESTION, "\r"]);
  });
});
