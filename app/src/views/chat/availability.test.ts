import { describe, expect, it } from "vitest";
import { chatUi, descriptors } from "../../test/agents";
import { zh } from "../../i18n/zh";
import { en } from "../../i18n/en";
import {
  controlAvailability, effortEntry, effortMechanism, modeButtonMechanism, modeMechanism, modeMenuAvailability,
  modelListMechanism, modelMechanism, modelMenuState, needsNewSession, savesDefaultHint, sessionBusy, type ControlMechanism,
} from "./availability";

const PROVIDERS = ["claude", "codex", "kimi", "gemini", "opencode"] as const;
const launchOptions = (provider: string) => descriptors([...PROVIDERS]).find((agent) => agent.id === provider)!.launch_options;
const option = (provider: string, id: string) => launchOptions(provider).find((candidate) => candidate.id === id)!;
const control = (provider: string, dimension: string) => chatUi(provider)!.mode_controls.find((candidate) => candidate.dimension === dimension) ?? null;

/// 验收 7：同一个函数决定入口是 可用 / 排队 / 置灰带原因。
describe("controlAvailability 的规则", () => {
  const MECHANISMS: ControlMechanism[] = ["keystroke", "command", "service", "restart", "dialog", "terminal_menu"];

  it("会话空闲：声明了能力的都可用", () => {
    for (const tone of ["waiting", "offline", "ended", "error", null]) {
      for (const mechanism of MECHANISMS) expect(controlAvailability(zh, { mechanism, tone })).toEqual({ state: "available" });
    }
  });

  it("回合在跑：按键照常可用，要开 CLI 菜单的置灰，其余排队", () => {
    expect(controlAvailability(zh, { mechanism: "keystroke", tone: "running" })).toEqual({ state: "available" });
    expect(controlAvailability(zh, { mechanism: "terminal_menu", tone: "running" })).toEqual({ state: "disabled", reason: zh.chat.availability.afterTurn });
    for (const mechanism of ["command", "service", "restart", "dialog"] as const) {
      expect(controlAvailability(zh, { mechanism, tone: "running" })).toEqual({ state: "queued", note: zh.chat.availability.queuedAfterTurn });
    }
  });

  it("有卡片在等：同样排队或置灰，说法换成处理完卡片后", () => {
    expect(controlAvailability(zh, { mechanism: "restart", tone: "pending" })).toEqual({ state: "queued", note: zh.chat.availability.queuedAfterCard });
    expect(controlAvailability(zh, { mechanism: "terminal_menu", tone: "pending" })).toEqual({ state: "disabled", reason: zh.chat.availability.afterCard });
  });

  it("目标档只能新建会话：不管会话在不在跑都置灰，原因写出路", () => {
    for (const tone of ["waiting", "running", "pending"]) {
      expect(controlAvailability(zh, { mechanism: "restart", tone, needsNewSession: true }))
        .toEqual({ state: "disabled", reason: zh.chat.availability.needsNewSession });
    }
  });

  it("没有声明能力：置灰并照实说", () => {
    expect(controlAvailability(zh, { mechanism: null, tone: "waiting" })).toEqual({ state: "disabled", reason: zh.chat.availability.unsupported });
  });

  it("排队说明与置灰原因都写明什么时候，中英文不超过规范长度、不用破折号和分号", () => {
    for (const text of Object.values(zh.chat.availability)) {
      expect(text.length, text).toBeLessThanOrEqual(20);
      expect(text).not.toMatch(/[—–；;]/);
    }
    for (const text of Object.values(en.chat.availability)) expect(text).not.toMatch(/[—–;]/);
    expect(zh.chat.availability.afterTurn).toContain("这一轮结束后");
    expect(zh.chat.availability.queuedAfterTurn).toContain("自动");
    expect(zh.chat.availability.needsNewSession).toContain("新建会话");
  });

  it("sessionBusy 只认 running 与 pending", () => {
    expect(["running", "pending", "waiting", "error", "offline", "ended", null].map(sessionBusy))
      .toEqual([true, true, false, false, false, false, false]);
  });
});

describe("能力声明 → 机制（五家）", () => {
  it("切模型", () => {
    expect(Object.fromEntries(PROVIDERS.map((provider) => [provider, modelMechanism(chatUi(provider))]))).toEqual({
      claude: "command", codex: "service", kimi: "restart", gemini: "restart", opencode: "dialog",
    });
    expect(modelMechanism(null)).toBeNull();
    expect(modelMechanism({ model_switch: null, model_select_command: null, model_menu_command: "/model" })).toBe("terminal_menu");
    expect(modelMechanism({ model_switch: null, model_select_command: null, model_menu_command: null })).toBeNull();
  });

  it("读模型清单：有菜单命令的才有这个入口，而且一律要开 CLI 菜单", () => {
    for (const provider of PROVIDERS) {
      expect(modelListMechanism(chatUi(provider)), provider).toBe(chatUi(provider)!.model_menu_command ? "terminal_menu" : null);
    }
  });

  it("思考档：Claude、Kimi 打命令，Codex 走会话服务，Gemini、OpenCode 没有", () => {
    expect(Object.fromEntries(PROVIDERS.map((provider) => [provider, effortMechanism(chatUi(provider))]))).toEqual({
      claude: "command", codex: "service", kimi: "command", gemini: null, opencode: null,
    });
  });

  it("模式栏：按插件声明的切换方式", () => {
    // Claude 权限：只有轮换键，菜单项是屏幕回显派生的（inputs 为空）。
    expect(modeMechanism(option("claude", "permission"), control("claude", "permission"), { inputs: [] })).toBe("keystroke");
    // Codex 权限与协作模式：会话服务。
    expect(modeMechanism(option("codex", "approval"), null, { inputs: [] })).toBe("service");
    expect(modeMechanism(option("codex", "collaboration"), null, { inputs: [] })).toBe("service");
    // Kimi 权限：带参重启；计划开关：直达命令。
    expect(modeMechanism(option("kimi", "permission"), null, { inputs: [] })).toBe("restart");
    expect(modeMechanism(option("kimi", "work"), control("kimi", "work"), { inputs: [{ data: "/plan on", submit: true }] })).toBe("command");
    // Gemini 审批：带参重启。OpenCode agent：TUI 对话框。
    expect(modeMechanism(option("gemini", "approval"), null, { inputs: [] })).toBe("restart");
    expect(modeMechanism(option("opencode", "agent"), control("opencode", "agent"), { inputs: [] })).toBe("dialog");
    // 什么都没声明。
    expect(modeMechanism(null, null, { inputs: [] })).toBeNull();
  });

  it("模式按钮本身：菜单命令要开 CLI 菜单，轮换键是按键", () => {
    expect(modeButtonMechanism({ cycle_input: "\u001b[Z", menu_command: null })).toBe("keystroke");
    expect(modeButtonMechanism({ cycle_input: null, menu_command: "/permissions" })).toBe("terminal_menu");
    expect(modeButtonMechanism(null)).toBeNull();
  });
});

describe("收紧权限（走查输入 B8）", () => {
  it("只有「带参重启、接回只能放宽」的栏里不带参数的档算收紧", () => {
    const kimi = option("kimi", "permission");
    const choice = (id: string) => kimi.choices.find((candidate) => candidate.id === id)!;
    expect(needsNewSession(kimi, choice("manual"), "yolo")).toBe(true);
    expect(needsNewSession(kimi, choice("manual"), null)).toBe(true);
    expect(needsNewSession(kimi, choice("manual"), "manual")).toBe(false);
    expect(needsNewSession(kimi, choice("auto"), "manual")).toBe(false);
    expect(needsNewSession(kimi, choice("yolo"), "auto")).toBe(false);
    // Gemini 也是带参重启，但每一档都带参数、接回可以收紧。
    const gemini = option("gemini", "approval");
    for (const candidate of gemini.choices) expect(needsNewSession(gemini, candidate, "yolo"), candidate.id).toBe(false);
    // Kimi 的计划开关声明了只能放宽，但它走直达命令不走重启。
    const work = option("kimi", "work");
    expect(needsNewSession(work, work.choices[0], "plan")).toBe(false);
    // 其余三家没有这个限制。
    for (const [provider, id] of [["claude", "permission"], ["codex", "approval"], ["opencode", "agent"]] as const) {
      for (const candidate of option(provider, id).choices) expect(needsNewSession(option(provider, id), candidate, "x"), `${provider}.${candidate.id}`).toBe(false);
    }
    expect(needsNewSession(null, null, "x")).toBe(false);
  });
});

describe("模式菜单每一档的状态（五家）", () => {
  const menu = (provider: string, id: string, tone: string, currentValue: string | null, inputsOf: (value: string) => unknown[] = () => []) => {
    const launchOption = option(provider, id);
    const options = launchOption.choices.map((choice) => ({ value: choice.id, inputs: inputsOf(choice.id) }));
    const result = modeMenuAvailability(zh, { launchOption, control: control(provider, id), options, currentValue, tone });
    return { byId: Object.fromEntries(options.map((entry, index) => [entry.value, result.items[index]])), note: result.queuedNote };
  };

  it("Kimi 计划开关：回合在跑时不置灰，点了排队，菜单顶上先说明（走查输入 B1）", () => {
    const running = menu("kimi", "work", "running", "default", (value) => [{ data: value === "plan" ? "/plan on" : "/plan off", submit: true }]);
    expect(running.byId.plan).toEqual({ state: "queued", note: zh.chat.availability.queuedAfterTurn });
    expect(running.note).toBe(zh.chat.availability.queuedAfterTurn);
    // 当前这一档不谈可不可用。
    expect(running.byId.default).toEqual({ state: "available" });
    const idle = menu("kimi", "work", "waiting", "default", () => [{ data: "/plan on", submit: true }]);
    expect(idle.byId.plan).toEqual({ state: "available" });
    expect(idle.note).toBeNull();
  });

  it("Kimi 权限：收紧的档置灰并写明要新建会话，放宽的档空闲可用、回合中排队", () => {
    const idle = menu("kimi", "permission", "waiting", "auto");
    expect(idle.byId.manual).toEqual({ state: "disabled", reason: zh.chat.availability.needsNewSession });
    expect(idle.byId.yolo).toEqual({ state: "available" });
    const running = menu("kimi", "permission", "running", "yolo");
    expect(running.byId.manual).toEqual({ state: "disabled", reason: zh.chat.availability.needsNewSession });
    expect(running.byId.auto).toEqual({ state: "queued", note: zh.chat.availability.queuedAfterTurn });
  });

  it("Codex、Gemini、OpenCode：回合在跑时每一档都排队，没有置灰", () => {
    for (const [provider, id, current] of [["codex", "approval", "ask"], ["codex", "collaboration", "default"], ["gemini", "approval", "default"], ["opencode", "agent", "build"]] as const) {
      const running = menu(provider, id, "running", current);
      for (const [value, availability] of Object.entries(running.byId)) {
        expect(availability.state, `${provider}.${id}.${value}`).toBe(value === current ? "available" : "queued");
      }
      expect(running.note, `${provider}.${id}`).toBe(zh.chat.availability.queuedAfterTurn);
    }
  });

  it("Claude 权限：回合在跑时照常可切，不出排队说明", () => {
    const running = menu("claude", "permission", "running", "default");
    for (const availability of Object.values(running.byId)) expect(availability).toEqual({ state: "available" });
    expect(running.note).toBeNull();
  });

  it("休眠态的恢复改选不碰活会话：每一档按可用算", () => {
    const launchOption = option("kimi", "permission");
    const result = modeMenuAvailability(zh, {
      launchOption, control: null, options: launchOption.choices.map((choice) => ({ value: choice.id, inputs: [] })),
      currentValue: "auto", tone: "running", skip: true,
    });
    expect(result.items.every((item) => item.state === "available")).toBe(true);
    expect(result.queuedNote).toBeNull();
  });
});

describe("思考档入口（走查输入 B7）", () => {
  const model = { id: "gpt-6.1-sol", label: "GPT-6.1-Sol", efforts: ["low", "medium", "high"] };

  it("Claude：插件的静态档位表，打命令", () => {
    expect(effortEntry(chatUi("claude"), null)).toEqual({ via: "command", levels: ["low", "medium", "high", "xhigh", "max"], model: null });
  });

  it("Kimi：档位随当前模型，模型没声明档位时不出入口", () => {
    expect(effortEntry(chatUi("kimi"), { id: "k3", label: "K3", efforts: ["low", "high", "max"] })).toEqual({ via: "command", levels: ["low", "high", "max"], model: null });
    expect(effortEntry(chatUi("kimi"), { id: "k3", label: "K3", efforts: [] })).toBeNull();
    expect(effortEntry(chatUi("kimi"), null)).toBeNull();
  });

  it("Codex：经会话服务随当前模型下发，对话窗可见可切", () => {
    expect(effortEntry(chatUi("codex"), model)).toEqual({ via: "service", levels: ["low", "medium", "high"], model: { id: "gpt-6.1-sol", label: "GPT-6.1-Sol" } });
    // 认不出当前模型、或它只有一档：没有可切的，不出入口。
    expect(effortEntry(chatUi("codex"), null)).toBeNull();
    expect(effortEntry(chatUi("codex"), { ...model, efforts: ["medium"] })).toBeNull();
  });

  it("Gemini、OpenCode：没有声明思考档能力，不出入口", () => {
    expect(effortEntry(chatUi("gemini"), model)).toBeNull();
    expect(effortEntry(chatUi("opencode"), model)).toBeNull();
    expect(effortEntry(null, model)).toBeNull();
  });
});

describe("切换会不会改 CLI 默认配置（走查输入 C1）", () => {
  it("按后端声明提醒：Claude 选模型、Kimi 切思考档，其余三家不提", () => {
    const both = { model: true, effort: true };
    expect(Object.fromEntries(PROVIDERS.map((provider) => [provider, savesDefaultHint(zh, chatUi(provider), both)]))).toEqual({
      claude: zh.chat.savesDefault.model,
      codex: null,
      kimi: zh.chat.savesDefault.effort,
      gemini: null,
      opencode: null,
    });
  });

  it("菜单里没有的入口不提；两样都会改时合成一句", () => {
    expect(savesDefaultHint(zh, chatUi("claude"), { model: false, effort: true })).toBeNull();
    expect(savesDefaultHint(zh, chatUi("kimi"), { model: true, effort: false })).toBeNull();
    expect(savesDefaultHint(en, { model_select_saves_default: true, effort_saves_default: true }, { model: true, effort: true })).toBe(en.chat.savesDefault.both);
    expect(savesDefaultHint(zh, null, { model: true, effort: true })).toBeNull();
    for (const text of Object.values(zh.chat.savesDefault)) expect(text.length, text).toBeLessThanOrEqual(20);
  });
});

describe("斜杠命令说明（走查输入 B11、C4）", () => {
  it("中英文说明表的命令一致", () => {
    expect(Object.keys(en.chat.slashDesc).sort()).toEqual(Object.keys(zh.chat.slashDesc).sort());
  });

  it("五家内置命令都有说明；侧问只在声明了能力的 Claude 的补全里", () => {
    for (const provider of PROVIDERS) {
      const ui = chatUi(provider)!;
      for (const command of ui.slash_commands.filter((entry) => entry.source === "builtin")) {
        expect(zh.chat.slashDesc[command.name], `${provider} ${command.name}`).toBeTruthy();
      }
      expect(ui.slash_commands.some((entry) => entry.name === "/btw"), provider).toBe(!!ui.side_question);
    }
  });

  it("撤回只有一个说法，不再叫撤销", () => {
    expect(zh.chat.slashDesc["/undo"]).toContain("撤回");
    expect(zh.chat.slashDesc["/undo"]).not.toContain("撤销");
  });
});

describe("模型菜单此刻的状态（五家）", () => {
  it("会话空闲：五家都没有排队说明，也没有置灰", () => {
    for (const provider of PROVIDERS) {
      expect(modelMenuState(zh, chatUi(provider), "waiting", true), provider).toEqual({ queuedNote: null, blocked: null, loadBlocked: null });
    }
  });

  it("回合在跑：选模型五家都排队并先说明；有清单读取入口的置灰带原因", () => {
    for (const provider of PROVIDERS) {
      const state = modelMenuState(zh, chatUi(provider), "running", false);
      expect(state.queuedNote, provider).toBe(zh.chat.availability.queuedAfterTurn);
      expect(state.blocked, provider).toBeNull();
      expect(state.loadBlocked, provider).toBe(chatUi(provider)!.model_menu_command ? zh.chat.availability.afterTurn : null);
    }
  });

  it("只能靠 CLI 菜单选模型的：回合在跑时模型项置灰带原因，不是点了才报错", () => {
    const menuOnly = { model_switch: null, model_select_command: null, model_menu_command: "/model", effort_switch: null };
    expect(modelMenuState(zh, menuOnly, "running", false)).toEqual({
      queuedNote: null, blocked: zh.chat.availability.afterTurn, loadBlocked: zh.chat.availability.afterTurn,
    });
    expect(modelMenuState(zh, menuOnly, "waiting", false)).toEqual({ queuedNote: null, blocked: null, loadBlocked: null });
  });

  it("没有任何切模型通路：不挂「不支持」，菜单里本来就没有可选项", () => {
    const none = { model_switch: null, model_select_command: null, model_menu_command: null, effort_switch: null };
    expect(modelMenuState(zh, none, "running", false)).toEqual({ queuedNote: null, blocked: null, loadBlocked: null });
    expect(modelMenuState(zh, null, "waiting", false)).toEqual({ queuedNote: null, blocked: null, loadBlocked: null });
  });
});
