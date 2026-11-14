/**
 * 「此刻做不到」的统一策略（验收 7）。
 *
 * 模型菜单、模式菜单、思考档、计划开关、读模型清单、权限档，同一个入口此刻该是什么样，
 * 都由 `controlAvailability` 一个函数定，结果只有三种：
 *
 * - available：点了立刻做。
 * - queued：现在做不了，点了会记下来，稍后自动执行。入口照常可点，界面先说明什么时候执行。
 * - disabled：点了也做不成。入口置灰，原因里写明什么时候能用、或者该去哪里做。
 *
 * 输入只有两样：后端声明的能力（这次操作靠什么完成）与当前会话状态。不看 Provider 名字。
 *
 * 此前这件事由各个渲染点自己决定，同一种情况有四种处理（走查输入 B1、B8）：
 * 排队并提示、置灰、点了才报错、直接不列。Kimi 的计划开关在回合中被置灰，紧跟其后的排队分支
 * 因此走不到；「读取模型清单」和 Kimi 收紧权限则是列出来、点了才报错。
 *
 * 「排了队的切换在回合结束后真的执行」由 useSessionSettings 负责，这里只管入口此刻的状态。
 */
import type { ChatUi, LaunchChoice, LaunchOption, ModeControl, ModeOption } from "../../api";
import type { Dict } from "../../i18n/zh";

export type Availability =
  | { state: "available" }
  | { state: "queued"; note: string }
  | { state: "disabled"; reason: string };

/**
 * 这次操作靠什么完成。由能力声明推出（见下面几个 `*Mechanism` 函数），决定回合在跑时能不能做：
 *
 * - keystroke：一个原始按键（Claude 的 Shift+Tab 轮换权限）。CLI 在回合中照常响应，随时可做。
 * - command：往终端打一条带目标值的命令（`/model X`、`/effort high`、`/plan on`）。回合中打进去会
 *   混进正在跑的回合，但目标值已知，可以记下来等回合结束再打。
 * - service：经会话自带的服务改设置（Codex）。宿主在回合结束时执行。
 * - restart：带参数重启接回同一会话（Kimi、Gemini）。回合中重启会掐掉回合，排到回合之后。
 * - dialog：宿主替用户在 TUI 自己的对话框里选（OpenCode）。同上，排到回合之后。
 * - terminal_menu：要先让 CLI 弹出它自己的菜单再读屏（读模型清单、菜单型命令、不知道目标档的盲切）。
 *   没有目标值可记，回合中做不了，也排不了队。
 */
export type ControlMechanism = "keystroke" | "command" | "service" | "restart" | "dialog" | "terminal_menu";

export interface AvailabilityInput {
  /** 能力声明推出的机制；null = 后端没有声明这项能力。 */
  mechanism: ControlMechanism | null;
  /** 会话状态（sessionTone 的结果）。running = 回合在跑，pending = 有审批或提问在等你。 */
  tone: string | null;
  /** 目标档在这个会话里达不到，只能新建会话（接回只能放宽权限，见 `needsNewSession`）。 */
  needsNewSession?: boolean;
}

/** 回合在跑、或有审批 / 提问在等：此刻不能往终端打字，也不能重启。 */
export function sessionBusy(tone: string | null): boolean {
  return tone === "running" || tone === "pending";
}

/** 入口此刻的状态。规则按顺序判，先命中的算数。 */
export function controlAvailability(t: Dict, input: AvailabilityInput): Availability {
  // 1. 后端没有声明这项能力：做不了，原因照实说。调用方通常直接不出这个入口；
  //    已经列出来的档（如启动项里有、会话里切不了的）置灰。
  if (!input.mechanism) return { state: "disabled", reason: t.chat.availability.unsupported };
  // 2. 目标档只能在新会话里达到：和会话在不在跑无关，等也没用。
  if (input.needsNewSession) return { state: "disabled", reason: t.chat.availability.needsNewSession };
  // 3. 会话空闲：直接做。
  if (!sessionBusy(input.tone)) return { state: "available" };
  // 4. 回合在跑或有卡片在等。
  switch (input.mechanism) {
    case "keystroke":
      return { state: "available" };
    case "terminal_menu":
      return { state: "disabled", reason: input.tone === "pending" ? t.chat.availability.afterCard : t.chat.availability.afterTurn };
    default:
      return { state: "queued", note: input.tone === "pending" ? t.chat.availability.queuedAfterCard : t.chat.availability.queuedAfterTurn };
  }
}

// ── 能力声明 → 机制 ─────────────────────────────────────────────────────────────

/** 切模型：插件声明了会话里的切换机制就用它；否则有内联命令（`/model <id>`）打命令；
 *  只有菜单命令的要先弹 CLI 的菜单；都没有 = 这家在会话里切不了模型。 */
export function modelMechanism(chatUi: Pick<ChatUi, "model_switch" | "model_select_command" | "model_menu_command"> | null | undefined): ControlMechanism | null {
  if (!chatUi) return null;
  if (chatUi.model_switch === "server_settings") return "service";
  if (chatUi.model_switch === "restart_resume") return "restart";
  if (chatUi.model_switch === "tui_dialog") return "dialog";
  if (chatUi.model_select_command) return "command";
  if (chatUi.model_menu_command) return "terminal_menu";
  return null;
}

/** 读模型清单：让 CLI 弹一次它的模型菜单再读屏。没有菜单命令 = 没有这个入口。 */
export function modelListMechanism(chatUi: Pick<ChatUi, "model_menu_command"> | null | undefined): ControlMechanism | null {
  return chatUi?.model_menu_command ? "terminal_menu" : null;
}

/** 思考档：打命令（Claude、Kimi）或随模型经会话服务下发（Codex）；没声明 = 没有这个入口。 */
export function effortMechanism(chatUi: Pick<ChatUi, "effort_switch"> | null | undefined): ControlMechanism | null {
  if (chatUi?.effort_switch === "command") return "command";
  if (chatUi?.effort_switch === "server_settings") return "service";
  return null;
}

/**
 * 模式菜单里的一档（权限档、计划开关、协作模式、agent 都走这里）。
 * - `launchOption`：这一栏的启动选项声明（带会话里怎么切）；没有声明的栏只有 `control`。
 * - `control`：这一栏的直达输入 / 轮换键 / 菜单命令。
 * - `option`：菜单里的这一档；`inputs` 非空 = 有直达命令。
 */
export function modeMechanism(
  launchOption: Pick<LaunchOption, "switch"> | null | undefined,
  control: Pick<ModeControl, "cycle_input" | "menu_command"> | null | undefined,
  option: Pick<ModeOption, "inputs"> | null,
): ControlMechanism | null {
  if (launchOption?.switch === "server") return "service";
  if (launchOption?.switch === "restart") return "restart";
  if (launchOption?.switch === "dialog") return "dialog";
  if (option && option.inputs.length > 0) return "command";
  if (control?.cycle_input) return "keystroke";
  if (control?.menu_command) return "terminal_menu";
  return null;
}

/** 模式按钮本身（没有可列的档时）：按一次轮换，或打开 CLI 的菜单。 */
export function modeButtonMechanism(control: Pick<ModeControl, "cycle_input" | "menu_command"> | null | undefined): ControlMechanism | null {
  if (control?.menu_command) return "terminal_menu";
  if (control?.cycle_input) return "keystroke";
  return null;
}

/**
 * 目标档只能在新会话里达到：这一栏靠带参重启接回来切，而接回时 CLI 以会话记住的档为准、
 * 参数只能放宽不能收紧（插件声明 `resume_upgrade_only`）。目标档不带参数（Kimi 的「每步都问」）
 * 而会话现在不是这一档，就是收紧，重启也到不了。
 */
export function needsNewSession(
  launchOption: Pick<LaunchOption, "switch" | "resume_upgrade_only"> | null | undefined,
  choice: Pick<LaunchChoice, "id" | "args"> | null | undefined,
  currentValue: string | null | undefined,
): boolean {
  if (!launchOption || !choice) return false;
  return launchOption.resume_upgrade_only && launchOption.switch === "restart"
    && choice.args.length === 0 && currentValue !== choice.id;
}

// ── 思考档入口 ────────────────────────────────────────────────────────────────

export interface EffortEntry {
  /** 怎么切：command = 打 `/effort <档>`；service = 随当前模型经会话服务下发。 */
  via: "command" | "service";
  /** 当前模型可选的档位。 */
  levels: readonly string[];
  /** via = service 时随请求带上的当前模型（会话服务的设置接口按模型收 effort）。 */
  model: { id: string; label: string } | null;
}

/**
 * 思考档入口：这家能不能在会话里切、有哪些档。返回 null = 不出入口（没声明能力，或当前模型
 * 没有可选的档）。Codex 的思考档后端早已能经会话服务下发，前端此前只认「声明了 /effort 命令」，
 * 于是 Codex 会话里看不到也切不了（走查输入 B7）。
 */
export function effortEntry(
  chatUi: Pick<ChatUi, "effort_switch" | "effort"> | null | undefined,
  currentModel: { id: string; label: string; efforts: readonly string[] } | null | undefined,
): EffortEntry | null {
  if (chatUi?.effort_switch === "command") {
    // 插件给了静态档位表就用（Claude）；否则取当前模型声明的（Kimi 的档位随模型变）。
    const fixed = chatUi.effort?.levels ?? [];
    const levels = fixed.length > 0 ? fixed : currentModel?.efforts ?? [];
    return levels.length > 1 ? { via: "command", levels, model: null } : null;
  }
  if (chatUi?.effort_switch === "server_settings") {
    if (!currentModel || currentModel.efforts.length <= 1) return null;
    return { via: "service", levels: currentModel.efforts, model: { id: currentModel.id, label: currentModel.label } };
  }
  return null;
}

// ── 模式菜单 ──────────────────────────────────────────────────────────────────

/**
 * 模式菜单每一档此刻的状态，外加菜单顶上的一句排队说明（有档会排队时才有）。
 * `skip`：这份菜单不碰活会话（休眠态的恢复改选只记启动参数），每一档都按可用算。
 */
export function modeMenuAvailability(
  t: Dict,
  input: {
    launchOption: Pick<LaunchOption, "switch" | "resume_upgrade_only" | "choices"> | null | undefined;
    control: Pick<ModeControl, "cycle_input" | "menu_command"> | null | undefined;
    options: readonly { value: string | number; inputs: readonly unknown[] }[];
    /** 会话此刻在这一栏的档。 */
    currentValue: string | null;
    tone: string | null;
    skip?: boolean;
  },
): { items: Availability[]; queuedNote: string | null } {
  const items = input.options.map((option): Availability => {
    const value = String(option.value);
    // 已经是这一档：点了不动，不谈可不可用。
    if (input.skip || value === input.currentValue) return { state: "available" };
    const choice = input.launchOption?.choices.find((candidate) => candidate.id === value);
    return controlAvailability(t, {
      mechanism: modeMechanism(input.launchOption, input.control, { inputs: option.inputs as ModeOption["inputs"] }),
      tone: input.tone,
      needsNewSession: needsNewSession(input.launchOption, choice, input.currentValue),
    });
  });
  const queued = items.find((item) => item.state === "queued");
  return { items, queuedNote: queued?.state === "queued" ? queued.note : null };
}

// ── 切换的副作用 ──────────────────────────────────────────────────────────────

/**
 * 在对话页切模型 / 思考档会不会顺带改写用户 CLI 的默认配置，各家不同（Claude 的 `/model` 会存成
 * 新会话默认，Kimi 的 `/effort` 会改写 config.toml，其余通路不改）。按后端声明提醒一句；
 * 不改的返回 null。`shown` 说明菜单里此刻实际有哪种入口：没有的入口不提。
 */
export function savesDefaultHint(
  t: Dict,
  chatUi: Pick<ChatUi, "model_select_saves_default" | "effort_saves_default"> | null | undefined,
  shown: { model: boolean; effort: boolean },
): string | null {
  const model = !!chatUi?.model_select_saves_default && shown.model;
  const effort = !!chatUi?.effort_saves_default && shown.effort;
  if (model && effort) return t.chat.savesDefault.both;
  if (model) return t.chat.savesDefault.model;
  if (effort) return t.chat.savesDefault.effort;
  return null;
}

// ── 模型菜单 ──────────────────────────────────────────────────────────────────

/**
 * 模型菜单此刻的状态（展开成 ModelMenu 的三个属性）：
 * - queuedNote：选模型（或切思考档）会排到稍后自动执行，菜单里先说明。
 * - blocked：只能靠 CLI 自己的菜单选、此刻开不了，模型项置灰并写明原因。
 * - loadBlocked：此刻读不了模型清单，读取入口置灰并写明原因。
 */
export function modelMenuState(
  t: Dict,
  chatUi: Pick<ChatUi, "model_switch" | "model_select_command" | "model_menu_command" | "effort_switch"> | null | undefined,
  tone: string | null,
  hasEffort: boolean,
): { queuedNote: string | null; blocked: string | null; loadBlocked: string | null } {
  const pick = controlAvailability(t, { mechanism: modelMechanism(chatUi), tone });
  const list = controlAvailability(t, { mechanism: modelListMechanism(chatUi), tone });
  const effort = hasEffort ? controlAvailability(t, { mechanism: effortMechanism(chatUi), tone }) : null;
  return {
    queuedNote: pick.state === "queued" ? pick.note : effort?.state === "queued" ? effort.note : null,
    // 这家没有任何切模型的通路时，菜单本来就没有可选的模型项，不必再挂一句「不支持」。
    blocked: pick.state === "disabled" && modelMechanism(chatUi) !== null ? pick.reason : null,
    loadBlocked: list.state === "disabled" && modelListMechanism(chatUi) !== null ? list.reason : null,
  };
}

// ── 斜杠命令 ───────────────────────────────────────────────────────────────────

type SlashUi = Pick<ChatUi, "menu_slash_commands" | "model_select_command"> | null | undefined;

/**
 * 裸发一条斜杠命令靠什么完成：插件声明「裸发会弹出 CLI 自己的界面」的是 terminal_menu，
 * 其余是普通命令（null = 不归这套策略管，照常发）。清单只有后端总装的 `menu_slash_commands` 一处。
 */
export function slashCommandMechanism(chatUi: SlashUi, name: string): ControlMechanism | null {
  return (chatUi?.menu_slash_commands ?? []).includes(name) ? "terminal_menu" : null;
}

/**
 * 手敲整条命令回车（裸发）此刻的状态。回合在跑时菜单型命令做不了：打进去会混进正在跑的回合，
 * 菜单也读不到。原因与补全里置灰的那一句相同（走查输入 B1、B9：此前是另一句「再打开菜单」）。
 */
export function slashSendAvailability(t: Dict, chatUi: SlashUi, name: string, tone: string | null): Availability {
  const mechanism = slashCommandMechanism(chatUi, name);
  return mechanism ? controlAvailability(t, { mechanism, tone }) : { state: "available" };
}

/**
 * 补全列表里的一项此刻的状态。选中只是把命令写进输入框，但菜单型命令写进去也发不出，
 * 所以回合在跑时直接置灰并写明原因，不等用户点了、回车了才报错。
 * 例外：同名命令带参数时是内联执行（Claude 的 `/model sonnet`），选中后还能接着填参数，不置灰。
 */
export function slashEntryAvailability(t: Dict, chatUi: SlashUi, name: string, tone: string | null): Availability {
  if (chatUi?.model_select_command === name) return { state: "available" };
  return slashSendAvailability(t, chatUi, name, tone);
}
