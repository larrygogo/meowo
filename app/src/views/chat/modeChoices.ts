/**
 * 模式的统一说法：对话框（composer 模式菜单）与新建会话页（桌面与手机）共用这一份。
 *
 * 各家模式原名不同、语义错位（Kimi `--yolo` 是「危险仍问」，Gemini `--yolo` 才是「都不问」），
 * 菜单每档按插件声明的三维（改文件 / 跑命令要不要问、是否只出方案）说，下一行是一句话说明，
 * 右边小字是 CLI 原名（任务 20261114-tui-model-switch，leader：「模式最好在交互界面能有一个统一的解释」）。
 */
import type { DynamicModeChoice, LaunchChoice, LaunchDefaults, LaunchOption, ModeTraits } from "../../api";
import type { Dict } from "../../i18n/zh";

export type ModeChoiceView = {
  value: string;
  /** 菜单里的名字：统一说法。 */
  label: string;
  /** 胶囊上的短名：CLI 原名（已有短名表的沿用）。 */
  short: string;
  /** 一句话说明。 */
  desc?: string;
  /** 小字：CLI 原名。 */
  detail?: string;
  risk: boolean;
};

/** 三维统一说法，如「改文件不问 · 跑命令危险时才问」「只出方案 · 不改文件 · 跑命令按权限档」。 */
export function traitsSummary(t: Dict, traits: ModeTraits): string {
  const parts = traits.edits === traits.commands
    ? [t.modes.both[traits.edits]]
    : [t.modes.edits[traits.edits], t.modes.commands[traits.commands]];
  if (traits.plan_only) parts.unshift(t.modes.planOnly);
  return parts.filter(Boolean).join(" · ");
}

/** 一栏启动选项的每档视图。`defaults` 是 CLI 配置里的默认档：没有统一说法的「默认」档按它来说。
 *  `dynamic` 是运行时才知道的档（OpenCode 自定义 agent），附在末尾。 */
export function modeChoiceViews(
  t: Dict,
  option: LaunchOption,
  defaults?: LaunchDefaults | null,
  dynamic: readonly DynamicModeChoice[] = [],
): ModeChoiceView[] {
  const resolved = (choice: LaunchChoice): LaunchChoice | undefined => {
    if (choice.traits) return choice;
    const id = defaults?.choices[option.id];
    return id && id !== choice.id ? option.choices.find((candidate) => candidate.id === id && candidate.traits) : undefined;
  };
  const views = option.choices.map((choice): ModeChoiceView => {
    const short = t.chat.modeShortNames[`${option.id}.${choice.id}`] ?? t.newSession.launchChoice[`${option.id}.${choice.id}`] ?? choice.label;
    const effective = resolved(choice);
    if (choice.traits) {
      return {
        value: choice.id,
        label: traitsSummary(t, choice.traits),
        short,
        desc: choice.desc ? t.modes.desc[choice.desc] : undefined,
        detail: choice.label,
        risk: choice.risk,
      };
    }
    // 「默认」档不传参数，行为由 CLI 配置决定：读得到配置就说成那一档，读不到如实说「按 CLI 配置」。
    // 读不到配置时，有说明的档直接用说明当名字（「按权限档执行」「按 Codex 配置文件的设置」）。
    const plain = !effective && choice.desc ? t.modes.desc[choice.desc] : undefined;
    if (plain) return { value: choice.id, label: plain, short, detail: choice.label, risk: choice.risk };
    return {
      value: choice.id,
      label: t.modes.followConfig(effective ? traitsSummary(t, effective.traits!) : t.modes.followConfigUnknown),
      short: effective ? t.modes.followConfig(effective.label) : short,
      desc: choice.desc ? t.modes.desc[choice.desc] : undefined,
      detail: choice.label,
      risk: choice.risk || !!effective?.risk,
    };
  });
  for (const extra of dynamic) {
    if (extra.option !== option.id || views.some((view) => view.value === extra.id)) continue;
    views.push({ value: extra.id, label: extra.label, short: extra.label, desc: t.modes.customAgent, risk: false });
  }
  return views;
}

/** 新建页显示哪一档：用户明确选过非默认档的用它；否则按 CLI 配置的默认档显示（不传参数时 CLI 本来
 *  就用它，显示与实际一致）；都没有退回插件声明的 default。 */
export function initialChoice(option: LaunchOption, stored: string | undefined, defaults?: LaunchDefaults | null): string {
  if (stored && stored !== option.default) return stored;
  const configured = defaults?.choices[option.id];
  if (configured && option.choices.some((choice) => choice.id === configured)) return configured;
  return option.default;
}

/** 屏幕上是否出现了任一原文（宽度窄时 CLI 会折行：逐行拼接后去掉换行再比，空白归一）。 */
export function screenShowsAny(lines: readonly string[], markers: readonly string[]): boolean {
  const squash = (text: string) => text.replace(/\s+/g, " ").trim();
  const joined = squash(lines.join(" "));
  return markers.some((marker) => marker && joined.includes(squash(marker)));
}
