import { describe, expect, it } from "vitest";
import { zh } from "../../i18n/zh";
import { en } from "../../i18n/en";
import { descriptors } from "../../test/agents";
import { initialChoice, modeChoiceViews, modelChoiceRows, planToggle, screenShowsAny, traitsSummary } from "./modeChoices";

const optionOf = (provider: string, id: string) => {
  const option = descriptors([provider]).find((agent) => agent.id === provider)?.launch_options.find((candidate) => candidate.id === id);
  if (!option) throw new Error(`${provider}.${id} 未声明`);
  return option;
};

describe("模式统一说法", () => {
  it("Kimi yolo 是「危险时才问」，Gemini yolo 才是「都不问」：同名档不同说法", () => {
    const kimi = modeChoiceViews(zh, optionOf("kimi", "permission"));
    const gemini = modeChoiceViews(zh, optionOf("gemini", "approval"));
    // 主文字是三维查表的短名，第二行一律是各家官方原文提炼的那句（第三轮 C1）。
    expect(kimi.find((view) => view.value === "yolo")).toMatchObject({ label: "必要时询问", desc: zh.modes.desc["kimi.permission.yolo"] });
    expect(gemini.find((view) => view.value === "yolo")).toMatchObject({ label: "都不问", desc: zh.modes.desc["gemini.approval.yolo"] });
    // 小字是 CLI 原名，说明来自各家原文提炼。
    expect(kimi.find((view) => view.value === "yolo")?.detail).toBe("Ask When Needed");
    expect(kimi.find((view) => view.value === "auto")?.label).toBe("都不问");
  });

  it("胶囊短名说中文统一说法，CLI 原名只在小字；计划维度说开没开", () => {
    const codex = modeChoiceViews(zh, optionOf("codex", "approval"));
    expect(codex.find((view) => view.value === "ask")).toMatchObject({ short: "必要时询问", detail: "Ask for approval" });
    expect(codex.find((view) => view.value === "approveForMe")?.short).toBe("自动审查");
    expect(planToggle(optionOf("kimi", "work"))).toEqual({ on: "plan", off: "default" });
    expect(planToggle(optionOf("codex", "collaboration"))).toEqual({ on: "plan", off: "default" });
    // OpenCode 的 build / plan 各有自己的权限，不是开关；Gemini 的 plan 是审批四档之一。
    expect(planToggle(optionOf("opencode", "agent"))).toBeNull();
    expect(planToggle(optionOf("gemini", "approval"))).toBeNull();
    expect(modeChoiceViews(zh, optionOf("kimi", "work")).map((view) => view.short)).toEqual(["计划：关", "计划：开"]);
  });

  /// 截图复核：Codex「Ask for approval」与「Approve for me」主文字曾都是「常规自动做，危险的才问」，分不清。
  /// 同一家同一栏各档的短名必须互不重复；第二行说明 ≤20 字，菜单里不折出单字行。
  it("同一家各档短名唯一，说明不超过 20 字", () => {
    for (const agent of descriptors(["claude", "codex", "kimi", "gemini", "opencode"])) {
      for (const option of agent.launch_options) {
        const views = modeChoiceViews(zh, option, null, [{ option: option.id, id: "reviewer", label: "reviewer" }]);
        const shorts = views.map((view) => view.short);
        expect(new Set(shorts).size, `${agent.id}.${option.id}: ${shorts.join(" / ")}`).toBe(shorts.length);
        for (const view of views) {
          if (option.choices.some((choice) => choice.traits)) expect(view.label).toBe(view.short);
          expect([...(view.desc ?? "")].length, `${agent.id}.${option.id}.${view.value}: ${view.desc}`).toBeLessThanOrEqual(20);
        }
      }
    }
  });

  /// 「统一解释」：三维相同的档跨家同一个短名（Kimi Never Ask / Codex Full Access / Gemini yolo 都是「都不问」）。
  /// 例外只有同一栏里三维撞了、又必须分开的档（Codex 自动审查），它单独命名。
  it("三维相同的档跨家短名相同", () => {
    const byTraits = new Map<string, Set<string>>();
    for (const agent of descriptors(["claude", "codex", "kimi", "gemini", "opencode"])) {
      for (const option of agent.launch_options) {
        if (planToggle(option)) continue; // 计划开关说开没开，另有测试
        for (const choice of option.choices) {
          // 单独命名的（Codex 自动审查）与三维表达不出差别、走共用表单独一项的（Gemini default）另测。
          if (!choice.traits || (choice.desc && (zh.modes.short[choice.desc] || choice.desc === "gemini.approval.default"))) continue;
          const key = JSON.stringify(choice.traits);
          const short = modeChoiceViews(zh, option).find((view) => view.value === choice.id)!.short;
          byTraits.set(key, new Set([...(byTraits.get(key) ?? []), short]));
        }
      }
    }
    for (const [traits, shorts] of byTraits) expect([...shorts], traits).toHaveLength(1);
    const short = (provider: string, option: string, value: string) =>
      modeChoiceViews(zh, optionOf(provider, option)).find((view) => view.value === value)?.short;
    expect([short("kimi", "permission", "auto"), short("codex", "approval", "yolo"), short("gemini", "approval", "yolo")]).toEqual(["都不问", "都不问", "都不问"]);
    // 第四轮 M2：语义不同必须不同名。Kimi 每步都问（只自动读文件）≠ Gemini default（已知安全的命令不问）。
    expect([short("kimi", "permission", "manual"), short("gemini", "approval", "default")]).toEqual(["每步都问", "改动先问"]);
    expect(modeChoiceViews(en, optionOf("gemini", "approval")).find((view) => view.value === "default")?.short).toBe("Ask before changes");
    // 共用表里每个名字只代表一种语义：不同键不能同名（中英都查）。
    for (const dict of [zh, en]) {
      const names = Object.values(dict.modes.shared);
      expect(new Set(names).size, names.join("/")).toBe(names.length);
    }
    // Gemini plan 与 Kimi / Codex 的计划档同叫「计划」，第二行如实区分：Gemini 不能跑命令，Kimi 命令按权限档。
    expect(short("gemini", "approval", "plan")).toBe("计划");
    const geminiPlan = modeChoiceViews(zh, optionOf("gemini", "approval")).find((view) => view.value === "plan");
    const kimiPlan = modeChoiceViews(zh, optionOf("kimi", "work")).find((view) => view.value === "plan");
    expect(geminiPlan?.desc).toBe(zh.modes.desc["gemini.approval.plan"]);
    expect(kimiPlan).toMatchObject({ short: "计划：开", desc: zh.modes.desc["kimi.work.plan"] });
  });

  /// 做空 M1：Kimi 没有改回「每步都问」的参数；CLI 配置默认是别的档时这一档选不到，说明原因。
  it("配置默认档不是它时，无参数的明确档选不到", () => {
    const kimi = modeChoiceViews(zh, optionOf("kimi", "permission"), { model: null, choices: { permission: "yolo" } });
    expect(kimi.find((view) => view.value === "manual")).toMatchObject({ disabled: true, desc: zh.modes.configLocked("必要时询问") });
    expect(kimi.find((view) => view.value === "yolo")?.disabled).toBeUndefined();
    expect(modeChoiceViews(zh, optionOf("kimi", "permission"), { model: null, choices: { permission: "manual" } }).some((view) => view.disabled)).toBe(false);
    // Gemini「改动先问」显式带 --approval-mode default，配置默认是什么都选得到。
    expect(modeChoiceViews(zh, optionOf("gemini", "approval"), { model: null, choices: { approval: "autoEdit" } }).some((view) => view.disabled)).toBe(false);
  });

  it("只规划的档把「只出方案」放在最前", () => {
    // 说明里一律中文逗号分隔（第三轮 C8）。
    expect(traitsSummary(zh, { edits: "blocked", commands: "inherit", plan_only: true })).toBe("只出方案，不改文件，命令按权限档");
  });

  it("没有统一说法的「默认」档按 CLI 配置说：读得到配置就说成那一档", () => {
    const codex = optionOf("codex", "approval");
    const unknown = modeChoiceViews(zh, codex).find((view) => view.value === "default");
    expect(unknown).toMatchObject({ label: "按配置", desc: zh.modes.desc["codex.approval.default"] });
    // 新建页读得到配置：不列「默认（只读）」与「只读」两行，只留「只读」并标「默认」（第三轮 C7）。
    const resolved = modeChoiceViews(zh, codex, { model: null, choices: { approval: "readOnly" } });
    expect(resolved.some((view) => view.value === "default")).toBe(false);
    expect(resolved.find((view) => view.value === "readOnly")).toMatchObject({ label: "只读", tag: zh.modes.defaultTag });
    expect(resolved.filter((view) => view.tag)).toHaveLength(1);
  });

  /// 第三轮 C1：说明一律用官方原文那句；Codex「必要时询问」与「自动审查」三维相同，说明必须不同。
  it("说明取官方原文，三维相同的档说明也分得开", () => {
    const codex = modeChoiceViews(zh, optionOf("codex", "approval"));
    const ask = codex.find((view) => view.value === "ask")?.desc;
    const review = codex.find((view) => view.value === "approveForMe")?.desc;
    expect(ask).toBe(zh.modes.desc["codex.approval.ask"]);
    expect(review).toBe(zh.modes.desc["codex.approval.approveForMe"]);
    expect(ask).not.toBe(review);
    expect(modeChoiceViews(zh, optionOf("kimi", "permission")).find((view) => view.value === "auto")?.desc).toContain("计划仍要你审");
    expect(modeChoiceViews(zh, optionOf("kimi", "work")).find((view) => view.value === "default")?.desc).toBe("按权限档执行");
    expect(modeChoiceViews(zh, optionOf("codex", "collaboration")).find((view) => view.value === "default")?.desc).toBe("按权限档执行");
    // 中英成对：每个说明键英文也有。
    for (const key of Object.keys(zh.modes.desc)) expect(en.modes.desc[key], key).toBeTruthy();
  });

  /// 第三轮 C3/C7：桌面与手机新建页同一份模型行。配置写明的、或清单标 is_default 的默认模型只留一行
  /// （值为空串 = 不传 -m）并标「默认」；读不到才留「默认（按 CLI 配置）」。
  it("新建页模型清单：默认模型只留一行并标默认", () => {
    const catalog = [
      { id: "k3", label: "K3", group: null, uncertain: false, is_default: false },
      { id: "k31", label: "K3.1 Pro", group: null, uncertain: false, is_default: true },
    ] as never[];
    const byFlag = modelChoiceRows(zh, catalog, null);
    expect(byFlag.map((row) => [row.value, row.label, row.tag ?? null])).toEqual([["k3", "K3", null], ["", "K3.1 Pro", "默认"]]);
    const byConfig = modelChoiceRows(zh, catalog, { model: "k3", choices: {} });
    expect(byConfig.map((row) => [row.value, row.tag ?? null])).toEqual([["", "默认"], ["k31", null]]);
    const unknown = modelChoiceRows(zh, catalog.map((entry: { is_default: boolean }) => ({ ...entry, is_default: false })) as never[], null);
    expect(unknown[0]).toMatchObject({ value: "", label: zh.modes.followConfig(zh.modes.followConfigUnknown) });
  });

  /// 第四轮 M1：配置里的模型不在清单里时单独一行标默认（不传 -m）；清单自带的默认模型不标、可以带 -m 单独选。
  it("新建页模型清单：配置模型不在清单里时单列一行", () => {
    const catalog = [
      { id: "gpt-5.6", label: "GPT-5.6", group: null, uncertain: false, is_default: true },
      { id: "gpt-5.6-mini", label: "GPT-5.6 Mini", group: null, uncertain: false, is_default: false },
    ] as never[];
    const rows = modelChoiceRows(zh, catalog, { model: "my-proxy-model", choices: {} });
    expect(rows.map((row) => [row.value, row.label, row.tag ?? null])).toEqual([
      ["", "my-proxy-model", "默认"],
      ["gpt-5.6", "GPT-5.6", null],
      ["gpt-5.6-mini", "GPT-5.6 Mini", null],
    ]);
    expect(rows.filter((row) => row.tag).length).toBe(1);
  });

  it("OpenCode 自定义 agent 附在末尾，没有统一说法", () => {
    const views = modeChoiceViews(zh, optionOf("opencode", "agent"), null, [{ option: "agent", id: "reviewer", label: "reviewer" }]);
    expect(views.map((view) => view.value)).toEqual(["build", "plan", "reviewer"]);
    expect(views[2].desc).toBe(zh.modes.customAgent);
  });

  it("新建页预选：明确选过的非默认档 > CLI 配置默认 > 插件 default", () => {
    const gemini = optionOf("gemini", "approval");
    expect(initialChoice(gemini, undefined, { model: null, choices: { approval: "plan" } })).toBe("plan");
    expect(initialChoice(gemini, "default", { model: null, choices: { approval: "plan" } })).toBe("plan");
    expect(initialChoice(gemini, "yolo", { model: null, choices: { approval: "plan" } })).toBe("yolo");
    expect(initialChoice(gemini, undefined, null)).toBe("default");
  });

  /// Kimi 2.6 首条消息前的屏幕原文（research-kimi-gemini §2.6 e4 Q1）；窄终端会折行。
  it("认出等第一条消息的原文，折行也认", () => {
    const marker = "No session yet — one will be created on your first message.";
    expect(screenShowsAny(["  Session:", "  No session yet — one will be created on your first message."], [marker])).toBe(true);
    expect(screenShowsAny(["  No session yet — one will be created", "  on your first message."], [marker])).toBe(true);
    expect(screenShowsAny(["  Session: session_abc"], [marker])).toBe(false);
    // 行首锚定：对话正文里引用这句不算（做空 L1）。
    expect(screenShowsAny(["  › 屏幕上写着 No session yet — one will be created on your first message."], [marker])).toBe(false);
  });
});
