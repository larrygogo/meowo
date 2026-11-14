// 终端草稿搬进对话页：保不住就不清（做空①的回归测试）。
//
// 做空复现过三件事：Codex 输入框里是大段粘贴的占位 `[Pasted Content 5120 chars]` 时，Meowo 清掉了终端
// 原稿，搬进对话页的却是占位文字本身；Kimi 一行长草稿被框宽折成两行，搬过来在折行处多了换行；只读
// 屏幕可见的行、每行还去掉首尾空白，超出可视高度的部分和行首缩进直接丢。
//
// 这里的每一帧都是本机真实 CLI 的画面（tests/probe_draft_fidelity.rs，2026-11-14，120×40）：五家各十来种
// 草稿形态，逐字打进或粘贴进输入框后抓下来，并进 test/fixtures/composer-frames.json（宿主的 Rust 测试读
// 同一份）。每一帧断言到底：要么搬过来的文本与帧里登记的原文逐字相等、终端被清空；要么判为读不全、
// 一个键也没按、终端画面原封不动。
import { describe, expect, it } from "vitest";
import { chatUi } from "../../test/agents";
import composerFrames from "../../test/fixtures/composer-frames.json";
import type { TerminalScreenFrameDto } from "../../api";
import { prepareComposer, readComposerFrame, type ComposerDraft, type ComposerSpec } from "./composer";
import { ComposerBlocked, composerBlockEvent, composerBlockExit, composerBlockNotice, tidyComposer } from "./nativeDraft";
import { zh } from "../../i18n/zh";
import { en } from "../../i18n/en";

type Sample = {
  name: string; provider: string; lines: string[]; expect: string; cols?: number; note?: string;
  cursor?: { row: number; col: number; visible: boolean; dim: boolean };
  draft?: ComposerDraft;
};
const samples = composerFrames.frames as Sample[];
const toFrame = (sample: Sample): TerminalScreenFrameDto => ({
  lines: sample.lines,
  cursorRow: sample.cursor?.row ?? 0,
  cursorCol: sample.cursor?.col ?? 0,
  cursorVisible: sample.cursor?.visible ?? false,
  cursorDim: sample.cursor?.dim ?? false,
  cols: sample.cols,
});
const named = (name: string): TerminalScreenFrameDto => {
  const sample = samples.find((entry) => entry.name === name);
  if (!sample) throw new Error(`没有这一帧：${name}`);
  return toFrame(sample);
};
const spec = (provider: string): ComposerSpec => chatUi(provider)!.composer!;

/// 回放实拍帧的假终端：按下这一家的清空键（最后一个）之后，画面换成它真机上清空后的那一帧。
const EMPTY: Record<string, string> = {
  codex: "codex-0.167.2-placeholder-cursor",
  kimi: "kimi-2.6.1-cleared",
  gemini: "gemini-0.68-placeholder-cursor",
  opencode: "opencode-1.19.5-placeholder-cursor",
  claude: "claude-2.1.325-stashed",
};
function terminal(provider: string, start: TerminalScreenFrameDto) {
  const declared = spec(provider);
  const emptied = declared.stash?.input ?? declared.clear_inputs[declared.clear_inputs.length - 1];
  let now = start;
  const keys: string[] = [];
  return {
    keys,
    current: () => now,
    io: {
      frame: async () => now,
      write: async (data: string) => { keys.push(data); if (data === emptied) now = named(EMPTY[provider]); },
      sleep: async () => {},
    },
  };
}

const forms = samples.filter((sample) => sample.name.includes("-form-"));
const PROVIDERS = ["claude", "codex", "gemini", "kimi", "opencode"];

describe("五家 × 草稿形态：原样搬，或者不动终端", () => {
  it("五家各有十种以上的形态", () => {
    for (const provider of PROVIDERS) {
      expect(forms.filter((sample) => sample.provider === provider).length, provider).toBeGreaterThanOrEqual(10);
    }
  });

  // 能暂存的（claude）不经过屏幕：草稿由 CLI 自己保管，提交后原样还原，什么形态都是按一次暂存键。
  const cleared = forms.filter((sample) => sample.provider !== "claude");
  const whole = cleared.filter((sample) => sample.draft?.complete);
  const partial = cleared.filter((sample) => sample.draft && !sample.draft.complete);

  it.each(whole.map((sample) => [sample.name, sample] as const))("%s：搬过来的文本与原文逐字相等，终端清空", async (_name, sample) => {
    const pty = terminal(sample.provider, toFrame(sample));
    const prepared = await prepareComposer(pty.io, spec(sample.provider), { running: false });
    expect(prepared).toEqual({ kind: "moved", draft: (sample.draft as { text: string }).text });
    expect(pty.keys.length).toBeGreaterThan(0);
    expect(readComposerFrame(pty.current(), spec(sample.provider)).state).toBe("empty");
  });

  it.each(partial.map((sample) => [sample.name, sample] as const))("%s：判为读不全，一个键也不按，终端草稿原样留着", async (_name, sample) => {
    const start = toFrame(sample);
    const pty = terminal(sample.provider, start);
    const prepared = await prepareComposer(pty.io, spec(sample.provider), { running: false });
    expect(prepared).toEqual({ kind: "incomplete", why: (sample.draft as { why: string }).why });
    expect(pty.keys).toEqual([]);
    expect(pty.current()).toBe(start);
  });

  it("每家都有被原样搬走的形态，也都有被判读不全的形态", () => {
    for (const provider of PROVIDERS.filter((id) => id !== "claude")) {
      expect(whole.some((sample) => sample.provider === provider), provider).toBe(true);
      for (const why of ["paste", "overflow", "wrap"]) {
        expect(partial.some((sample) => sample.provider === provider && sample.draft && !sample.draft.complete && sample.draft.why === why), `${provider} ${why}`).toBe(true);
      }
    }
  });

  const claude = forms.filter((sample) => sample.provider === "claude");
  it.each(claude.filter((sample) => sample.expect === "occupied").map((sample) => [sample.name, sample] as const))("%s：按一次暂存键，草稿留在 CLI 里", async (_name, sample) => {
    const pty = terminal("claude", toFrame(sample));
    expect(await prepareComposer(pty.io, spec("claude"), { running: false })).toEqual({ kind: "stashed" });
    expect(pty.keys).toEqual(["\x13"]);
  });
  it("claude 的草稿把上边框顶出屏幕：整框认不出，不按键", async () => {
    const tall = claude.find((sample) => sample.name.endsWith("-form-tall"))!;
    expect(tall.expect).toBe("unknown");
    const pty = terminal("claude", toFrame(tall));
    expect(await prepareComposer(pty.io, spec("claude"), { running: false })).toEqual({ kind: "clean", verified: false });
    expect(pty.keys).toEqual([]);
  });
  // 撤回前、切模型前清草稿不走暂存，用的是这份判定：claude 也得逐帧对。
  it.each(claude.filter((sample) => sample.draft).map((sample) => [sample.name, sample] as const))("%s：读草稿的判定", (_name, sample) => {
    expect(readComposerFrame(toFrame(sample), spec("claude")).draft).toEqual(sample.draft);
  });
});

describe("做空复现的两个现象", () => {
  it("codex：大段粘贴折成占位时不清终端原稿，更不把占位文字当草稿搬走", async () => {
    const start = named("codex-0.167.2-form-paste_big_line");
    expect(start.lines.some((line) => line.includes("[Pasted Content 5120 chars]"))).toBe(true);
    const pty = terminal("codex", start);
    const moved: string[] = [];
    const error = await tidyComposer(pty.io, spec("codex"), { running: false }, (draft) => moved.push(draft)).catch((caught) => caught);
    expect(error).toBeInstanceOf(ComposerBlocked);
    expect((error as ComposerBlocked).block).toEqual({ kind: "incomplete", why: "paste" });
    expect(pty.keys).toEqual([]);
    expect(moved).toEqual([]);
  });
  it("kimi：一行长草稿被框宽折成几行时不清，不在折行处添换行", async () => {
    const pty = terminal("kimi", named("kimi-2.6.1-form-wrap_words"));
    expect(await prepareComposer(pty.io, spec("kimi"), { running: false })).toEqual({ kind: "incomplete", why: "wrap" });
    expect(pty.keys).toEqual([]);
  });
  it("行首缩进与空行原样保留", async () => {
    for (const provider of ["codex", "kimi", "gemini", "opencode"]) {
      const sample = forms.find((entry) => entry.provider === provider && entry.name.endsWith("-form-multi"))!;
      const pty = terminal(provider, toFrame(sample));
      expect(await prepareComposer(pty.io, spec(provider), { running: false }), provider)
        .toEqual({ kind: "moved", draft: "line one\n    indented four\n\n  two spaces after blank\nlast line" });
    }
  });
});

describe("分不清折行与换行的边界", () => {
  const multi = () => samples.find((sample) => sample.name === "kimi-2.6.1-form-multi")!;
  const rewrite = (sample: Sample, from: string, to: string): TerminalScreenFrameDto => {
    const frame = toFrame(sample);
    // 框宽不变：把替换后的行按原长度补回空格。
    return { ...frame, lines: frame.lines.map((line) => {
      if (!line.includes(from)) return line;
      const border = line.lastIndexOf("│");
      const body = line.slice(0, border).replace(from, to).trimEnd();
      return body.padEnd(border, " ").slice(0, border) + line.slice(border);
    }) };
  };
  it("上一行还放得下下一行的第一个词：确认是换行", () => {
    expect(readComposerFrame(toFrame(multi()), spec("kimi")).draft).toMatchObject({ complete: true });
  });
  it("上一行写到了框边、下一个词放不下：可能是折行也可能是换行，判读不全", () => {
    // 内容区 110 格。下一行开头是 4 格缩进加 8 个字母的词，连同一个空格要 13 格：上一行写到 98 格就放不下了。
    const full = rewrite(multi(), "line one", "x".repeat(98));
    expect(readComposerFrame(full, spec("kimi")).draft).toEqual({ complete: false, why: "wrap" });
    // 少写一格就放得下，CLI 却另起了一行：只能是换行。
    const roomy = rewrite(multi(), "line one", "x".repeat(97));
    expect(readComposerFrame(roomy, spec("kimi")).draft).toMatchObject({ complete: true });
  });
  it("以终端右沿为界的家，帧里没有终端列数时多行草稿判读不全", () => {
    const sample = samples.find((entry) => entry.name === "codex-0.167.2-form-multi")!;
    expect(readComposerFrame(toFrame(sample), spec("codex")).draft).toMatchObject({ complete: true });
    expect(readComposerFrame({ ...toFrame(sample), cols: undefined }, spec("codex")).draft).toEqual({ complete: false, why: "unverified" });
  });
  it("没有声明判定依据的规格：任何草稿都不算读全", () => {
    const bare = { ...spec("kimi"), fidelity: null };
    expect(readComposerFrame(named("kimi-2.6.1-form-short"), bare).draft).toEqual({ complete: false, why: "unverified" });
  });
  it("两次读到的草稿不一样（用户还在终端里打字）：不清", async () => {
    const first = named("kimi-2.6.1-form-short");
    const second = named("kimi-2.6.1-form-wide");
    let reads = 0;
    const keys: string[] = [];
    const io = { frame: async () => (reads++ === 0 ? first : second), write: async (data: string) => { keys.push(data); }, sleep: async () => {} };
    expect(await prepareComposer(io, spec("kimi"), { running: false })).toEqual({ kind: "incomplete", why: "unverified" });
    expect(keys).toEqual([]);
  });
});

describe("读不全之后由人选择", () => {
  const paste = () => terminal("kimi", named("kimi-2.6.1-form-paste_big_line"));
  it("连同草稿一起发：不清，先写一个换行把草稿和正文隔开", async () => {
    const pty = paste();
    expect(await prepareComposer(pty.io, spec("kimi"), { running: false, draft: "keep", separator: "\n" })).toEqual({ kind: "clean", verified: false });
    expect(pty.keys).toEqual(["\n"]);
    expect(readComposerFrame(pty.current(), spec("kimi")).state).toBe("occupied");
  });
  it("放弃草稿后发：按清空键清掉，不搬", async () => {
    const pty = paste();
    const moved: string[] = [];
    expect(await tidyComposer(pty.io, spec("kimi"), { running: false, draft: "discard" }, (draft) => moved.push(draft))).toEqual({ kind: "clean", verified: true });
    expect(pty.keys).toEqual(["\x03"]);
    expect(moved).toEqual([]);
  });
  it("放弃没清掉：如实说没收起", async () => {
    const start = named("kimi-2.6.1-form-paste_big_line");
    const keys: string[] = [];
    const io = { frame: async () => start, write: async (data: string) => { keys.push(data); }, sleep: async () => {} };
    expect(await prepareComposer(io, spec("kimi"), { running: false, draft: "discard" })).toEqual({ kind: "occupied", draft: "", reason: "failed" });
  });
  it("输入框本来是空的：两种选择都不按键", async () => {
    for (const draft of ["keep", "discard"] as const) {
      const pty = terminal("kimi", named("kimi-2.6.1-cleared"));
      expect(await prepareComposer(pty.io, spec("kimi"), { running: false, draft, separator: "\n" })).toEqual({ kind: "clean", verified: true });
      expect(pty.keys).toEqual([]);
    }
  });

  it("错误条：说明原因，给两个选择和看终端画面的出口，放弃要再确认一次", () => {
    const chosen: unknown[] = [];
    const exit = composerBlockExit({ kind: "incomplete", why: "paste" }, zh, (next) => chosen.push(next));
    expect(composerBlockNotice({ kind: "incomplete", why: "paste" }, zh).message).toBe(zh.chat.composerDraftPaste);
    expect(composerBlockNotice({ kind: "incomplete", why: "overflow" }, zh).message).toBe(zh.chat.composerDraftOverflow);
    expect(composerBlockNotice({ kind: "incomplete", why: "wrap" }, zh).message).toBe(zh.chat.composerDraftWrapped);
    expect(composerBlockNotice({ kind: "incomplete", why: "unverified" }, zh).message).toBe(zh.chat.composerDraftUnsure);
    expect(exit.terminal).toBe(true);
    expect(exit.retry).toBeUndefined();
    expect(exit.actions?.map((action) => [action.label, action.confirmLabel])).toEqual([
      [zh.chat.composerSendWithDraft, undefined],
      [zh.chat.composerDiscardDraft, zh.chat.composerDiscardConfirm],
    ]);
    exit.actions?.forEach((action) => action.run());
    expect(chosen).toEqual([{ draft: "keep" }, { draft: "discard" }]);
  });
  it("其余拦下的情形照旧是一个重试钮", () => {
    const chosen: unknown[] = [];
    const exit = composerBlockExit({ kind: "unrecognized" }, zh, (next) => chosen.push(next));
    expect(exit).toMatchObject({ terminal: true, retryLabel: zh.chat.composerSendAnyway });
    expect(exit.actions).toBeUndefined();
    exit.retry?.();
    expect(chosen).toEqual([{ force: true }]);
  });

  it("被拦下的发送写进运行日志的内容：固定代码，没有草稿正文", () => {
    expect(composerBlockEvent({ kind: "incomplete", why: "overflow" })).toEqual({ kind: "send_blocked", reason: "composer_draft_incomplete", detail: { why: "overflow" } });
    expect(composerBlockEvent({ kind: "unrecognized" })).toEqual({ kind: "send_blocked", reason: "composer_unrecognized" });
    expect(composerBlockEvent({ kind: "alt_mode", mode: "shell", running: true })).toEqual({ kind: "send_blocked", reason: "composer_alt_mode", detail: { mode: "shell", running: true } });
    const kept = composerBlockEvent({ kind: "occupied", draft: "secret draft text", reason: "running" });
    expect(kept).toEqual({ kind: "send_blocked", reason: "composer_draft_kept", detail: { why: "running" } });
    expect(JSON.stringify(kept)).not.toContain("secret");
  });

  // 文案约定（meowo-copy-style）：不超过 20 字、不用破折号和分号、不叫用户去终端收拾；zh / en 同步。
  it("新增文案守约定", () => {
    const keys = ["composerDraftPaste", "composerDraftOverflow", "composerDraftWrapped", "composerDraftUnsure",
      "composerSendWithDraft", "composerDiscardDraft", "composerDiscardConfirm"] as const;
    for (const key of keys) {
      expect([...zh.chat[key]].length, key).toBeLessThanOrEqual(20);
      expect(zh.chat[key], key).not.toMatch(/[—；;]|请先在终端|去终端/);
      expect(en.chat[key], key).toBeTruthy();
      expect(en.chat[key], key).not.toMatch(/[—;]/);
    }
  });
});
