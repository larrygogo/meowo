/**
 * 模式胶囊的图标按当前档变化（第四轮 L4）：手机 375 宽工具栏上模式胶囊只剩图标，不点开也要看得出
 * 现在是哪一档。
 *
 * - 计划开关（Kimi 工作模式、Codex 协作模式）：开 = 实心灯泡，关 = 空心灯泡。
 * - OpenCode agent：只出方案的 agent（plan）= 实心灯泡，其余 agent = agent 图标。
 * - 权限一类（Codex approval、Kimi permission、Gemini approval，以及 Claude permission）：同一个盾牌外形，
 *   里面的记号按插件声明的三维（改文件 / 跑命令要不要问、是否只出方案）区分；高风险档（启动选项声明了
 *   risk，如「都不问」）一律叹号盾牌，颜色由按钮的 is-risk 给危险色（--cc-err-text）。
 *   Codex「自动审查」与「必要时询问」三维相同（都是越界才问），差别在谁来审，单独用星号盾牌；Codex「只读」
 *   按沙箱单独用锁。改文件要问的其余档（每步都问、改动先问）用对勾。
 */
import type { Glyph } from "../../icons/Icon";
import {
  icAgents,
  icLightbulb,
  icLightbulbOn,
  icShield,
  icShieldCheck,
  icShieldHalf,
  icShieldLock,
  icShieldPen,
  icShieldPlan,
  icShieldSpark,
  icShieldWarn,
} from "../../icons/glyphs.chat";
import type { LaunchOption } from "../../api";
import { planToggle } from "./modeChoices";

/** 图标名（测试与截图说明用），与 glyph 一一对应。 */
export type ModeIconName =
  | "lightbulb" | "lightbulb-on" | "agent"
  | "shield" | "shield-check" | "shield-lock" | "shield-half" | "shield-pen" | "shield-spark" | "shield-plan" | "shield-warn";

const GLYPHS: Record<ModeIconName, Glyph> = {
  "lightbulb": icLightbulb,
  "lightbulb-on": icLightbulbOn,
  "agent": icAgents,
  "shield": icShield,
  "shield-check": icShieldCheck,
  "shield-lock": icShieldLock,
  "shield-half": icShieldHalf,
  "shield-pen": icShieldPen,
  "shield-spark": icShieldSpark,
  "shield-plan": icShieldPlan,
  "shield-warn": icShieldWarn,
};

/** 一栏模式在当前档 `value` 下的图标名。`option` 是插件声明的启动选项（没有时按维度给通用图标）。 */
export function modeIconName(option: LaunchOption | undefined, dimension: string, value: string | undefined): ModeIconName {
  const toggle = option ? planToggle(option) : null;
  if (toggle) return value === toggle.on ? "lightbulb-on" : "lightbulb";
  const choice = option?.choices.find((candidate) => candidate.id === value);
  if (dimension === "agent") return choice?.traits?.plan_only ? "lightbulb-on" : "agent";
  if (!choice) return "shield";
  if (choice.risk) return "shield-warn";
  if (choice.desc?.endsWith(".approveForMe")) return "shield-spark";
  const traits = choice.traits;
  if (!traits || (traits.edits === "inherit" && traits.commands === "inherit")) return "shield";
  if (traits.plan_only) return "shield-plan";
  if (traits.edits === "never" && traits.commands === "never") return "shield-warn";
  // 只读按沙箱认（Codex readOnly）：三维上它和「改动先问」一样是改文件要问，但改不了才是它的本意。
  if (traits.edits === "blocked" || choice.desc?.endsWith(".readOnly")) return "shield-lock";
  if (traits.edits === "always") return "shield-check";
  if (traits.edits === "never") return "shield-pen";
  return "shield-half";
}

export function modeIconGlyph(name: ModeIconName): Glyph {
  return GLYPHS[name];
}
