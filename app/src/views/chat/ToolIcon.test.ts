import { describe, expect, it } from "vitest";
import { zh } from "../../i18n/zh";
import { toolKind, toolShortName } from "./ToolIcon";

const name = (tool: string) => toolShortName(tool, toolKind(tool), zh as Parameters<typeof toolShortName>[2]);

describe("toolShortName", () => {
  it("claude 常用的其它工具给中文动作词，不再露英文原名", () => {
    expect(name("TaskStop")).toBe("停止任务");
    expect(name("Skill")).toBe("技能");
    expect(name("AskUserQuestion")).toBe("提问");
    expect(name("ScheduleWakeup")).toBe("定时唤醒");
  });

  it("MCP 工具显示成「服务 · 工具」", () => {
    expect(name("mcp__kimi-cu__click")).toBe("kimi-cu · click");
    expect(name("mcp__claude_ai_Claude_Docs__update")).toBe("claude_ai_Claude_Docs · update");
  });

  it("归类工具照旧用类别词，没收录的回落原名", () => {
    expect(name("Bash")).toBe("运行");
    expect(name("Agent")).toBe("子任务");
    expect(name("SomeNewTool")).toBe("SomeNewTool");
  });
});
