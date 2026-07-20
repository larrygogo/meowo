import { describe, expect, it } from "vitest";
import type { ChatItem } from "../api";
import { reduceChatEvents } from "./reducer";

const user = (id: string, text: string): ChatItem => ({
  type: "user_text",
  id,
  timestamp: null,
  text,
});

describe("reduceChatEvents", () => {
  it("merges assistant deltas across polling boundaries", () => {
    const first = reduceChatEvents([], [{
      type: "assistant_delta",
      id: "a1",
      timestamp: null,
      text: "正在",
    }], false);
    const second = reduceChatEvents(first, [{
      type: "assistant_delta",
      id: "a2",
      timestamp: null,
      text: "处理",
    }], false);
    expect(second).toEqual([{
      type: "assistant_text",
      id: "a1",
      timestamp: null,
      text: "正在处理",
    }]);
  });

  it("deduplicates adjacent equivalent semantic events and preserves the old reference", () => {
    const previous = [user("prompt", "继续")];
    const next = reduceChatEvents(previous, [user("append-message", "继续")], false);
    expect(next).toBe(previous);
  });

  it("does not deduplicate equal user text across another event", () => {
    const previous: ChatItem[] = [
      user("u1", "继续"),
      { type: "meta", id: "compact", timestamp: null, kind: "compacted" },
    ];
    expect(reduceChatEvents(previous, [user("u2", "继续")], false)).toHaveLength(3);
  });

  it("reset discards prior messages before reducing the new batch", () => {
    expect(reduceChatEvents([user("old", "旧")], [user("new", "新")], true))
      .toEqual([user("new", "新")]);
  });
});
