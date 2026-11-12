import { describe, expect, it } from "vitest";
import { zh } from "./zh";
import { en } from "./en";
import { formatBackendError } from "./errors";

// 第三轮验收 13（对话窗部分）：对话窗、输入框、审批 / 关卡卡、贴图、撤回、发送这些流程里，
// 用户向文案不许把人推去「终端检查 / 清理 / 处理」或「刷新」。手机远程页没有终端页，那些话
// 等于死路（用户 2026-11-01：「我在手机上远程，你让我去 agent 回答问题不是扯淡吗」；
// 2026-11-10：「不要让用户去终端去清除」）。能在 Meowo 里做的给按钮（SendErrorExits 的重试 /
// 看终端画面），做不到的写原因。
//
// 只扫 chat 命名空间：侧栏、面板、设置的同类文案归别的组。按钮名「去终端页」「在终端作答」
// 这类是 Meowo 自己的入口，不在禁止之列。禁的是「请你自己去终端做某事」的句式。

// 按钮名：点下去就是 Meowo 自己带人去终端页 / 交还终端表单，不是叫人自己去。
const BUTTON_LABELS = new Set(["chat.questionAnswerInTerminal", "chat.modelGoTerminal"]);

function walk(node: unknown, path: string, out: [string, string][]) {
  if (typeof node === "string") out.push([path, node]);
  else if (typeof node === "function") {
    try { walk((node as (...a: unknown[]) => unknown)("/model", 2, 3), path, out); } catch { /* 参数形状不符的跳过 */ }
  } else if (node && typeof node === "object") for (const [k, v] of Object.entries(node)) walk(v, path ? `${path}.${k}` : k, out);
}

const ZH_DEAD_END = /(?:请|可)?(?:先)?(?:到|去|切到|回到|在)终端(?:页)?(?:里)?(?:检查|查看|确认|清理|清空|处理|作答|切换|继续)|刷新后|请刷新|在桌面端|请先清空|清空后再/;
const EN_DEAD_END = /check the terminal|on the terminal tab|in the terminal (?:running|first)|refresh and|answer on the desktop|clear it(?: first|, then)/i;

describe("对话窗文案不把人推去终端或刷新（验收 13）", () => {
  it("中文 chat 文案", () => {
    const out: [string, string][] = [];
    walk(zh.chat, "chat", out);
    expect(out.filter(([path, text]) => !BUTTON_LABELS.has(path) && ZH_DEAD_END.test(text))).toEqual([]);
  });

  it("英文 chat 文案", () => {
    const out: [string, string][] = [];
    walk(en.chat, "chat", out);
    expect(out.filter(([path, text]) => !BUTTON_LABELS.has(path) && EN_DEAD_END.test(text))).toEqual([]);
  });

  it("关卡报错不叫人刷新：卡片自己重读（走查 C-11）", () => {
    for (const code of ["lead/gate_bad_choice", "lead/gate_changed"]) {
      expect(formatBackendError(code, "zh-CN")).not.toMatch(ZH_DEAD_END);
      expect(formatBackendError(code, "en")).not.toMatch(EN_DEAD_END);
    }
  });
});
