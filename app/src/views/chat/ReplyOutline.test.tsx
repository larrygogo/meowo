import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatItem } from "../../api";
import { ChatMarkdown } from "../ChatMarkdown";
import { Message } from "./Message";

afterEach(cleanup);

const longReply = [
  "## 问题一：按钮圆角",
  "说明".repeat(300),
  "## 问题二：间距",
  "说明".repeat(300),
  "### 细节",
  "补充".repeat(100),
].join("\n\n");
const reply = (text: string, type: "assistant_text" | "assistant_delta" = "assistant_text") =>
  ({ type, id: "a1", timestamp: null, text }) as ChatItem;

describe("长回复目录", () => {
  it("写完的长回复列出小标题，点一条跳到它", () => {
    const scroll = vi.fn();
    Element.prototype.scrollIntoView = scroll;
    render(<Message item={reply(longReply)} />);
    const toggle = screen.getByRole("button", { name: "目录 · 3" });
    fireEvent.click(toggle);
    const items = [...document.querySelectorAll(".chat-outline-item")].map((el) => el.textContent);
    expect(items).toEqual(["问题一：按钮圆角", "问题二：间距", "细节"]);
    fireEvent.click(screen.getByRole("button", { name: "问题二：间距" }));
    expect(scroll).toHaveBeenCalledTimes(1);
    expect((scroll.mock.contexts[0] as HTMLElement).textContent).toBe("问题二：间距");
    expect(document.querySelector(".chat-outline-menu")).toBeNull();
  });

  it("还在流式输出、太短、或只有一个小标题：不给目录", () => {
    render(<Message item={reply(longReply, "assistant_delta")} />);
    expect(document.querySelector(".chat-outline")).toBeNull();
    cleanup();
    render(<Message item={reply("## 一\n\n短\n\n## 二\n\n短")} />);
    expect(document.querySelector(".chat-outline")).toBeNull();
    cleanup();
    render(<Message item={reply(`## 只有一个\n\n${"字".repeat(2000)}`)} />);
    expect(document.querySelector(".chat-outline")).toBeNull();
  });

  it("菜单按 Esc 或点外面关掉", () => {
    render(<Message item={reply(longReply)} />);
    fireEvent.click(screen.getByRole("button", { name: "目录 · 3" }));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(document.querySelector(".chat-outline-menu")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "目录 · 3" }));
    fireEvent.pointerDown(document.body);
    expect(document.querySelector(".chat-outline-menu")).toBeNull();
  });
});

describe("超长代码块收起", () => {
  const code = (lines: number) => "```ts\n" + Array.from({ length: lines }, (_, i) => `const line${i} = ${i};`).join("\n") + "\n```";

  it("超过 30 行默认收起，给「展开全部 N 行」；展开后能收回", () => {
    render(<ChatMarkdown text={code(40)} />);
    const block = document.querySelector(".chat-md-pre")!;
    expect(block.classList.contains("is-folded")).toBe(true);
    const fold = screen.getByRole("button", { name: "展开全部 40 行" });
    fireEvent.click(fold);
    expect(block.classList.contains("is-folded")).toBe(false);
    expect(screen.getByRole("button", { name: "收起代码" }).getAttribute("aria-expanded")).toBe("true");
  });

  it("30 行以内不收起、不出按钮", () => {
    render(<ChatMarkdown text={code(30)} />);
    expect(document.querySelector(".chat-md-pre.is-folded")).toBeNull();
    expect(document.querySelector(".chat-md-pre-fold")).toBeNull();
  });
});
