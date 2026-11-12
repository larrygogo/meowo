import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// 右侧面板的对齐规则都是纯样式，jsdom 不排版，量不出位置；这里守住规则本身，
// 实际坐标靠截图场景（qa-panel-max*、qa-diff-max-2560、qa-md-max*、qa-files-ui150、qa-panel-ui150）。
const css = readFileSync(resolve(__dirname, "../../styles.css"), "utf8");
const rule = (selector: string) => {
  const at = css.indexOf(`${selector} {`);
  return at < 0 ? "" : css.slice(at, css.indexOf("}", at));
};
const all = (selector: string) => css.split(/\r?\n/).filter((line) => line.startsWith(`${selector} {`)).join("\n");

describe("单文件视图：头行与正文同一条内容列", () => {
  // 2560 最大化时「返回」在 x=250、「打开」右缘在 x=2542，代码只到 x≈800；
  // Markdown 正文在 1102–1914，头行却在 474–2542。
  it("头行和滚动区都按 --view-col-pad 留白", () => {
    expect(all(".chat-diff-filehead")).toContain("padding-inline: calc(var(--view-col-pad) + var(--sp-10))");
    expect(all(".chat-diff-scroll")).toContain("padding-inline: var(--view-col-pad)");
  });
  it("Markdown 预览换列宽时两个变量一起重写，正文不再自带 max-width", () => {
    const md = rule(".chat-files-preview:has(> .chat-diff-scroll > .chat-diff-mdview)");
    expect(md).toContain("--view-col: 880px");
    expect(md).toContain("--view-col-pad:");
    expect(all(".chat-diff-mdview")).not.toContain("max-width");
  });
});

describe("「更改」面板：底板一条左缘、图标一列", () => {
  // 改前仓库行底板在 8、卡片在 12，分组箭头 12、文件图标 20、「存储库」24。
  it("仓库区、提交框、推送区、分组的左右内边距都是 --sp-12", () => {
    expect(all(".scm-repos")).toContain("padding: 0 var(--sp-12)");
    expect(all(".scm-groups")).toContain("padding: 0 var(--sp-12)");
    expect(all(".scm-commit")).toMatch(/padding: var\(--sp-12\) var\(--sp-12\)/);
    expect(all(".scm-forge")).toMatch(/padding: var\(--sp-4\) var\(--sp-12\)/);
  });
  it("小标题、仓库行、分组头、文件行的内容都从底板内 --sp-10 起", () => {
    expect(rule(".scm-section-title")).toContain("padding: var(--sp-10) var(--sp-10)");
    expect(rule(".scm-repo")).toContain("padding: 0 var(--sp-10)");
    expect(rule(".scm-group-toggle")).toContain("padding: 0 var(--sp-4) 0 var(--sp-10)");
    expect(rule(".scm-row-main")).toContain("padding: 0 var(--sp-4) 0 var(--sp-10)");
  });
  it("仓库区的分隔线画在内容列里，不再是容器自己的 border", () => {
    expect(all(".scm-repos")).not.toContain("border-bottom");
    expect(rule(".scm-repos::after")).toContain("background: var(--cc-border)");
  });
});

describe("界面缩放：行高与按钮高乘 --cc-ui", () => {
  // 150% 时字高 12→17.5px，按钮仍 28px，上下余量从 8px 挤到 5px。
  it.each([
    ".scm-repo", ".scm-commit-btn", ".scm-forge-btn", ".scm-group-head", ".scm-row", ".scm-row-main",
    ".chat-diff-search", ".chat-diff-tree-dir, .chat-diff-tree-file",
  ])("%s", (selector) => {
    expect(rule(selector)).toMatch(/height: calc\(var\(--btn-h-md\) \* var\(--cc-ui\)\)/);
  });
});

describe("文件树：文件行补箭头位", () => {
  it("没有箭头的树行前面补 12px，列表形态不补", () => {
    expect(rule(".chat-diff-tree-file:not(.is-flat):not(:has(> .chat-diff-chev))::before")).toContain("flex: 0 0 12px");
  });
});

describe("终端搜索条", () => {
  it("开着时藏掉同一个角上的按钮，底色不透明", () => {
    expect(rule(".term-search ~ .managed-terminal-actions")).toContain("visibility: hidden");
    expect(rule(".term-search")).toContain("background: rgb(34, 35, 37)");
  });
});
