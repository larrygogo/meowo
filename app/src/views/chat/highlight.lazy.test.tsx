import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, waitFor } from "@testing-library/react";
import { ChatMarkdown } from "../ChatMarkdown";
import { UnifiedDiff } from "./ToolActivity";

afterEach(cleanup);

// hljs 改成第一次需要时才动态加载（highlight.ts）：这个文件里不预先加载，验证首帧按纯文本显示、
// 引擎到位后已经显示着的代码块自己上色。vitest 每个测试文件模块独立，引擎状态从零开始，
// 所以两种调用方放在同一个用例里一起验（第二个用例时引擎早已加载）。
describe("代码高亮懒加载", () => {
  it("加载前按原文显示，加载后对话代码块与工具 diff 都上色", async () => {
    const hunks = [{ old_start: 1, old_lines: 1, new_start: 1, new_lines: 1, lines: ["-let a = 1;", "+let a = 2;"] }];
    const { container } = render(
      <>
        <div data-testid="md"><ChatMarkdown text={"```ts\nconst answer = 42;\n```"} /></div>
        <div data-testid="diff"><UnifiedDiff hunks={hunks} lang="rust" /></div>
      </>,
    );
    const md = container.querySelector('[data-testid="md"]')!;
    const diff = container.querySelector('[data-testid="diff"]')!;
    // 首帧：文本完整、没有任何 hljs 着色。
    expect(md.querySelector("code.language-ts")?.textContent).toContain("const answer = 42;");
    expect(diff.textContent).toContain("let a = 1;");
    expect(container.querySelector('[class^="hljs-"]')).toBeNull();
    // 引擎到位：同一批 DOM 自己重渲染出颜色，不用父层重来。
    await waitFor(() => {
      expect(md.querySelector("code.language-ts .hljs-keyword")?.textContent).toBe("const");
      expect(diff.querySelector(".hljs-keyword")?.textContent).toBe("let");
    });
    // 上色只加 span，不改字：文本保持一致，排版不跳。
    expect(md.querySelector("code.language-ts")?.textContent).toContain("const answer = 42;");
    expect(diff.textContent).toContain("let a = 2;");
  });
});
