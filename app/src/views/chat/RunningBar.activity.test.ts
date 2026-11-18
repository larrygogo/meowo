// 运行条次行「此刻正在跑的那一步」的取法（liveToolActivity）。
import { describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import { liveToolActivity } from "./RunningBar";

const user = (id: string): ChatItem => ({ type: "user_text", id, timestamp: null, text: "开始" }) as ChatItem;
const bash = (id: string, command: string, description: string | null = null): ChatItem =>
  ({ type: "tool_use", id, timestamp: null, name: "Bash", kind: "run", summary: command, detail: { kind: "command", command, description } }) as ChatItem;
const read = (id: string, path: string): ChatItem =>
  ({ type: "tool_use", id, timestamp: null, name: "Read", kind: "read", summary: path }) as ChatItem;
const done = (id: string, toolUseId: string): ChatItem =>
  ({ type: "tool_result", id, timestamp: null, tool_use_id: toolUseId, text: "", is_error: false }) as ChatItem;

describe("liveToolActivity", () => {
  it("取当前回合最后一条没回执的调用；全都跑完了给不出", () => {
    expect(liveToolActivity([user("u1"), bash("t1", "cargo build"), done("r1", "t1"), read("t2", "src/a.ts")]))
      .toEqual({ text: "Read: src/a.ts", tip: "Read: src/a.ts" });
    expect(liveToolActivity([user("u1"), bash("t1", "cd C:/repo && cargo test")]))
      .toEqual({ text: "› cargo test", tip: "› cd C:/repo && cargo test" });
    expect(liveToolActivity([user("u1"), bash("t1", "cargo build"), done("r1", "t1")])).toBeNull();
  });

  it("上一回合里没回执的调用（被打断）不算在跑", () => {
    expect(liveToolActivity([user("u1"), bash("t1", "sleep 100"), user("u2")])).toBeNull();
  });
});
