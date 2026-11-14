import { afterEach, describe, expect, it } from "vitest";
import { act, cleanup, render } from "@testing-library/react";
import { useRef } from "react";
import type { ChatItem } from "../../api";
import { countsAsUnseen, useStickToBottom } from "./useStickToBottom";

afterEach(cleanup);

/// jsdom 不做布局：滚动区与内层容器的几何量由测试给。
function geometry(el: HTMLElement, values: { scrollHeight: number; clientHeight: number; scrollTop?: number }) {
  Object.defineProperty(el, "scrollHeight", { configurable: true, value: values.scrollHeight });
  Object.defineProperty(el, "clientHeight", { configurable: true, value: values.clientHeight });
  if (values.scrollTop !== undefined) el.scrollTop = values.scrollTop;
}

const say = (id: string): ChatItem => ({ type: "assistant_text", id, timestamp: null, text: id });

let api: ReturnType<typeof useStickToBottom>;
function Harness({ items }: { items: ChatItem[] }) {
  const scrollRef = useRef<HTMLDivElement>(null);
  api = useStickToBottom({ scrollRef, visible: true, timelineItems: items, contentCount: items.filter(countsAsUnseen).length });
  return (
    <div ref={scrollRef} data-testid="scroll" onScroll={api.onScroll}>
      <pre data-testid="inner" style={{ overflowY: "auto" }}>长输出</pre>
      {!api.atBottom && <button type="button">回到最新{api.unseenCount > 0 ? ` ${api.unseenCount}` : ""}</button>}
    </div>
  );
}

describe("useStickToBottom", () => {
  it("滚轮落在内层滚动区后来了新内容：不再吸底，但「回到最新」和未读数会出现", () => {
    const view = render(<Harness items={[say("a1")]} />);
    const scroll = view.getByTestId("scroll");
    const inner = view.getByTestId("inner");
    // 主区贴着底；内层 pre 自己有滚动余量。
    geometry(scroll, { scrollHeight: 1000, clientHeight: 400, scrollTop: 600 });
    geometry(inner, { scrollHeight: 900, clientHeight: 200, scrollTop: 0 });
    act(() => { inner.dispatchEvent(new WheelEvent("wheel", { deltaY: 40, bubbles: true })); });
    expect(api.followRef.current).toBe(false);
    // 此刻主区还在底部，钮不出现。
    expect(view.queryByRole("button")).toBeNull();

    // 新内容到达，主区内容长高了 500px，没有任何主区滚动事件。
    geometry(scroll, { scrollHeight: 1500, clientHeight: 400 });
    view.rerender(<Harness items={[say("a1"), say("a2"), say("a3")]} />);
    // 没被拽到底……
    expect(scroll.scrollTop).toBe(600);
    // ……但有出口，且数得出新来了两条。此前这里什么都不出现。
    expect(view.getByRole("button").textContent).toBe("回到最新 2");

    act(() => { api.jumpToLatest(); });
    expect(view.queryByRole("button")).toBeNull();
    expect(api.followRef.current).toBe(true);
  });

  it("换会话时「回到最新」一并收掉：新会话不足一屏、没有滚动事件也不残留", () => {
    const view = render(<Harness items={[say("a1"), say("a2")]} />);
    const scroll = view.getByTestId("scroll");
    // 旧会话滚到了最顶上。
    geometry(scroll, { scrollHeight: 2000, clientHeight: 400, scrollTop: 0 });
    act(() => { scroll.dispatchEvent(new Event("scroll", { bubbles: true })); });
    expect(view.getByRole("button")).toBeTruthy();

    // 切到一个内容不足一屏的会话：scrollTop 仍是 0，不会再有滚动事件。
    geometry(scroll, { scrollHeight: 300, clientHeight: 400, scrollTop: 0 });
    act(() => { api.resetForSession(); });
    view.rerender(<Harness items={[say("b1")]} />);
    expect(view.queryByRole("button")).toBeNull();
  });
});
