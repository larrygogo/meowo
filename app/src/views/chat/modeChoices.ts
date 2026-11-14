/**
 * 模式的统一说法：对话框（composer 模式菜单）与新建会话页（桌面与手机）共用这一份。
 *
 * 各家模式原名不同、语义错位（Kimi `--yolo` 是「危险仍问」，Gemini `--yolo` 才是「都不问」），
 * 菜单每档的名字按插件声明的三维（改文件 / 跑命令要不要问、是否只出方案）查共用短名，下一行一律是
 * 各家官方原文提炼的一句话（t.modes.desc，三维只管短名与分组，第三轮 C1），右边小字是 CLI 原名（任务 20261114-tui-model-switch，leader：「模式最好在交互界面能有一个统一的解释」）。
 */
import type { DynamicModeChoice, LaunchChoice, LaunchDefaults, LaunchOption, ModelCatalogEntry, ModeTraits } from "../../api";
import type { Dict } from "../../i18n/zh";

export type ModeChoiceView = {
  value: string;
  /** 菜单里的名字：与胶囊同一个短名。 */
  label: string;
  /** 胶囊上的短名：CLI 原名（已有短名表的沿用）。 */
  short: string;
  /** 第二行：官方原文提炼的一句话（t.modes.desc）；没有时退回三维统一说法。 */
  desc?: string;
  /** 名字旁的小字标签：新建页里 CLI 配置的默认档标「默认」（第三轮 C7）。 */
  tag?: string;
  /** 小字：CLI 原名。 */
  detail?: string;
  risk: boolean;
  /** 选不到：这一档没有启动参数、只靠 CLI 配置的默认档表达，而配置的默认档是别的（做空 M1）。 */
  disabled?: boolean;
};

/** 三维统一说法，如「改文件不问，命令危险才问」。说明里一律用中文逗号分隔（第三轮 C8）。 */
export function traitsSummary(t: Dict, traits: ModeTraits): string {
  const parts = traits.edits === traits.commands
    ? [t.modes.both[traits.edits]]
    : [t.modes.edits[traits.edits], t.modes.commands[traits.commands]];
  if (traits.plan_only) parts.unshift(t.modes.planOnly);
  return parts.filter(Boolean).join(t.modes.sep);
}

/** 共用短名：按三维查表，同一语义跨家同一个名字（「统一解释」，2026-11-14 截图复核）。
 *  三维相同、同一栏里又必须分开的档由 `t.modes.short[说明键]` 单独命名（Codex 自动审查）。
 *  查不到的组合返回 undefined，调用方退回 CLI 原名。 */
export function sharedShort(t: Dict, traits: ModeTraits, descKey?: string | null): string | undefined {
  const own = descKey ? t.modes.short[descKey] : undefined;
  if (own) return own;
  if (traits.plan_only) return t.modes.shared.plan;
  const key = `${traits.edits}/${traits.commands}`;
  const table: Record<string, string> = {
    "always/always": t.modes.shared.ask,
    "always/risky": t.modes.shared.readOnly,
    "never/always": t.modes.shared.autoEdits,
    "risky/risky": t.modes.shared.risky,
    "never/never": t.modes.shared.never,
  };
  return table[key];
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

/** 一栏启动选项的每档视图。`defaults` 是 CLI 配置里的默认档（只有新建页传）：没有统一说法的「默认」档
 *  读得到配置时不单列一行（不出现「默认（X）」与「X」两行），改在 X 那行标「默认」（第三轮 C7）。
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
  const official = (choice: LaunchChoice) => (choice.desc ? t.modes.desc[choice.desc] : undefined);
  const configuredId = defaults?.choices[option.id];
  const views = option.choices.flatMap((choice): ModeChoiceView[] => {
    // 胶囊短名：计划开关说开没开；其余用统一说法的短名（中文界面中文），CLI 原名只在菜单小字。
    const short = toggle ? t.modes.plan(choice.id === toggle.on)
      : (choice.traits ? sharedShort(t, choice.traits, choice.desc) : choice.desc ? t.modes.short[choice.desc] : undefined)
        ?? t.chat.modeShortNames[`${option.id}.${choice.id}`] ?? t.newSession.launchChoice[`${option.id}.${choice.id}`] ?? choice.label;
    const effective = resolved(choice);
    // 菜单主文字 = 胶囊短名（同一家各档互不重复，一眼分得清「越界才问」与「自动审查」）；
    // 第二行写三维统一说法（≤20 字，不折出单字行）；CLI 原名是行尾小字（2026-11-14 截图复核）。
    if (choice.traits) {
      // 不带参数的明确档（Kimi Always Ask：没有 --manual）只有在 CLI 配置的默认档就是它时才选得到；
      // 配置默认是别的档时，不传参数实际会用配置那一档，界面说「每步都问」就是假的。
      const configured = defaults?.choices[option.id];
      const configuredChoice = configured && configured !== choice.id && choice.args.length === 0
        ? option.choices.find((candidate) => candidate.id === configured && candidate.traits)
        : undefined;
      if (configuredChoice?.traits) {
        const name = sharedShort(t, configuredChoice.traits, configuredChoice.desc) ?? configuredChoice.label;
        return [{ value: choice.id, label: short, short, desc: t.modes.configLocked(name), detail: choice.label, risk: choice.risk, disabled: true }];
      }
      const tag = defaults && configuredId === choice.id && option.choices.some((other) => !other.traits) ? t.modes.defaultTag : undefined;
      return [{ value: choice.id, label: short, short, desc: official(choice) ?? traitsSummary(t, choice.traits), detail: choice.label, risk: choice.risk, ...(tag ? { tag } : {}) }];
    }
    // 「默认」档不传参数，行为由 CLI 配置决定：读得到配置就说成那一档；读不到用这档自己的说明
    // （「按权限档执行」「按 Codex 配置文件的设置」），连说明都没有才说「按 CLI 配置」。
    // 新建页读得到配置（defaults 只有新建页传）：这一行与配置那一档重复，省掉，由那一档标「默认」。
    if (effective?.traits) return [];
    return [{
      value: choice.id,
      label: short,
      short,
      desc: official(choice) ?? t.modes.followConfigUnknown,
      detail: choice.label,
      risk: choice.risk,
    }];
  });
  for (const extra of dynamic) {
    if (extra.option !== option.id || views.some((view) => view.value === extra.id)) continue;
    views.push({ value: extra.id, label: extra.label, short: extra.label, desc: t.modes.customAgent, risk: false });
  }
  return views;
}

/** 新建页的模型清单行（桌面 NewSessionPage 与手机 NewSessionPanel 共用，第三轮 C3/C7）。
 *  CLI 配置里的默认模型（配置写明的，或清单标 is_default 的）不另列「默认（X）」：X 那一行的值就是
 *  「不传 -m」（空串），名字旁标「默认」。读不到默认模型时才留一行「默认（按 CLI 配置）」。 */
export function modelChoiceRows(
  t: Dict,
  catalog: readonly ModelCatalogEntry[],
  defaults?: LaunchDefaults | null,
): { value: string; label: string; group?: string | null; detail?: string; tag?: string }[] {
  const configured = catalog.find((entry) => entry.id === defaults?.model) ?? catalog.find((entry) => entry.is_default);
  const rows = catalog.map((entry) => ({
    value: entry === configured ? "" : entry.id,
    label: entry.label,
    group: entry.group,
    detail: entry.uncertain ? t.modes.uncertain : undefined,
    ...(entry === configured ? { tag: t.modes.defaultTag } : {}),
  }));
  if (configured) return rows;
  return [{ value: "", label: t.modes.followConfig(defaults?.model ?? t.modes.followConfigUnknown), group: null, detail: undefined }, ...rows];
}

/** 新建页显示哪一档：用户明确选过非默认档的用它；否则按 CLI 配置的默认档显示（不传参数时 CLI 本来
 *  就用它，显示与实际一致）；都没有退回插件声明的 default。 */
export function initialChoice(option: LaunchOption, stored: string | undefined, defaults?: LaunchDefaults | null): string {
  // 存的是被省掉的「默认」行（见 modeChoiceViews）：显示成配置那一档。
  if (stored && stored !== option.default) return stored;
  const configured = defaults?.choices[option.id];
  if (configured && option.choices.some((choice) => choice.id === configured)) return configured;
  return option.default;
}

/** 屏幕上是否有一行**以**某段原文开头（行首锚定，对话正文里引用这句不算；memory「哨兵行首锚定」）。
 *  窄终端会折行：从这一行起把后续几行接上再比，空白归一。 */
export function screenShowsAny(lines: readonly string[], markers: readonly string[]): boolean {
  const squash = (text: string) => text.replace(/\s+/g, " ").trim();
  return lines.some((_, index) => {
    const from = squash(lines.slice(index, index + 4).join(" "));
    return markers.some((marker) => marker && from.startsWith(squash(marker)));
  });
}
