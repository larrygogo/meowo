import { describe, expect, it } from "vitest";
import { zh } from "../../i18n/zh";
import { descriptors } from "../../test/agents";
import { initialChoice, modeChoiceViews, screenShowsAny, traitsSummary } from "./modeChoices";

const optionOf = (provider: string, id: string) => {
  const option = descriptors([provider]).find((agent) => agent.id === provider)?.launch_options.find((candidate) => candidate.id === id);
  if (!option) throw new Error(`${provider}.${id} 未声明`);
  return option;
};

describe("模式统一说法", () => {
  it("Kimi yolo 是「危险时才问」，Gemini yolo 才是「都不问」：同名档不同说法", () => {
    const kimi = modeChoiceViews(zh, optionOf("kimi", "permission"));
    const gemini = modeChoiceViews(zh, optionOf("gemini", "approval"));
    expect(kimi.find((view) => view.value === "yolo")?.label).toBe("常规自动做，危险的才问");
    expect(gemini.find((view) => view.value === "yolo")?.label).toBe("改文件和跑命令都不问");
    // 小字是 CLI 原名，说明来自各家原文提炼。
    expect(kimi.find((view) => view.value === "yolo")?.detail).toBe("Ask When Needed");
    expect(kimi.find((view) => view.value === "auto")?.desc).toBe(zh.modes.desc["kimi.permission.auto"]);
  });

  it("只规划的档把「只出方案」放在最前", () => {
    expect(traitsSummary(zh, { edits: "blocked", commands: "inherit", plan_only: true })).toBe("只出方案 · 不改文件 · 命令按权限档");
  });

  it("没有统一说法的「默认」档按 CLI 配置说：读得到配置就说成那一档", () => {
    const codex = optionOf("codex", "approval");
    const unknown = modeChoiceViews(zh, codex).find((view) => view.value === "default");
    expect(unknown?.label).toBe(zh.modes.desc["codex.approval.default"]);
    const resolved = modeChoiceViews(zh, codex, { model: null, choices: { approval: "readOnly" } }).find((view) => view.value === "default");
    expect(resolved?.label).toBe(zh.modes.followConfig("改文件要问 · 命令危险才问"));
    expect(resolved?.short).toBe(zh.modes.followConfig("Read Only"));
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
