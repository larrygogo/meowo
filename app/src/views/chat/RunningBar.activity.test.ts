// 运行条次行「此刻正在跑的那一步」的取法（liveToolActivity）。
import { describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import { zh } from "../../i18n/zh";
import { en } from "../../i18n/en";
import { liveToolActivity } from "./RunningBar";
import { toolShortName } from "./ToolIcon";

const user = (id: string): ChatItem => ({ type: "user_text", id, timestamp: null, text: "开始" }) as ChatItem;
const bash = (id: string, command: string, description: string | null = null): ChatItem =>
  ({ type: "tool_use", id, timestamp: null, name: "Bash", kind: "run", summary: command, detail: { kind: "command", command, description } }) as ChatItem;
const read = (id: string, path: string): ChatItem =>
  ({ type: "tool_use", id, timestamp: null, name: "Read", kind: "read", summary: path }) as ChatItem;
const done = (id: string, toolUseId: string): ChatItem =>
  ({ type: "tool_result", id, timestamp: null, tool_use_id: toolUseId, text: "", is_error: false }) as ChatItem;

describe("liveToolActivity", () => {
  it("取当前回合最后一条没回执的调用；全都跑完了给不出", () => {
    expect(liveToolActivity([user("u1"), bash("t1", "cargo build"), done("r1", "t1"), read("t2", "src/a.ts")], zh))
      .toEqual({ text: "读取: src/a.ts", tip: "读取: src/a.ts" });
    expect(liveToolActivity([user("u1"), bash("t1", "cd C:/repo && cargo test")], zh))
      .toEqual({ text: "› cargo test", tip: "› cd C:/repo && cargo test" });
    expect(liveToolActivity([user("u1"), bash("t1", "cargo build"), done("r1", "t1")], zh)).toBeNull();
  });

  it("上一回合里没回执的调用（被打断）不算在跑", () => {
    expect(liveToolActivity([user("u1"), bash("t1", "sleep 100"), user("u2")], zh)).toBeNull();
  });

  it("不露工具原名：发图按类别写人话，Meowo 的浏览器工具给译名（中英文）", () => {
    const send = { type: "tool_use", id: "s", timestamp: null, name: "mcp__meowo__send_image", kind: "image", summary: '{"source":"a.png"}' } as ChatItem;
    expect(liveToolActivity([user("u1"), send], zh)?.text).toBe("正在发图片");
    expect(liveToolActivity([user("u1"), send], en)?.text).toBe("Sending an image");
    const shot = { type: "tool_use", id: "b", timestamp: null, name: "mcp__meowo__browser_screenshot", kind: "other", summary: "" } as ChatItem;
    expect(liveToolActivity([user("u1"), shot], zh)?.text).toBe("网页截图");
    expect(liveToolActivity([user("u1"), shot], en)?.text).toBe("Screenshot");
    // Kimi 等不带 mcp__ 前缀的形态走同一张表。
    expect(toolShortName("browser_open", "other", zh)).toBe("打开网页");
    // 不认得的 MCP 工具照旧拆成「服务 · 工具」。
    expect(toolShortName("mcp__x__do", "other", zh)).toBe("x · do");
    expect(toolShortName("mcp__meowo__send_image", "image", zh)).toBe("发图片");
  });
});
