import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { findTextRanges, TranscriptSearch } from "./TranscriptSearch";
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

/// 搜索按用户看到的连续文字匹配：同一块里被行内元素切开的词也能搜到（段尾不折行的那组字、加粗、行内代码）。
describe("findTextRanges 跨行内元素匹配", () => {
  const mount = (html: string) => {
    const host = document.createElement("div");
    host.innerHTML = html;
    document.body.append(host);
    return host;
  };
  afterEach(() => { document.body.innerHTML = ""; });

  it("对话正文的段尾词（ChatMarkdown 把最后四个字包进 .chat-md-keep）能搜到，范围正好覆盖这个词", async () => {
    const { ChatMarkdown } = await import("../ChatMarkdown");
    const { container } = render(<ChatMarkdown text="改完了，规则都收进了 schema 里。接下来推到远端分支。" />);
    expect(container.querySelector(".chat-md-keep")?.textContent).toBe("端分支。");
    const ranges = findTextRanges(container, "远端分支");
    expect(ranges.map((range) => range.toString())).toEqual(["远端分支"]);
    expect(ranges[0].startContainer).not.toBe(ranges[0].endContainer);
  });

  it("加粗、行内代码切开的词也能搜到", () => {
    const host = mount("<p>运行 <strong>vi</strong>test 之后看 <code>login</code>.ts 的结果</p>");
    expect(findTextRanges(host, "vitest").map(String)).toEqual(["vitest"]);
    expect(findTextRanges(host, "login.ts").map(String)).toEqual(["login.ts"]);
  });

  it("不跨块拼接：两段之间不会凭空连出一个词", () => {
    const host = mount("<p>前一段结尾是远</p><p>端分支开头</p>");
    expect(findTextRanges(host, "远端")).toEqual([]);
    expect(findTextRanges(host, "端分支").map(String)).toEqual(["端分支"]);
  });

  it("大小写不敏感（调用方传小写），一段里多处都找到", () => {
    const host = mount("<p>Foo <em>foo</em> fOO</p>");
    expect(findTextRanges(host, "foo").map(String)).toEqual(["Foo", "foo", "fOO"]);
  });
});

describe("区内搜索命中跨节点的段尾词（复核用例：flow-states 搜「别名没」）", () => {
  it("搜「别名没」命中 1 次并画出高亮", async () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ top: 0, left: 0, width: 600, height: 400, right: 600, bottom: 400, x: 0, y: 0, toJSON: () => ({}) } as DOMRect);
    const { ChatMarkdown } = await import("../ChatMarkdown");
    const { container } = render(<div className="chat-scroll"><ChatMarkdown text="边界情况都覆盖到了，只有加号别名没测。" /></div>);
    const host = container.firstElementChild as HTMLElement;
    expect(host.querySelector(".chat-md-keep")?.textContent).toBe("名没测。");
    render(<TranscriptSearch scrollRef={{ current: host }} onClose={() => {}} />);
    fireEvent.change(screen.getByPlaceholderText(zh.chat.searchPlaceholder), { target: { value: "别名没" } });
    expect(await screen.findByText("1/1")).toBeTruthy();
    expect(document.querySelectorAll(".chat-search-mark.is-current")).toHaveLength(1);
  });
});
