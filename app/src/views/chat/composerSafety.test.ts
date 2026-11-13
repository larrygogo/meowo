import { describe, expect, it } from "vitest";
import { composerCandidate, composerSafety, type ComposerGuard } from "./composerSafety";
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
const kimi: ComposerGuard = {
  prompt_footer: false, input_start_pattern: "^(\\s*│ > )",
  "top_pattern": "^\\s*╭─+╮\\s*$",
  "bottom_pattern": "^\\s*╰─+╯\\s*$",
  "row_pattern": "^\\s*│(.*)│\\s*$",
  "prompt_prefix": ">",
  "tail_rows": 0,
  "tail_pattern": null,
  "empty_pattern": "^$"
};
const opencode: ComposerGuard = {
  prompt_footer: false, input_start_pattern: "^(\\s*┃  )",
  "top_pattern": "^\\s*┃\\s*$",
  "bottom_pattern": "^\\s*╹▀+\\s*$",
  "row_pattern": "^\\s*┃(.*)$",
  "prompt_prefix": "",
  "tail_rows": 2,
  "tail_pattern": "^\\s*┃\\s+\\S.* · .+$",
  "empty_pattern": "^Ask anything…(?: \"[^\"\\r\\n]*\")?$"
};
describe("composerSafety 实拍边界", () => {
 it("Kimi 空框可写入", () => expect(composerSafety(captures.kimiEmpty, kimi)).toBe("empty"));
 it("Kimi 三行与中间光标保留原稿", () => expect(composerSafety(captures.kimiDraft, kimi)).toBe("occupied"));
 it("OpenCode 占位空框可写入", () => expect(composerSafety(captures.opencodeEmpty, opencode)).toBe("empty"));
 it("OpenCode 折叠粘贴不是空框", () => expect(composerSafety(captures.opencodePaste, opencode)).toBe("occupied"));
 it("OpenCode 不能越过正文去找最后空行", () => expect(composerSafety(captures.opencodeKeyboard, opencode)).toBe("occupied"));
 it("框体缺失或能力未声明时不写入", () => {
  expect(composerSafety(["ordinary output"], kimi)).toBe("unknown");
  expect(composerSafety(captures.kimiEmpty.slice(1), kimi)).toBe("unknown");
  expect(composerSafety(captures.opencodeEmpty.slice(0, -1), opencode)).toBe("unknown");
  expect(composerSafety(captures.kimiEmpty, null)).toBe("unknown");
 });
 it("尾行不匹配或规则损坏时不写入", () => {
  expect(composerSafety(captures.opencodeEmpty, {...opencode, tail_pattern:"never matches"})).toBe("unknown");
  expect(composerSafety(captures.kimiEmpty, {...kimi, row_pattern:"["})).toBe("unknown");
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
const codex: ComposerGuard = {
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
const gemini: ComposerGuard = {
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
  expect(composerCandidate(moreCaptures.codexEmpty,codex)).toMatchObject({state:"empty",inputRow:0,inputCol:2,isPlaceholder:true});
  expect(composerCandidate(moreCaptures.codexDraft,codex).state).toBe("occupied");
 });
 it("Gemini 输入起点在 > 后，不能移到额外缩进的灰字首字", () => {
  expect(composerCandidate(moreCaptures.geminiEmpty,gemini)).toMatchObject({state:"empty",inputRow:1,inputCol:3,isPlaceholder:true});
  expect(composerCandidate(moreCaptures.geminiDraft,gemini).state).toBe("occupied");
 });
 it("缺 prompt 或框内出现陌生形态不能跳到最后续行", () => {
  expect(composerCandidate(moreCaptures.codexDraft.slice(1),codex).state).toBe("unknown");
  const interrupted=[...moreCaptures.codexDraft];interrupted[1]="unrecognized overlay";
  expect(composerCandidate(interrupted,codex).state).toBe("unknown");
 });
 it("已识别正文框但没有可靠输入前缀也不可写", () => {
  expect(composerCandidate(moreCaptures.geminiEmpty,{...gemini,input_start_pattern:"never matches"}).state).toBe("unknown");
 });
 it("候选坐标以完整屏幕为准，不以裁图为准", () => {
  expect(composerCandidate(["history",...moreCaptures.geminiEmpty],gemini).inputRow).toBe(2);
 });
});

const codexWithShortcuts = ["› Ask Codex to do anything", "", "  GPT-6.1-Sol default · ~\\Desktop\\workspace\\meowo\\target\\provider-compat-lead\\target\\lead-provider-probes\\composer\\runs…", "  ← for agents · ? for shortcuts"];
it("Codex 最下方的快捷键提示不是模型 footer", () => expect(composerCandidate(codexWithShortcuts,codex).state).toBe("empty"));
