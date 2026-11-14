import { describe, expect, it } from "vitest";
import { descriptors } from "../../test/agents";
import { modeIconGlyph, modeIconName } from "./modeIcon";

const optionOf = (provider: string, id: string) => {
  const option = descriptors([provider]).find((agent) => agent.id === provider)?.launch_options.find((candidate) => candidate.id === id);
  if (!option) throw new Error(`${provider}.${id} 未声明`);
  return option;
};

// 第四轮 L4：手机工具栏的模式胶囊只剩图标，图标必须按当前档变化；同一栏里每档图标不同。
describe("模式胶囊图标按档变化", () => {
  const cases: [string, string, Record<string, string>][] = [
    ["codex", "approval", { default: "shield", readOnly: "shield-lock", ask: "shield-half", approveForMe: "shield-spark", yolo: "shield-warn" }],
    ["kimi", "permission", { manual: "shield-check", yolo: "shield-half", auto: "shield-warn" }],
    ["gemini", "approval", { default: "shield-check", autoEdit: "shield-pen", plan: "shield-plan", yolo: "shield-warn" }],
    ["opencode", "agent", { build: "agent", plan: "lightbulb-on" }],
    ["codex", "collaboration", { default: "lightbulb", plan: "lightbulb-on" }],
    ["kimi", "work", { default: "lightbulb", plan: "lightbulb-on" }],
  ];
  for (const [provider, id, expected] of cases) {
    it(`${provider}.${id}`, () => {
      const option = optionOf(provider, id);
      const names = option.choices.map((choice) => modeIconName(option, id, choice.id));
      expect(Object.fromEntries(option.choices.map((choice, index) => [choice.id, names[index]]))).toEqual(expected);
      // 同一栏里每档图标不同，图标都存在。
      expect(new Set(names).size).toBe(names.length);
      for (const name of names) expect(modeIconGlyph(name)).toBeTruthy();
    });
  }

  it("高风险档一律叹号盾牌（按钮另配危险色）", () => {
    for (const [provider, id] of [["codex", "approval"], ["kimi", "permission"], ["gemini", "approval"]] as const) {
      const option = optionOf(provider, id);
      for (const choice of option.choices.filter((candidate) => candidate.risk)) {
        expect(modeIconName(option, id, choice.id)).toBe("shield-warn");
      }
    }
  });

  it("OpenCode 自定义 agent 与未知档退回通用图标", () => {
    const agent = optionOf("opencode", "agent");
    expect(modeIconName(agent, "agent", "my-reviewer")).toBe("agent");
    expect(modeIconName(optionOf("codex", "approval"), "approval", undefined)).toBe("shield");
    expect(modeIconName(undefined, "permission", "whatever")).toBe("shield");
  });

  it("计划开着用实心灯泡（glyph 带 fill）", () => {
    expect(modeIconGlyph("lightbulb-on").fill).toBe(true);
    expect(modeIconGlyph("lightbulb").fill).toBeFalsy();
  });
});
