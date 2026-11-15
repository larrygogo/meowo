import { describe, expect, it } from "vitest";
import { terminalAttention } from "../../terminalAttention";
import { grammar } from "../../test/agents";
import { OPENCODE_ALWAYS_CONFIRM_SCREEN, OPENCODE_PERMISSION_SCREEN } from "../../test/requestSamples";
import { FOLLOW_UP_WINDOW_MS, followUpInput } from "./terminalFollowUp";

describe("持久放行的二次确认页", () => {
  const confirm = terminalAttention(OPENCODE_ALWAYS_CONFIRM_SCREEN, [], false, false, grammar("opencode"))!;
  const permission = terminalAttention(OPENCODE_PERMISSION_SCREEN, [], false, false, grammar("opencode"))!;
  const clicked = { session: 7, rule: "opencode:command-approval", at: 1_000 };

  it("刚在卡片上点了持久放行、认出确认页：替用户按确认（确认页第一项，回车）", () => {
    expect(followUpInput(clicked, confirm, 7, 1_800)).toBe("\r");
  });

  it("没点过（用户自己在终端里走到确认页）：不替他按，照常出卡", () => {
    expect(followUpInput(null, confirm, 7, 1_800)).toBeNull();
  });

  it("换了会话、过了时限、时钟倒退：都不按", () => {
    expect(followUpInput(clicked, confirm, 8, 1_800)).toBeNull();
    expect(followUpInput(clicked, confirm, 7, 1_000 + FOLLOW_UP_WINDOW_MS + 1)).toBeNull();
    expect(followUpInput(clicked, confirm, 7, 999)).toBeNull();
  });

  it("认出的不是确认页（还是权限框本身，或别的规则的确认页）：不按", () => {
    expect(followUpInput(clicked, permission, 7, 1_800)).toBeNull();
    expect(followUpInput({ ...clicked, rule: "kimi:command-approval" }, confirm, 7, 1_800)).toBeNull();
  });

  it("确认页的选项没认出来：不盲按", () => {
    expect(followUpInput(clicked, { confirms: "opencode:command-approval" }, 7, 1_800)).toBeNull();
    expect(followUpInput(clicked, { confirms: "opencode:command-approval", options: [] }, 7, 1_800)).toBeNull();
  });
});
