// 往终端写正文之前收拾输入框（prepareComposer）：五家用本机真实 CLI 的画面回放。
//
// 假终端的每一帧都取自共享样例（test/fixtures/composer-frames.json，2026-11-14 probe_tui_script 实拍），
// 按键之后切到哪一帧照真机的反应写：codex / kimi / opencode 的 Ctrl-C 一下清空，gemini 的 Ctrl-K、Ctrl-U、
// 退格一轮清一行，claude 的 Ctrl-S 暂存后回到占位。断言到底：按了哪些键、草稿去了哪、最后画面是不是空的。
import { describe, expect, it } from "vitest";
import { chatUi } from "../../test/agents";
import composerFrames from "../../test/fixtures/composer-frames.json";
import type { TerminalScreenFrameDto } from "../../api";
import { prepareComposer, readComposerFrame, type ComposerSpec } from "./composer";
import { ComposerBlocked, composerBlockNotice, tidyComposer } from "./nativeDraft";
import { zh } from "../../i18n/zh";

type Sample = { name: string; lines: string[]; cursor?: { row: number; col: number; visible: boolean; dim: boolean } };
const sample = (name: string): TerminalScreenFrameDto => {
  const frame = (composerFrames.frames as Sample[]).find((entry) => entry.name === name);
  if (!frame) throw new Error(`没有这一帧：${name}`);
  return {
    lines: frame.lines,
    cursorRow: frame.cursor?.row ?? 0,
    cursorCol: frame.cursor?.col ?? 0,
    cursorVisible: frame.cursor?.visible ?? false,
    cursorDim: frame.cursor?.dim ?? false,
  };
};
const spec = (provider: string): ComposerSpec => chatUi(provider)!.composer!;

/// 回放实拍帧的假终端：`react(key, 当前帧)` 返回按键之后的帧（不变就返回 undefined）。
function terminal(start: TerminalScreenFrameDto, react: (key: string, now: TerminalScreenFrameDto) => TerminalScreenFrameDto | undefined) {
  let now = start;
  const keys: string[] = [];
  return {
    keys,
    current: () => now,
    io: {
      frame: async () => now,
      write: async (data: string) => { keys.push(data); now = react(data, now) ?? now; },
      sleep: async () => {},
    },
  };
}

const CTRL_C = "\x03";

describe("四家终端输入框里有草稿：清掉并把内容交给对话页", () => {
  const cases = [
    { provider: "codex", draft: "codex-0.167.2-same-text-draft", empty: "codex-0.167.2-placeholder-cursor", text: "Ask Codex to do anything" },
    { provider: "kimi", draft: "kimi-2.6.1-draft", empty: "kimi-2.6.1-cleared", text: "DRAFT-TWO" },
    { provider: "opencode", draft: "opencode-1.19.5-draft-multiline", empty: "opencode-1.19.5-placeholder-cursor", text: "DRAFT-ONE\nDRAFT-TWO" },
  ];
  it.each(cases)("$provider：按一次 Ctrl-C 清空，草稿原文带回", async ({ provider, draft, empty, text }) => {
    const pty = terminal(sample(draft), (key) => (key === CTRL_C ? sample(empty) : undefined));
    const prepared = await prepareComposer(pty.io, spec(provider), { running: false });
    expect(prepared).toEqual({ kind: "moved", draft: text });
    expect(pty.keys).toEqual([CTRL_C]);
    expect(readComposerFrame(pty.current(), spec(provider)).state).toBe("empty");
  });

  it.each(cases)("$provider：回合进行中也只清草稿（真机取证：不打断回合）", async ({ provider, draft, empty, text }) => {
    const pty = terminal(sample(draft), (key) => (key === CTRL_C ? sample(empty) : undefined));
    expect(await prepareComposer(pty.io, spec(provider), { running: true })).toEqual({ kind: "moved", draft: text });
    expect(pty.keys).toEqual([CTRL_C]);
  });

  it.each(cases)("$provider：输入框是空的（只有占位）时一个键都不按", async ({ provider, empty }) => {
    // 这些清空键在空输入框上是「退出」：认出是空的就绝不能按。
    const pty = terminal(sample(empty), () => undefined);
    expect(await prepareComposer(pty.io, spec(provider), { running: false })).toEqual({ kind: "clean", verified: true });
    expect(pty.keys).toEqual([]);
  });

  it("gemini：行编辑键逐行清，两行草稿两轮清完，全程不按 Ctrl-C（它会取消当前请求）", async () => {
    const two = sample("gemini-0.68-draft-multiline");
    // 真机：第一轮 Ctrl-K、Ctrl-U 之后只剩首行（` > AAA`），第二轮清空回到占位。
    const one = { ...two, lines: two.lines.filter((line) => !line.includes("DRAFT-TWO")), cursorRow: two.cursorRow - 1 };
    const empty = sample("gemini-0.68-placeholder-cursor");
    const pty = terminal(two, (key, now) => (key === "\x15" ? (now === two ? one : empty) : undefined));
    const prepared = await prepareComposer(pty.io, spec("gemini"), { running: true });
    expect(prepared).toEqual({ kind: "moved", draft: "DRAFT-ONE\nDRAFT-TWO" });
    expect(pty.keys).not.toContain(CTRL_C);
    expect(pty.keys.slice(0, 2)).toEqual(["\x0b", "\x15"]);
    expect(new Set(pty.keys)).toEqual(new Set(["\x0b", "\x15", "\x7f"]));
    expect(readComposerFrame(pty.current(), spec("gemini")).state).toBe("empty");
  });

  it("gemini：与占位同文的真实草稿也认得出并清掉（此前分不清，被当成空框把消息接在草稿后面发出去）", async () => {
    const pty = terminal(sample("gemini-0.68-same-text-draft"), (key) => (key === "\x15" ? sample("gemini-0.68-placeholder-cursor") : undefined));
    expect(await prepareComposer(pty.io, spec("gemini"), { running: false })).toEqual({ kind: "moved", draft: "Type your message or @path/to/file" });
  });

  it.each(cases)("$provider：按了清空键画面没变，如实说没收起，不继续写", async ({ provider, draft, text }) => {
    const pty = terminal(sample(draft), () => undefined);
    expect(await prepareComposer(pty.io, spec(provider), { running: false })).toEqual({ kind: "occupied", draft: text, reason: "failed" });
    // 只按了声明的那一下，没有连按（第二下在空框上是退出）。
    expect(pty.keys).toEqual([CTRL_C]);
  });
});

describe("claude：草稿用暂存键收起，提交后 CLI 自己还原", () => {
  it("有草稿按一次 Ctrl-S，空闲与回合进行中都一样", async () => {
    for (const running of [false, true]) {
      const pty = terminal(sample("claude-2.1.325-draft-multiline"), (key) => (key === "\x13" ? sample("claude-2.1.325-stashed") : undefined));
      expect(await prepareComposer(pty.io, spec("claude"), { running })).toEqual({ kind: "stashed" });
      expect(pty.keys).toEqual(["\x13"]);
    }
  });
  it("只有占位（或预测的下一句）时不按：暂存键是切换语义，空框上按会把旧暂存放出来", async () => {
    const pty = terminal(sample("claude-2.1.325-placeholder-cursor"), () => undefined);
    expect(await prepareComposer(pty.io, spec("claude"), { running: false })).toEqual({ kind: "clean", verified: true });
    expect(pty.keys).toEqual([]);
  });
  it("认不出输入框照发（另有提示识别与子代理视图兜着），但不算确认干净", async () => {
    const pty = terminal({ lines: ["  Select model", "❯ 1. Opus"], cursorRow: 1, cursorCol: 0, cursorVisible: false }, () => undefined);
    expect(await prepareComposer(pty.io, spec("claude"), { running: false })).toEqual({ kind: "clean", verified: false });
    expect(pty.keys).toEqual([]);
  });
});

describe("gemini 的特殊输入模式：先退出再写", () => {
  it("shell 模式按 Esc 回到普通输入框", async () => {
    const pty = terminal(sample("gemini-0.68-shell-mode"), (key) => (key === "\x1b" ? sample("gemini-0.68-placeholder-cursor") : undefined));
    expect(await prepareComposer(pty.io, spec("gemini"), { running: false })).toEqual({ kind: "clean", verified: true });
    expect(pty.keys).toEqual(["\x1b"]);
  });
  it("vim NORMAL 按 i 回到 INSERT，里面的草稿接着清掉带回", async () => {
    const insertDraft = { ...sample("gemini-0.68-vim-normal-draft"), cursorRow: 3, cursorCol: 8, cursorVisible: true };
    insertDraft.lines = insertDraft.lines.map((line) => line.replace("[NORMAL]", "[INSERT]"));
    const pty = terminal(sample("gemini-0.68-vim-normal-draft"), (key) => (key === "i" ? insertDraft
      : key === "\x15" ? sample("gemini-0.68-vim-insert-placeholder") : undefined));
    expect(await prepareComposer(pty.io, spec("gemini"), { running: false })).toEqual({ kind: "moved", draft: "hello" });
    expect(pty.keys[0]).toBe("i");
  });
  it("回合进行中不按退出键（Esc 在运行中是取消请求），说明要等回合结束", async () => {
    const pty = terminal(sample("gemini-0.68-shell-mode"), () => undefined);
    const prepared = await prepareComposer(pty.io, spec("gemini"), { running: true });
    expect(prepared).toEqual({ kind: "alt_mode", mode: "shell", running: true });
    expect(pty.keys).toEqual([]);
  });
  it("退不出去：不写正文，说明是哪种模式", async () => {
    const pty = terminal(sample("gemini-0.68-vim-normal-empty"), () => undefined);
    expect(await prepareComposer(pty.io, spec("gemini"), { running: false })).toEqual({ kind: "alt_mode", mode: "vim_normal", running: false });
    expect(pty.keys).toEqual(["i"]);
  });
});

describe("认不出输入框", () => {
  const menu = { lines: ["Select model", "● 1. gemini-2.5-pro", "  2. gemini-2.5-flash"], cursorRow: 1, cursorCol: 0, cursorVisible: false };
  it.each(["codex", "kimi", "gemini", "opencode"])("%s 不写、不按键；用户点了「仍然发送」才放行", async (provider) => {
    const pty = terminal(menu, () => undefined);
    expect(await prepareComposer(pty.io, spec(provider), { running: false })).toEqual({ kind: "unrecognized" });
    expect(await prepareComposer(pty.io, spec(provider), { running: false, force: true })).toEqual({ kind: "clean", verified: false });
    expect(pty.keys).toEqual([]);
  });
  it("读不到画面按认不出处理", async () => {
    const io = { frame: async () => null, write: async () => {}, sleep: async () => {} };
    expect(await prepareComposer(io, spec("kimi"), { running: false })).toEqual({ kind: "unrecognized" });
    const broken = { frame: async () => { throw new Error("offline"); }, write: async () => {}, sleep: async () => {} };
    expect(await prepareComposer(broken, spec("kimi"), { running: false })).toEqual({ kind: "unrecognized" });
  });
  it("插件没声明输入框规格（旧版本 CLI）：不读画面，照常发送", async () => {
    let reads = 0;
    const io = { frame: async () => { reads += 1; return null; }, write: async () => {}, sleep: async () => {} };
    expect(await prepareComposer(io, null, { running: false })).toEqual({ kind: "clean", verified: false });
    expect(reads).toBe(0);
  });
});

describe("界面这一侧：草稿搬运与错误条", () => {
  it("清出来的草稿交给调用方，空白草稿不交", async () => {
    const moved: string[] = [];
    const pty = terminal(sample("kimi-2.6.1-draft"), (key) => (key === CTRL_C ? sample("kimi-2.6.1-cleared") : undefined));
    await tidyComposer(pty.io, spec("kimi"), { running: false }, (draft) => moved.push(draft));
    expect(moved).toEqual(["DRAFT-TWO"]);
  });
  it("收不掉时抛 ComposerBlocked，错误条有重试与原因，认不出时按钮是「仍然发送」", async () => {
    const stuck = terminal(sample("kimi-2.6.1-draft"), () => undefined);
    const error = await tidyComposer(stuck.io, spec("kimi"), { running: false }, () => {}).catch((caught) => caught);
    expect(error).toBeInstanceOf(ComposerBlocked);
    expect(composerBlockNotice((error as ComposerBlocked).block, zh)).toEqual({ message: zh.chat.composerDraftStuck, retryLabel: zh.chat.errorRetry, force: false });
    expect(composerBlockNotice({ kind: "unrecognized" }, zh)).toEqual({ message: zh.chat.composerUnrecognized, retryLabel: zh.chat.composerSendAnyway, force: true });
    expect(composerBlockNotice({ kind: "alt_mode", mode: "shell", running: false }, zh).message).toBe(zh.chat.composerModeShell);
    expect(composerBlockNotice({ kind: "alt_mode", mode: "vim_normal", running: false }, zh).message).toBe(zh.chat.composerModeVim);
    expect(composerBlockNotice({ kind: "alt_mode", mode: "shell", running: true }, zh).message).toBe(zh.chat.composerModeRunning);
    expect(composerBlockNotice({ kind: "occupied", draft: "x", reason: "running" }, zh).message).toBe(zh.chat.composerDraftRunning);
  });
  // 文案约定（meowo-copy-style）：不超过 20 字、不用破折号和分号、不叫用户去终端收拾。
  it("新增文案守约定", () => {
    const keys = ["settingsTerminalNotReady", "nativeDraftMoved", "composerUnrecognized", "composerSendAnyway", "composerModeShell", "composerModeVim",
      "composerModeOther", "composerModeRunning", "composerDraftRunning", "composerDraftNoClear", "composerDraftStuck"] as const;
    for (const key of keys) {
      const text = zh.chat[key];
      expect([...text].length, key).toBeLessThanOrEqual(20);
      expect(text, key).not.toMatch(/[—；;]|请先在终端|去终端/);
    }
  });
});
