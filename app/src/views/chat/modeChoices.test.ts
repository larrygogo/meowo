import { describe, expect, it } from "vitest";
import { zh } from "../../i18n/zh";
import { descriptors } from "../../test/agents";
import { initialChoice, modeChoiceViews, planToggle, screenShowsAny, traitsSummary } from "./modeChoices";

const optionOf = (provider: string, id: string) => {
  const option = descriptors([provider]).find((agent) => agent.id === provider)?.launch_options.find((candidate) => candidate.id === id);
  if (!option) throw new Error(`${provider}.${id} 未声明`);
  return option;
};

describe("模式统一说法", () => {
  it("Kimi yolo 是「危险时才问」，Gemini yolo 才是「都不问」：同名档不同说法", () => {
    const kimi = modeChoiceViews(zh, optionOf("kimi", "permission"));
    const gemini = modeChoiceViews(zh, optionOf("gemini", "approval"));
    // 主文字是短名，第二行是三维统一说法。
    expect(kimi.find((view) => view.value === "yolo")).toMatchObject({ label: "必要时询问", desc: "常规自动做，危险的才问" });
    expect(gemini.find((view) => view.value === "yolo")).toMatchObject({ label: "都不问", desc: "改文件和跑命令都不问" });
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
          if (!choice.traits || (choice.desc && zh.modes.short[choice.desc])) continue;
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
    expect([short("kimi", "permission", "manual"), short("gemini", "approval", "default")]).toEqual(["每步都问", "每步都问"]);
    // Gemini plan 与 Kimi / Codex 的计划档同叫「计划」，第二行如实区分：Gemini 不能跑命令，Kimi 命令按权限档。
    expect(short("gemini", "approval", "plan")).toBe("计划");
    const geminiPlan = modeChoiceViews(zh, optionOf("gemini", "approval")).find((view) => view.value === "plan");
    const kimiPlan = modeChoiceViews(zh, optionOf("kimi", "work")).find((view) => view.value === "plan");
    expect(geminiPlan?.desc).toBe("只出方案 · 不改文件也不跑命令");
    expect(kimiPlan).toMatchObject({ short: "计划：开", desc: "只出方案 · 不改文件 · 命令按权限档" });
  });

  it("只规划的档把「只出方案」放在最前", () => {
    expect(traitsSummary(zh, { edits: "blocked", commands: "inherit", plan_only: true })).toBe("只出方案 · 不改文件 · 命令按权限档");
  });

  it("没有统一说法的「默认」档按 CLI 配置说：读得到配置就说成那一档", () => {
    const codex = optionOf("codex", "approval");
    const unknown = modeChoiceViews(zh, codex).find((view) => view.value === "default");
    expect(unknown).toMatchObject({ label: "按配置", desc: zh.modes.desc["codex.approval.default"] });
    const resolved = modeChoiceViews(zh, codex, { model: null, choices: { approval: "readOnly" } }).find((view) => view.value === "default");
    expect(resolved).toMatchObject({ label: zh.modes.followConfig("只读"), short: zh.modes.followConfig("只读"), desc: "改文件要问 · 命令危险才问" });
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
  });
});
