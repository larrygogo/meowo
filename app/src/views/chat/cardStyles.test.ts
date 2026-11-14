import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

/// 卡片样式里靠截图量出来的两处，写成规则钉住（jsdom 不排版，只能查样式表的文字）。
const css = readFileSync(resolve(__dirname, "../../styles.css"), "utf8");
/// 取以这个选择器开头的那条规则（行首匹配，不会误取到更长选择器的尾巴）。
const rule = (selector: string) => {
  const at = css.split(/\r?\n/).find((line) => line.startsWith(selector + " {"));
  return at ?? "";
};

describe("卡片样式", () => {
  /// 2026-11-14 手机页实测（elementFromPoint）：「仅收起」是 static 时，手机页给按钮加的外扩热区
  /// （mobile.css 的 button::after，绝对定位）改以整张卡为参照，盖住卡片正文，点选项、点命令、
  /// 点复制落点全是「仅收起」。它必须是定位元素。
  it("卡片里文字形态的「仅收起」是定位元素，热区不会盖住整张卡", () => {
    const inline = rule(".chat-attention-dismiss.is-inline");
    expect(inline).toContain("position: relative");
    expect(inline).not.toContain("position: static");
    // 基类是绝对定位在右上角的 ×：回到文档流时偏移要清掉，否则按钮会错位。
    expect(inline).toContain("top: auto");
    expect(inline).toContain("right: auto");
  });

  /// 2026-11-14 量测：手机英文的「Allow and remember (this project, local) · cargo test」超出卡片左右各 3px。
  it("持久放行按钮最宽到卡片内宽，标签自己省略", () => {
    expect(rule(".chat-approval-actions-side > button.is-persistent")).toContain("max-width: 100%");
    // 其他选项按钮上是终端原文，同样可能很长，同一条宽度规则；类名在样式表里真实存在。
    expect(rule(".chat-approval-actions-side > button.is-other")).toContain("max-width: 100%");
    const label = rule(".chat-approval-persist-label");
    expect(label).toContain("overflow: hidden");
    expect(label).toContain("text-overflow: ellipsis");
  });

  /// 卡片待处理时输入框照常可写：整块输入区不能再设 pointer-events:none。
  it("屏幕卡在场时输入区只降级工具行，不挡输入框的指针", () => {
    expect(css).not.toMatch(/\.chat-compose\.is-locked\s*\{[^}]*pointer-events:\s*none/);
    expect(rule(".chat-compose.is-locked > :not(textarea)")).toContain("opacity");
  });
});
