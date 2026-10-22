import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createRef } from "react";
import type { ChatItem } from "../../api";
import { UserMessageNav, userNavEntries } from "./UserMessageNav";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}));

const user = (id: string, text: string): ChatItem => ({ type: "user_text", id, timestamp: null, text });
const ai = (id: string, text: string): ChatItem => ({ type: "assistant_text", id, timestamp: null, text });

afterEach(cleanup);

describe("userNavEntries", () => {
  it("只收用户自己说的话:命令显示成命令行,通知/免责声明/助手消息不进导航", () => {
    const entries = userNavEntries([
      user("u1", "帮我\n  看看这个 bug"),
      ai("a1", "好的"),
      user("c1", "<command-name>/model</command-name>\n<command-message>model</command-message>\n<command-args>opus</command-args>"),
      user("caveat", "<local-command-caveat>Caveat: ignore</local-command-caveat>"),
      user("n1", "<task-notification>\n<task-id>x</task-id>\n<status>completed</status>\n</task-notification>"),
      user("u2", "继续"),
    ]);
    expect(entries).toEqual([
      { id: "u1", label: "帮我 看看这个 bug", kind: "text" },
      { id: "c1", label: "/model opus", kind: "command" },
      { id: "u2", label: "继续", kind: "text" },
    ]);
  });

  it("连续纯图消息与 Transcript 一样并成一组,只留组首", () => {
    const entries = userNavEntries([
      user("i1", "[Image: source: C:\\tmp\\a.png]"),
      user("i2", "[Image: source: C:\\tmp\\b.png]"),
      user("u1", "这两张图"),
    ]);
    expect(entries.map((entry) => [entry.id, entry.kind])).toEqual([["i1", "image"], ["u1", "text"]]);
  });
});

describe("UserMessageNav", () => {
  it("少于两条不渲染;点导航项把对应气泡滚进视口并标为当前", () => {
    const scrollRef = createRef<HTMLElement>();
    const { rerender } = render(<>
      <main ref={scrollRef}><article data-msg-id="u1">第一条</article></main>
      <UserMessageNav items={[user("u1", "第一条")]} scrollRef={scrollRef} />
    </>);
    expect(screen.queryByRole("navigation", { name: "用户消息导航" })).toBeNull();

    const items = [user("u1", "第一条"), ai("a1", "回复"), user("u2", "第二条问题")];
    rerender(<>
      <main ref={scrollRef}>
        <article data-msg-id="u1">第一条</article>
        <article data-msg-id="u2">第二条问题</article>
      </main>
      <UserMessageNav items={items} scrollRef={scrollRef} />
    </>);
    const target = scrollRef.current!.querySelector<HTMLElement>('[data-msg-id="u1"]')!;
    const scrollIntoView = vi.fn();
    target.scrollIntoView = scrollIntoView;
    const nav = screen.getByRole("navigation", { name: "用户消息导航" });
    // 收起态文字不可见，按钮名字靠 aria-label 提供。
    const button = within(nav).getByRole("button", { name: "第一条" });
    fireEvent.click(button);
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "start", behavior: "instant" });
    expect(button.getAttribute("aria-current")).toBe("true");
  });

  it("触屏/窄窗命中隐藏查询时不渲染、不挂滚动监听", () => {
    vi.stubGlobal("matchMedia", () => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
    try {
      const scrollRef = createRef<HTMLElement>();
      const items = [user("u1", "第一条"), user("u2", "第二条")];
      const main = document.createElement("main");
      const addListener = vi.spyOn(main, "addEventListener");
      (scrollRef as { current: HTMLElement }).current = main;
      render(<UserMessageNav items={items} scrollRef={scrollRef} />);
      expect(screen.queryByRole("navigation", { name: "用户消息导航" })).toBeNull();
      expect(addListener).not.toHaveBeenCalledWith("scroll", expect.anything(), expect.anything());
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("悬停时横线按离光标的远近变长,移开恢复", () => {
    const scrollRef = createRef<HTMLElement>();
    const items = ["a", "b", "c", "d", "e", "f"].map((id) => user(id, "消息 " + id));
    render(<>
      <main ref={scrollRef}>{items.map((item) => <article key={item.id} data-msg-id={item.id} />)}</main>
      <UserMessageNav items={items} scrollRef={scrollRef} />
    </>);
    const nav = screen.getByRole("navigation", { name: "用户消息导航" });
    const buttons = within(nav).getAllByRole("button");
    const widths = () => buttons.map((button) => button.querySelector<HTMLElement>(".chat-msg-nav-dash")!.style.width);
    fireEvent.mouseEnter(buttons[2]);
    expect(widths()).toEqual(["13px", "16px", "20px", "16px", "13px", "10px"]);
    fireEvent.mouseLeave(nav.querySelector(".chat-msg-nav-list")!);
    expect(widths()).toEqual(["", "", "", "", "", ""]);
  });
});
