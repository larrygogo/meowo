import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import { FOLD_SESSION_LIMIT, foldScope, resetFoldMemory } from "./foldMemory";
import { Transcript } from "./Transcript";

afterEach(() => {
  cleanup();
  resetFoldMemory();
});

const thinking = (id: string): ChatItem[] => [{ type: "reasoning", id, timestamp: null, text: `想法 ${id}` }];

function openReasoning() {
  const details = document.querySelector("details.chat-reasoning") as HTMLDetailsElement;
  details.open = true;
  fireEvent(details, new Event("toggle"));
}

const reasoningOpen = () => (document.querySelector("details.chat-reasoning") as HTMLDetailsElement).open;

describe("foldMemory", () => {
  /// 对照 Kimi 的 history-state：切到别的会话看一眼再回来，刚展开的块原样还在。
  it("切走再回来，展开状态按会话还原", () => {
    const { rerender } = render(<Transcript sessionId={1} items={thinking("r1")} />);
    openReasoning();
    rerender(<Transcript sessionId={2} items={thinking("r2")} />);
    expect(reasoningOpen()).toBe(false);
    rerender(<Transcript sessionId={1} items={thinking("r1")} />);
    expect(reasoningOpen()).toBe(true);
  });

  it(`只记最近 ${FOLD_SESSION_LIMIT} 个会话，更早的淘汰`, () => {
    foldScope(1).set("fold:x", true);
    for (let id = 2; id <= FOLD_SESSION_LIMIT; id += 1) foldScope(id);
    // 再碰一次 1：它变成最近使用，下一个新会话挤掉的是 2。
    expect(foldScope(1).get("fold:x")).toBe(true);
    foldScope(FOLD_SESSION_LIMIT + 1);
    expect(foldScope(1).get("fold:x")).toBe(true);
    foldScope(2).set("fold:y", true);
    for (let id = 10; id < 10 + FOLD_SESSION_LIMIT; id += 1) foldScope(id);
    expect(foldScope(1).get("fold:x")).toBeUndefined();
  });
});
