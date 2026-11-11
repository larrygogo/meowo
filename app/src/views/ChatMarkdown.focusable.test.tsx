// 横向溢出的代码块可键盘聚焦（axe scrollable-region-focusable，验收 #23）；不溢出的不进 Tab 序。
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ChatMarkdown } from "./ChatMarkdown";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it("溢出的代码块 tabIndex=0 且有名字，短代码块不可聚焦", () => {
  vi.spyOn(HTMLElement.prototype, "scrollWidth", "get").mockImplementation(function (this: HTMLElement) {
    return this.tagName === "PRE" && this.textContent!.length > 20 ? 900 : 100;
  });
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(function (this: HTMLElement) {
    return this.tagName === "PRE" ? 300 : 0;
  });
  render(<ChatMarkdown text={"```sh\nshort\n```\n\n```sh\na-very-long-command --with --many --flags\n```"} />);
  const pres = document.querySelectorAll(".chat-md-pre > pre");
  expect(pres[0].hasAttribute("tabindex")).toBe(false);
  expect(pres[1].getAttribute("tabindex")).toBe("0");
  expect(screen.getByRole("region", { name: "sh 代码" })).toBe(pres[1]);
});
