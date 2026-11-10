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

  // 做空审查（2026-11-10）找到的绕过形状：修复前要么栈溢出、要么同步卡 1–60+ 秒。
  const BYPASSES: Array<[string, string]> = [
    ["交错嵌套强调", `${"*a ".repeat(4000)}b${" a*".repeat(4000)}`],
    ["20000 个 *", "*".repeat(20000)],
    ["15000 个 **", "**".repeat(15000)],
    ["15000 个 ~~", "~~".repeat(15000)],
    ["440 层缩进列表", Array.from({ length: 440 }, (_, n) => `${"  ".repeat(n)}- x`).join("\n")],
    ["20000 个链接", "[a](b)".repeat(20000)],
    // 第二轮做空审查：表格与词内下划线（修复前 4.5 秒、4.7 秒、17 秒）。
    ["10 列 × 2000 行表格", ["|" + " h |".repeat(10), "|" + " --- |".repeat(10), ...Array.from({ length: 2000 }, (_, n) => "|" + ` c${n} |`.repeat(10))].join("\n")],
    ["100 列 × 200 行表格", ["|" + " h |".repeat(100), "|" + " - |".repeat(100), ...Array.from({ length: 200 }, () => "|" + " c |".repeat(100))].join("\n")],
    ["a_b_ × 20000", "a_b_".repeat(20000)],
  ];
  it.each(BYPASSES)("%s：不抛错、500ms 内、原文显示", (_name, text) => {
    const { container, ms } = timedRender(text);
    expect(ms).toBeLessThan(500);
    expect(container.querySelector("pre.chat-md-plain")?.textContent).toBe(text);
  });

  it("刚好低于竖线上限的大表格照常排版，且 500ms 内渲染完", () => {
    // 5 列：每行 6 根竖线，330 行 ≈ 1992 根，压在 2000 以下。
    const table = ["| a | b | c | d | e |", "| - | - | - | - | - |", ...Array.from({ length: 328 }, (_, n) => `| ${n} | **x** | y | z | w |`)].join("\n");
    expect(markdownTooComplex(table)).toBe(false);
    const { container, ms } = timedRender(table);
    expect(container.querySelector("table")).toBeTruthy();
    expect(ms).toBeLessThan(500);
  });

  it("正常的强调、snake_case、代码块与长列表不被误降级", () => {
    // 几百条连写的「- **名称**：说明」：列表项各自是独立的行内上下文，不累计。
    const list = Array.from({ length: 600 }, (_, n) => `- **条目 ${n}**：说明 _斜体_ ~~删除~~`).join("\n");
    expect(markdownTooComplex(list)).toBe(false);
    // 表格每行都加粗。
    const table = ["| a | b |", "| --- | --- |", ...Array.from({ length: 600 }, () => "| **x** | *y* |")].join("\n");
    expect(markdownTooComplex(table)).toBe(false);
    // 一段里几百个 snake_case 标识符（800 个词内 `_`，上限 1024）。
    expect(markdownTooComplex(Array.from({ length: 400 }, (_, n) => `foo_bar_${n}`).join(" "))).toBe(false);
    // 长报告里很多张小表：竖线按一张表计，不累计。
    const manyTables = Array.from({ length: 200 }, (_, n) => [`表 ${n}`, "", "| a | b | c |", "| - | - | - |", "| 1 | 2 | 3 |", "| 4 | 5 | 6 |", ""].join("\n")).join("\n");
    expect(markdownTooComplex(manyTables)).toBe(false);
    // 代码块里成片的 `**kwargs` 和深缩进不算（800 行约 14 万字，在总长上限内）。
    const code = ["```py", ...Array.from({ length: 800 }, () => `${" ".repeat(160)}f(**kwargs, *args)`), "```", "正文"].join("\n");
    expect(markdownTooComplex(code)).toBe(false);
    // 分隔线。
    expect(markdownTooComplex("前\n\n***\n\n后")).toBe(false);
  });

  it("预检阈值：正常嵌套放行，超深嵌套、超长拦下", () => {
    expect(markdownTooComplex("> a\n> > b\n- x\n  - y\n    1. z")).toBe(false);
    expect(markdownTooComplex(`${"- ".repeat(40)}x`)).toBe(true);
    expect(markdownTooComplex(`${"[".repeat(20)}\n\n${"[".repeat(20)}`)).toBe(false);
    expect(markdownTooComplex("x".repeat(200_001))).toBe(true);
  });
});
