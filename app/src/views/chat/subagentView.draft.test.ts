import { describe, expect, it } from "vitest";
import { readSubagentScreen } from "./subagentView";
import { SUBAGENT_VIEW } from "../../test/agents";

// 做空第二轮 D：主会话里用户自己打了以「Message @」开头的草稿，纯文本屏幕上和子代理视图的
// 占位一模一样（屏幕只给字、不给颜色，占位的暗色认不出来）。面板画着、而且 main 行是「正在看」
// 的记号时，这条占位证据作废。

const COMPOSER = { input: "\u0013", composer_prompt: "❯", composer_border: "─", stashed_marker: "› stashed" };
const RULE = "─".repeat(118);

describe("readSubagentScreen：以 Message @ 开头的草稿", () => {
  it("面板显示正在看 main：不是子代理视图", () => {
    const draft = [RULE, "❯ Message @小王 帮我看下这个报错", RULE, "  ↑/↓ to select", "❯ ● main", "  ◯ general-purpose  后端实现 Excel 附件解析      13s"];
    expect(readSubagentScreen(draft, SUBAGENT_VIEW, COMPOSER).viewing).toBe(false);
  });

  it("真正的子代理视图照旧认得（面板里 main 不是正在看的记号）", () => {
    const viewing = [RULE, "❯ Message @general-purpose…", RULE, "  ↑/↓ to select", "  ◯ main", "❯ ● general-purpose  后端实现 Excel 附件解析      4s"];
    expect(readSubagentScreen(viewing, SUBAGENT_VIEW, COMPOSER).viewing).toBe(true);
  });
});
