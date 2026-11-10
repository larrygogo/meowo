import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import { ChatMarkdown } from "./ChatMarkdown";
import { markdownTooComplex } from "./chat/markdownGuard";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

// 病态正文（markdownGuard.ts 文件头的实测）：3000 层嵌套引用让 react-markdown 栈溢出、
// 20000 层成对嵌套的 `[…]` 让链接标签反复回溯、同步卡 5–7 秒。修复后两者都直接按原文显示。
const DEEP_QUOTE = `${">".repeat(3000)} 深处的一句话`;
// 成对嵌套才是慢的那种：只开不闭约 0.1 秒，`[[…x…]]` 20000 层实测 7 秒（HEAD 代码，jsdom）。
const DEEP_BRACKETS = `${"[".repeat(20000)}链接${"]".repeat(20000)}`;

function timedRender(text: string) {
  // 消息级兜底会 console.error 记录被接住的异常；这里只关心不抛、不卡。
  vi.spyOn(console, "error").mockImplementation(() => {});
  const start = performance.now();
  const view = render(<div className="chat-md"><ChatMarkdown text={text} /></div>);
  return { ...view, ms: performance.now() - start };
}

/** 一份正常的长报告：标题、段落、列表、三层引用、表格、代码块、链接，约 6 万字。 */
function normalReport(): string {
  const section = (n: number) => [
    `## 第 ${n} 节：性能与稳定性`,
    "",
    `这一节记录第 ${n} 轮压测的结论，包含 [对照数据](https://example.com/run/${n}) 与 **关键发现**，以及一些 \`inline code\`。`.repeat(3),
    "",
    "- 启动耗时下降明显",
    "  - 冷启动 1.2s → 0.8s",
    "  - 热启动不变",
    "- 内存占用平稳",
    "",
    "> 引用一层",
    "> > 引用两层",
    "> > > 引用三层",
    "",
    "| 指标 | 改前 | 改后 |",
    "| --- | --- | --- |",
    "| 首屏 | 900KB | 550KB |",
    "",
    "```ts",
    `const round${n} = [[1, 2], [3, [4, 5]]];`,
    "```",
    "",
  ].join("\n");
  const parts: string[] = [];
  for (let n = 1; parts.join("\n").length < 60_000; n += 1) parts.push(section(n));
  return parts.join("\n");
}

describe("病态 markdown 降级", () => {
  it("3000 层嵌套引用：不抛错、500ms 内、原文显示", () => {
    const { container, ms } = timedRender(DEEP_QUOTE);
    expect(ms).toBeLessThan(500);
    expect(container.querySelector("pre.chat-md-plain")?.textContent).toBe(DEEP_QUOTE);
    expect(container.querySelector("blockquote")).toBeNull();
  });

  it("20000 层嵌套 [ ]：不卡、500ms 内、原文显示", () => {
    const { container, ms } = timedRender(DEEP_BRACKETS);
    expect(ms).toBeLessThan(500);
    expect(container.querySelector("pre.chat-md-plain")?.textContent).toBe(DEEP_BRACKETS);
  });

  it("几万字的正常报告照常排版，不被误降级", () => {
    const text = normalReport();
    expect(text.length).toBeGreaterThan(60_000);
    expect(markdownTooComplex(text)).toBe(false);
    const { container } = timedRender(text);
    expect(container.querySelector("pre.chat-md-plain")).toBeNull();
    expect(container.querySelectorAll("h2").length).toBeGreaterThan(10);
    expect(container.querySelector("table")).toBeTruthy();
    expect(container.querySelector("blockquote blockquote blockquote")).toBeTruthy();
  });

  it("预检阈值：正常嵌套放行，超深嵌套、超长拦下", () => {
    expect(markdownTooComplex("> a\n> > b\n- x\n  - y\n    1. z")).toBe(false);
    expect(markdownTooComplex(`${"- ".repeat(40)}x`)).toBe(true);
    expect(markdownTooComplex(`${"[".repeat(20)}\n\n${"[".repeat(20)}`)).toBe(false);
    expect(markdownTooComplex("x".repeat(200_001))).toBe(true);
  });
});
