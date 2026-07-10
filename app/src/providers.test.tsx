import { describe, it, expect } from "vitest";
import { agentAssets, tintStyle } from "./providers";

// 这里**不再**守护「前端 key 集合 == Rust 枚举」：agent 名单由后端 list_agents() 下发，前端无从
// 也不必守护它。本文件改守真正属于前端的那部分——视觉资产的兜底行为。

describe("agent 视觉资产", () => {
  it("已知 agent 各有图标", () => {
    for (const id of ["claude", "kimi", "codex"]) {
      expect(agentAssets(id).Icon, `缺少图标: ${id}`).toBeTruthy();
    }
  });

  it("未知 agent 走中性兜底，绝不伪装成 claude", () => {
    const unknown = agentAssets("__nope__");
    expect(unknown).toBeTruthy();
    // 关键回归：旧的 providerConfig 未知时回退 PROVIDERS.claude，于是一个本版本不认识的 agent
    // 会顶着 Claude 的赤陶徽标出现在卡片上。
    expect(unknown.Icon).not.toBe(agentAssets("claude").Icon);
    expect(unknown.needsTile).toBe(false);
    expect(unknown.tint).toBeUndefined();
  });

  it("只有 currentColor 徽标吃 tint；固定品牌色的不吃", () => {
    // claude 的 logomark 用 currentColor 绘制 → 由容器给品牌橙（主题明暗由 CSS 变量承担）。
    expect(tintStyle("claude")).toEqual({ color: "var(--cc-claude)" });
    // kimi(位图) / codex(自带黑底方块) 自带固定色，不设 color——否则会被容器染色。
    expect(tintStyle("kimi")).toEqual({});
    expect(tintStyle("codex")).toEqual({});
    // 未知 agent 同样不染色。
    expect(tintStyle("__nope__")).toEqual({});
  });

  it("断开态不给 tint，让位给 .stk-agent-off 的灰", () => {
    // inline style 优先级高于 class：断开时若仍设 color，徽标不会转灰。
    expect(tintStyle("claude", false)).toEqual({});
    expect(tintStyle("claude", true)).toEqual({ color: "var(--cc-claude)" });
  });

  it("设置页只有裸 logomark 需要品牌色底座", () => {
    expect(agentAssets("claude").needsTile).toBe(true);
    expect(agentAssets("kimi").needsTile).toBe(false);
    expect(agentAssets("codex").needsTile).toBe(false);
  });
});
