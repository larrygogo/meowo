import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { TranscriptSearch } from "./TranscriptSearch";
import { zh } from "../../i18n/zh";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

/** 区内搜索条：输入即测、Esc 关闭并归还焦点语义（走 useDismissable 的 Esc 层）。
 *  jsdom 下 Range.getClientRects() 不返回真实矩形，故只断言控件与关闭链路，
 *  高亮叠层的实际位置靠桌面端实拍核对。 */
describe("TranscriptSearch", () => {
  it("打字进输入框，Esc 关闭", () => {
    const onClose = vi.fn();
    const host = document.createElement("div");
    host.textContent = "思考过程很短";
    document.body.append(host);
    render(<TranscriptSearch scrollRef={{ current: host }} onClose={onClose} />);
    const input = screen.getByPlaceholderText(zh.chat.searchPlaceholder);
    fireEvent.change(input, { target: { value: "思考" } });
    expect((input as HTMLInputElement).value).toBe("思考");
    fireEvent.keyDown(input, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  // 用户反馈输不了中文：组字期间受控 value 不跟，React 把拼音改回旧值。显示值必须跟着
  // 组字走，搜索（计数）等组字结束才算。
  it("中文输入法组字期间输入框保留拼音，组字结束才搜索", () => {
    const host = document.createElement("div");
    host.textContent = "中文搜索";
    document.body.append(host);
    render(<TranscriptSearch scrollRef={{ current: host }} onClose={() => {}} />);
    const input = screen.getByPlaceholderText(zh.chat.searchPlaceholder) as HTMLInputElement;
    fireEvent.compositionStart(input);
    fireEvent.change(input, { target: { value: "zhong" } });
    expect(input.value).toBe("zhong");
    expect(screen.getByRole("status").textContent).toBe("");
    fireEvent.change(input, { target: { value: "中" } });
    fireEvent.compositionEnd(input);
    expect(input.value).toBe("中");
    expect(screen.getByRole("status").textContent).not.toBe("");
  });

  it("上/下一处与关闭都是可达的按钮", () => {
    const host = document.createElement("div");
    host.textContent = "abc";
    document.body.append(host);
    render(<TranscriptSearch scrollRef={{ current: host }} onClose={() => {}} />);
    expect(screen.getByRole("button", { name: zh.chat.searchPrev })).toBeTruthy();
    expect(screen.getByRole("button", { name: zh.chat.searchNext })).toBeTruthy();
    expect(screen.getByRole("button", { name: zh.chat.close })).toBeTruthy();
  });

  // 滚动会触发重测：此前重测一律把当前项归零，「下一处」一滚动就被拉回第一处（手机实测
  // 计数恒为 1/N）。只有换搜索词才回到第一处。
  it("翻到下一处后滚动重测，当前项不回到第一处", async () => {
    // jsdom 不排版：给元素一个非零矩形，命中才不会被当成不可见滤掉。
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ top: 10, left: 0, width: 20, height: 10, right: 20, bottom: 20, x: 0, y: 10, toJSON: () => ({}) } as DOMRect);
    const host = document.createElement("div");
    for (let i = 0; i < 3; i++) {
      const p = document.createElement("p");
      p.textContent = `foo ${i}`;
      host.append(p);
    }
    document.body.append(host);
    render(<TranscriptSearch scrollRef={{ current: host }} onClose={() => {}} />);
    fireEvent.change(screen.getByPlaceholderText(zh.chat.searchPlaceholder), { target: { value: "foo" } });
    expect(await screen.findByText("1/3")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: zh.chat.searchNext }));
    expect(screen.getByText("2/3")).toBeTruthy();
    await act(async () => {
      host.dispatchEvent(new Event("scroll"));
      await new Promise((resolve) => requestAnimationFrame(() => resolve(null)));
    });
    expect(screen.getByText("2/3")).toBeTruthy();
  });
});
