import { describe, expect, it } from "vitest";
import type { ChatItem } from "../../generated/contracts/ChatItem";
import { lastSteps } from "./RosterGlance";

const use = (id: string, name: string, summary: string): ChatItem => ({ type: "tool_use", id, timestamp: "2026-11-12T08:00:00Z", name, summary });
const result = (of: string, isError: boolean): ChatItem => ({ type: "tool_result", id: `r-${of}`, timestamp: null, tool_use_id: of, text: "", is_error: isError });
const say = (id: string): ChatItem => ({ type: "assistant_text", id, timestamp: null, text: "…" });
const label = (name: string) => name.toLowerCase();
const clock = () => "08:00";

describe("lastSteps", () => {
  it("只取工具调用，取最后几次，按时间先后排", () => {
    const items = [use("1", "Read", "a.ts"), say("s"), use("2", "Edit", "a.ts"), use("3", "Bash", "bun test"), use("4", "Grep", "foo")];
    expect(lastSteps(items, 3, label, clock).map((s) => s.id)).toEqual(["2", "3", "4"]);
  });

  it("报错的那一步标出来，别的不标", () => {
    const items = [use("1", "Bash", "bun test"), result("1", true), use("2", "Read", "a.ts"), result("2", false)];
    expect(lastSteps(items, 5, label, clock).map((s) => [s.id, s.failed])).toEqual([["1", true], ["2", false]]);
  });

  it("没有工具调用时是空的", () => {
    expect(lastSteps([say("s")], 5, label, clock)).toEqual([]);
  });

  it("动作名和时间交给调用方洗", () => {
    const [step] = lastSteps([use("1", "Bash", "ls")], 5, label, clock);
    expect(step).toMatchObject({ name: "bash", time: "08:00", summary: "ls" });
  });
});
