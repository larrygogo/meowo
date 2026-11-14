import { describe, expect, it } from "vitest";
import { readComposer, type ComposerSpec } from "./composer";
import sharedFrames from "../../test/fixtures/composer-frames.json";

// 这里的规格只测认框，操作类字段（暂存、清空、特殊模式）取空值。
const OPS = { send_requires_recognition: true, stash: null, clear_inputs: [], clear_rounds: 1, clear_while_running: false, alt_modes: [] } satisfies Partial<ComposerSpec>;
const state = (screen: readonly string[] | null, spec: ComposerSpec | null | undefined) => readComposer(screen, spec).state;
// 2026-11-12 无模型 PTY 物理行实拍：target/lead-provider-probes/composer/runs/{kimi-24600,opencode-83332}。
// 仅裁输入框，保留实拍字符与空格，覆盖多行、中间光标与折叠粘贴。
const captures = {
  "kimiEmpty": [
    " ╭────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮",
    " │ >                                                                                                                  │",
    " ╰────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯"
  ],
  "kimiDraft": [
    " ╭────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮",
    " │ > DRAFT-FIRST                                                                                                      │",
    " │   DRAFT-SECOND                                                                                                     │",
    " │   DRAFT-THIRD                                                                                                      │",
    " ╰────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯"
  ],
  "opencodeEmpty": [
    "                       ┃                                                                                                ",
    "                       ┃  Ask anything… \"Fix a TODO in the codebase\"                                                    ",
    "                       ┃                                                                                                ",
    "                       ┃  Build · Claude Sonnet 4.6 GitHub Copilot                                                      ",
    "                       ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀                      "
  ],
  "opencodePaste": [
    "                       ┃                                                                                                ",
    "                       ┃  [Pasted ~3 lines]                                                                             ",
    "                       ┃                                                                                                ",
    "                       ┃  Build · Claude Sonnet 4.6 GitHub Copilot                                                      ",
    "                       ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀                      "
  ],
  "opencodeKeyboard": [
    "                       ┃                                                                                                ",
    "                       ┃  KEY-FIRST                                                                                     ",
    "                       ┃  KEY-SECXOND                                                                                   ",
    "                       ┃  KEY-THIRD                                                                                     ",
    "                       ┃                                                                                                ",
    "                       ┃  Build · Claude Sonnet 4.6 GitHub Copilot                                                      ",
    "                       ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀                      "
  ]
};
const kimi: ComposerSpec = { ...OPS,
  prompt_footer: false, input_start_pattern: "^(\\s*│ > )",
  "top_pattern": "^\\s*╭─+╮\\s*$",
  "bottom_pattern": "^\\s*╰─+╯\\s*$",
  "row_pattern": "^\\s*│(.*)│\\s*$",
  "prompt_prefix": ">",
  "tail_rows": 0,
  "tail_pattern": null,
  "empty_pattern": "^$"
};
const opencode: ComposerSpec = { ...OPS,
  prompt_footer: false, input_start_pattern: "^(\\s*┃  )",
  "top_pattern": "^\\s*┃\\s*$",
  "bottom_pattern": "^\\s*╹▀+\\s*$",
  "row_pattern": "^\\s*┃(.*)$",
  "prompt_prefix": "",
  "tail_rows": 2,
  "tail_pattern": "^\\s*┃\\s+\\S.* · .+$",
  "empty_pattern": "^Ask anything…(?: \"[^\"\\r\\n]*\")?$"
};
describe("读框的实拍边界", () => {
 it("Kimi 空框可写入", () => expect(state(captures.kimiEmpty, kimi)).toBe("empty"));
 it("Kimi 三行与中间光标保留原稿", () => expect(state(captures.kimiDraft, kimi)).toBe("occupied"));
 it("OpenCode 占位空框可写入", () => expect(state(captures.opencodeEmpty, opencode)).toBe("empty"));
 it("OpenCode 折叠粘贴不是空框", () => expect(state(captures.opencodePaste, opencode)).toBe("occupied"));
 it("OpenCode 不能越过正文去找最后空行", () => expect(state(captures.opencodeKeyboard, opencode)).toBe("occupied"));
 it("框体缺失或能力未声明时不写入", () => {
  expect(state(["ordinary output"], kimi)).toBe("unknown");
  expect(state(captures.kimiEmpty.slice(1), kimi)).toBe("unknown");
  expect(state(captures.opencodeEmpty.slice(0, -1), opencode)).toBe("unknown");
  expect(state(captures.kimiEmpty, null)).toBe("unknown");
 });
 it("尾行不匹配或规则损坏时不写入", () => {
  expect(state(captures.opencodeEmpty, {...opencode, tail_pattern:"never matches"})).toBe("unknown");
  expect(state(captures.kimiEmpty, {...kimi, row_pattern:"["})).toBe("unknown");
 });
});

// 根代理 2026-11-12 无模型实拍：codex-62104、gemini-64784、gemini-48516，只裁完整输入区。
const moreCaptures = {
  "codexEmpty": [
    "› Ask Codex to do anything",
    "",
    "  GPT-6.1-Sol default · ~\\Desktop\\workspace\\meowo\\target\\provider-compat-lead\\target\\lead-provider-probes\\composer\\runs…"
  ],
  "codexDraft": [
    "› DRAFT-FIRST",
    "  DRAFT-SECOND",
    "  DRAFT-THIRD",
    "",
    "  GPT-6.1-Sol default · ~\\Desktop\\workspace\\meowo\\target\\provider-compat-lead\\target\\lead-provider-probes\\composer\\runs…"
  ],
  "geminiEmpty": [
    "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
    " >   Type your message or @path/to/file",
    "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────"
  ],
  "geminiDraft": [
    "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
    " > DRAFT-FIRST",
    "   DRAFT-SECOND",
    "   DRAFT-THIRD",
    "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────"
  ]
};
const codex: ComposerSpec = { ...OPS,
  "prompt_footer": true,
  "input_start_pattern": "^(› )",
  "top_pattern": "^› ",
  "bottom_pattern": "^  [A-Za-z0-9][^\\r\\n]* · .+$",
  "row_pattern": "^(?:› |  )(.*)$",
  "prompt_prefix": "",
  "tail_rows": 0,
  "tail_pattern": null,
  "empty_pattern": "^(?:Ask Codex to do anything)?$"
};
const gemini: ComposerSpec = { ...OPS,
  "prompt_footer": false,
  "input_start_pattern": "^(\\s*> )",
  "top_pattern": "^─+$",
  "bottom_pattern": "^─+$",
  "row_pattern": "^ (.*)$",
  "prompt_prefix": ">",
  "tail_rows": 0,
  "tail_pattern": null,
  "empty_pattern": "^(?:Type your message or @path/to/file)?$"
};
describe("无边框与水平边框的光标候选", () => {
 it("Codex 无边框从 prompt 到最后 footer 保留整段", () => {
  expect(readComposer(moreCaptures.codexEmpty,codex)).toMatchObject({state:"empty",inputRow:0,inputCol:2,isPlaceholder:true});
  expect(readComposer(moreCaptures.codexDraft,codex).state).toBe("occupied");
 });
 it("Gemini 输入起点在 > 后，不能移到额外缩进的灰字首字", () => {
  expect(readComposer(moreCaptures.geminiEmpty,gemini)).toMatchObject({state:"empty",inputRow:1,inputCol:3,isPlaceholder:true});
  expect(readComposer(moreCaptures.geminiDraft,gemini).state).toBe("occupied");
 });
 it("缺 prompt 或框内出现陌生形态不能跳到最后续行", () => {
  expect(readComposer(moreCaptures.codexDraft.slice(1),codex).state).toBe("unknown");
  const interrupted=[...moreCaptures.codexDraft];interrupted[1]="unrecognized overlay";
  expect(readComposer(interrupted,codex).state).toBe("unknown");
 });
 it("已识别正文框但没有可靠输入前缀也不可写", () => {
  expect(readComposer(moreCaptures.geminiEmpty,{...gemini,input_start_pattern:"never matches"}).state).toBe("unknown");
 });
 it("候选坐标以完整屏幕为准，不以裁图为准", () => {
  expect(readComposer(["history",...moreCaptures.geminiEmpty],gemini).inputRow).toBe(2);
 });
});

const codexWithShortcuts = ["› Ask Codex to do anything", "", "  GPT-6.1-Sol default · ~\\Desktop\\workspace\\meowo\\target\\provider-compat-lead\\target\\lead-provider-probes\\composer\\runs…", "  ← for agents · ? for shortcuts"];
it("Codex 最下方的快捷键提示不是模型 footer", () => expect(readComposer(codexWithShortcuts,codex).state).toBe("empty"));

// 做空 N2：与宿主 meowo_agent::composer::read 读同一份实拍帧与规格（Rust 测试另核对规格与插件声明一致），
// 两边对每一帧必须给出同样的结论。Gemini 0.68 的输入框边框是 ▄/▀ 半块字符。
type SharedFrame = { name: string; provider: string; lines: string[]; expect: string; text?: string; cursor?: { row: number; col: number; visible: boolean; dim: boolean } };
describe("前后端输入框判定一致", () => {
  const specs = sharedFrames.specs as unknown as Record<string, ComposerSpec>;
  const verdict = (frame: SharedFrame, lines: string[]) => {
    const read = readComposer(lines, specs[frame.provider], frame.cursor ?? null);
    return { verdict: read.altMode ? `alt:${read.altMode.id}` : read.state, text: read.text };
  };
  for (const frame of sharedFrames.frames as SharedFrame[]) {
    it(frame.name, () => {
      for (const lines of [frame.lines, frame.lines.map((line) => line.trimEnd())]) {
        const got = verdict(frame, lines);
        expect(got.verdict).toBe(frame.expect);
        if (frame.text !== undefined) expect(got.text).toBe(frame.text);
      }
    });
  }
  it("五家都有实拍帧", () => {
    expect([...new Set(sharedFrames.frames.map((frame) => frame.provider))].sort()).toEqual(["claude", "codex", "gemini", "kimi", "opencode"]);
  });
});

// 占位与内容相同的真实草稿只差在光标（2026-11-14 五家真机取证）：这是此前「分不清，只好放行」的已知坑
// （.lead/20261112-lead-providers/defects.md G）。
describe("占位与同文草稿", () => {
  const specs = sharedFrames.specs as unknown as Record<string, ComposerSpec>;
  const frame = (name: string) => (sharedFrames.frames as SharedFrame[]).find((entry) => entry.name === name)!;
  it.each([
    ["codex", "codex-0.167.2-placeholder-cursor", "codex-0.167.2-same-text-draft"],
    ["gemini", "gemini-0.68-placeholder-cursor", "gemini-0.68-same-text-draft"],
  ])("%s 占位算空，同一句真实草稿算有字", (provider, placeholder, draft) => {
    const empty = frame(placeholder);
    const typed = frame(draft);
    expect(readComposer(empty.lines, specs[provider], empty.cursor)).toMatchObject({ state: "empty", isPlaceholder: true });
    expect(readComposer(typed.lines, specs[provider], typed.cursor)).toMatchObject({ state: "occupied", isPlaceholder: false });
  });
  it("没有光标证据时退回只看文字", () => {
    const typed = frame("codex-0.167.2-same-text-draft");
    expect(readComposer(typed.lines, specs.codex).state).toBe("empty");
    expect(readComposer(typed.lines, specs.codex, { ...typed.cursor!, visible: false }).state).toBe("empty");
  });
  it("claude 的占位内容不固定：光标在输入起点且该格暗色才算占位", () => {
    const base = frame("claude-2.1.325-placeholder-cursor");
    const lines = base.lines.map((line) => line.replace('Try "fix typecheck errors"', "run the tests again"));
    // 预测的下一句：文字对不上占位规则，靠暗色格认。
    expect(readComposer(lines, specs.claude, base.cursor).state).toBe("empty");
    // 同样的文字、光标也在行首，但不是暗色：用户打完字按了 Home。
    expect(readComposer(lines, specs.claude, { ...base.cursor!, dim: false }).state).toBe("occupied");
  });
});
