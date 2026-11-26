import { describe, expect, it } from "vitest";
import { zh } from "../../i18n/zh";
import { toolGlyph, toolKindLabel, toolShortName, type ToolKind } from "./ToolIcon";
import { icGlobe, icTerminal } from "../../icons/glyphs";
import { icFile, icImage } from "../../icons/glyphs.chat";

const t = zh as Parameters<typeof toolShortName>[2];
const name = (tool: string, kind: ToolKind = "other") => toolShortName(tool, kind, t);

describe("toolShortName", () => {
  it("claude 常用的其它工具给中文动作词，不再露英文原名", () => {
    expect(name("TaskStop")).toBe("停止任务");
    expect(name("Skill")).toBe("技能");
    expect(name("AskUserQuestion")).toBe("提问");
    expect(name("ScheduleWakeup")).toBe("定时唤醒");
  });

  it("其余四家归不进类别的常用工具也有中文名", () => {
    expect(name("write_stdin")).toBe("终端输入");
    expect(name("wait_agent")).toBe("等待子任务");
    expect(name("ask_question")).toBe("提问");
    expect(name("generate_image")).toBe("生成图片");
    expect(name("NotifyUser")).toBe("通知");
    expect(name("list")).toBe("列出目录");
  });

  it("MCP 工具显示成「服务 · 工具」", () => {
    expect(name("mcp__kimi-cu__click")).toBe("kimi-cu · click");
    expect(name("mcp__claude_ai_Claude_Docs__update")).toBe("claude_ai_Claude_Docs · update");
  });

  it("归类工具用类别词，与工具叫什么无关；没收录的回落原名", () => {
    expect(name("Bash", "run")).toBe("运行");
    expect(name("run_shell_command", "run")).toBe("运行");
    expect(name("exec_command", "run")).toBe("运行");
    expect(name("replace", "edit")).toBe("编辑");
    expect(name("Agent", "agent")).toBe("子任务");
    expect(name("SomeNewTool")).toBe("SomeNewTool");
  });

  it("类别只认后端给的字段：名字里带 agent、search、shell 的工具不会被归错", () => {
    // 这三个都是此前按名字子串猜错的真实工具（走查 B1）。
    expect(name("wait_agent", "other")).not.toBe("子任务");
    expect(name("ToolSearch", "other")).toBe("ToolSearch");
    expect(name("KillShell", "other")).toBe("KillShell");
    expect(toolKindLabel("read_file", "read", t)).toBe("读取文件");
    expect(toolKindLabel("wait_agent", "other", t)).toBe("等待子任务");
  });
});

describe("toolGlyph", () => {
  it("Meowo 浏览器工具是网页图标、截图与发图是图片图标，不再顶着终端图标；五家名字形态同一个结果", () => {
    for (const tool of ["mcp__meowo__browser_open", "browser_open", "mcp__meowo__browser_click"]) expect(toolGlyph("other", tool, false)).toBe(icGlobe);
    for (const tool of ["mcp__meowo__browser_screenshot", "browser_screenshot"]) expect(toolGlyph("other", tool, true)).toBe(icImage);
  });

  it("没收录的工具回执里带图（别家的截图工具）用图片图标；不带图仍是终端", () => {
    expect(toolGlyph("other", "mcp__chrome__take_screenshot", true)).toBe(icImage);
    expect(toolGlyph("other", "mcp__chrome__click", false)).toBe(icTerminal);
  });

  it("有类别的先按类别：命令行回执带图仍是终端，读图片仍是文件", () => {
    expect(toolGlyph("run", "Bash", true)).toBe(icTerminal);
    expect(toolGlyph("read", "Read", true)).toBe(icFile);
    expect(toolGlyph("web", "WebFetch", false)).toBe(icGlobe);
  });
});
