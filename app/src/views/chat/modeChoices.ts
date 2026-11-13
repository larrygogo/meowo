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
  /** 菜单里的名字：与胶囊同一个短名。 */
  label: string;
  /** 胶囊上的短名：CLI 原名（已有短名表的沿用）。 */
  short: string;
  /** 第二行：三维统一说法（改文件 / 跑命令要不要问、是否只出方案）。 */
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

/** 计划开关：两档里恰有一档「只出方案」，另一档不自带说法或「按权限档」（Kimi 工作模式、Codex 协作模式）。
 *  返回开 / 关两档的 id；OpenCode 的 build / plan 各有自己的权限，不算开关。 */
export function planToggle(option: LaunchOption): { on: string; off: string } | null {
  if (option.choices.length !== 2) return null;
  const on = option.choices.find((choice) => choice.traits?.plan_only);
  const off = option.choices.find((choice) => choice !== on);
  if (!on || !off) return null;
  const plain = !off.traits || (off.traits.edits === "inherit" && off.traits.commands === "inherit");
  return plain ? { on: on.id, off: off.id } : null;
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
  const toggle = planToggle(option);
  const views = option.choices.map((choice): ModeChoiceView => {
    // 胶囊短名：计划开关说开没开；其余用统一说法的短名（中文界面中文），CLI 原名只在菜单小字。
    const short = toggle ? t.modes.plan(choice.id === toggle.on)
      : (choice.desc ? t.modes.short[choice.desc] : undefined)
        ?? t.chat.modeShortNames[`${option.id}.${choice.id}`] ?? t.newSession.launchChoice[`${option.id}.${choice.id}`] ?? choice.label;
    const effective = resolved(choice);
    // 菜单主文字 = 胶囊短名（同一家各档互不重复，一眼分得清「越界才问」与「自动审查」）；
    // 第二行写三维统一说法（≤20 字，不折出单字行）；CLI 原名是行尾小字（2026-11-14 截图复核）。
    if (choice.traits) {
      return { value: choice.id, label: short, short, desc: traitsSummary(t, choice.traits), detail: choice.label, risk: choice.risk };
    }
    // 「默认」档不传参数，行为由 CLI 配置决定：读得到配置就说成那一档；读不到用这档自己的说明
    // （「按权限档执行」「按 Codex 配置文件的设置」），连说明都没有才说「按 CLI 配置」。
    if (effective?.traits) {
      const name = t.modes.followConfig((effective.desc ? t.modes.short[effective.desc] : undefined) ?? effective.label);
      return { value: choice.id, label: name, short: name, desc: traitsSummary(t, effective.traits), detail: choice.label, risk: choice.risk || effective.risk };
    }
    return {
      value: choice.id,
      label: short,
      short,
      desc: choice.desc ? t.modes.desc[choice.desc] : t.modes.followConfigUnknown,
      detail: choice.label,
      risk: choice.risk,
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
