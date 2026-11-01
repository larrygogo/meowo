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
