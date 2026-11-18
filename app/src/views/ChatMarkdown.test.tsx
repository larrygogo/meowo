import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ChatMarkdown, tableToMarkdown } from "./ChatMarkdown";

afterEach(() => {
  cleanup();
  try { localStorage.removeItem("meowo-chat-code-wrap"); } catch { /* 无存储 */ }
});

describe("ChatMarkdown 表格", () => {
  /// 复制出来的纯文本要能原样贴回 markdown：表头分隔行、单元格里的竖线转义、换行压平。
  it("表格转 markdown 源", () => {
    render(<ChatMarkdown text={"| 名称 | 说明 |\n| --- | --- |\n| a\\|b | 第一行 |\n| c | d |"} />);
    const table = document.querySelector(".chat-md-table table") as HTMLTableElement;
    expect(tableToMarkdown(table)).toBe("| 名称 | 说明 |\n| --- | --- |\n| a\\|b | 第一行 |\n| c | d |");
    expect(screen.getByRole("button", { name: "复制表格" })).toBeTruthy();
  });
});

describe("ChatMarkdown 代码块换行", () => {
  /// 换行钮只在横向溢出时出现；点一次对所有代码块生效（全局偏好），再点关回去。
  it("溢出才给换行钮，开关作用于所有代码块", () => {
    const proto = HTMLElement.prototype;
    const sw = Object.getOwnPropertyDescriptor(proto, "scrollWidth");
    const cw = Object.getOwnPropertyDescriptor(proto, "clientWidth");
    Object.defineProperty(proto, "scrollWidth", { configurable: true, get() { return this.tagName === "PRE" && this.textContent!.length > 20 ? 900 : 100; } });
    Object.defineProperty(proto, "clientWidth", { configurable: true, get() { return this.tagName === "PRE" ? 300 : 0; } });
    try {
      render(<ChatMarkdown text={"```sh\nshort\n```\n\n```sh\na-very-long-command --with --many --flags\n```"} />);
      const blocks = document.querySelectorAll(".chat-md-pre");
      expect(blocks).toHaveLength(2);
      const toggles = screen.getAllByRole("button", { name: "自动换行" });
      expect(toggles).toHaveLength(1);
      fireEvent.click(toggles[0]);
      expect(document.querySelectorAll(".chat-md-pre.is-wrapped")).toHaveLength(2);
      fireEvent.click(screen.getAllByRole("button", { name: "取消换行" })[0]);
      expect(document.querySelectorAll(".chat-md-pre.is-wrapped")).toHaveLength(0);
    } finally {
      if (sw) Object.defineProperty(proto, "scrollWidth", sw); else delete (proto as { scrollWidth?: number }).scrollWidth;
      if (cw) Object.defineProperty(proto, "clientWidth", cw); else delete (proto as { clientWidth?: number }).clientWidth;
    }
  });
});

describe("段尾不留孤字（U2）", () => {
  it("中文段落最后四个字包成一组不折行，文字本身不变", () => {
    const text = "这是一段很长的段落，用来检查消息列宽，而不是把一行拉得很长导致阅读困难。";
    const { container } = render(<ChatMarkdown text={text} />);
    const keep = container.querySelector("p .chat-md-keep");
    expect(keep?.textContent).toBe("读困难。");
    expect(container.querySelector("p")?.textContent).toBe(text);
  });
  it("列表项里的段落同样处理", () => {
    const { container } = render(<ChatMarkdown text={"- 第一项写得比较长一些，看看末尾\n- 第二项也写得比较长，看看末尾的字"} />);
    expect([...container.querySelectorAll("li .chat-md-keep")].map((el) => el.textContent)).toEqual(["看看末尾", "末尾的字"]);
  });
  it("西文、末尾是链接或行内代码、很短的段落不碰", () => {
    const { container } = render(<ChatMarkdown text={"Plain English paragraph that wraps.\n\n结尾是代码 `src/a.ts`\n\n结尾是链接 [文档](https://example.com)\n\n好的。"} />);
    expect(container.querySelector(".chat-md-keep")).toBeNull();
  });
});
