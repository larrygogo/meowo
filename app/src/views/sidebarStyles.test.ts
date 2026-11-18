import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

/// 会话列表的排版回归（jsdom 不排版，只能查样式表的文字，同 chat/cardStyles.test.ts）。
const styles = readFileSync(resolve(__dirname, "../styles.css"), "utf8");
const mobile = readFileSync(resolve(__dirname, "../mobile/mobile.css"), "utf8");
/// 选择器正好是 `selector` 的全部规则体（单行、多行规则都取得到）。
const rules = (css: string, selector: string) => {
  const out: string[] = [];
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const re = new RegExp(`(?:^|\\n)\\s*${escaped}\\s*\\{([^}]*)\\}`, "g");
  for (let m = re.exec(css); m; m = re.exec(css)) out.push(m[1]);
  return out;
};

describe("会话列表条目", () => {
  /// 2026-11-18 手机实拍：手机卡片显式写了 min-height，盖掉 flex 子项默认的 min-height:auto；列表是定高的
  /// flex 列，条目一多卡片就被压到 min-height，「目录已删除」标签溢出到下一张卡上。条目必须不可压缩。
  it("条目 flex-shrink:0，任何覆盖规则都不把它改回可压缩", () => {
    const base = rules(styles, ".chat-sidebar-item");
    expect(base.some((body) => /flex-shrink:\s*0/.test(body))).toBe(true);
    const overrides = [...base, ...rules(mobile, ".remote-session-list .chat-sidebar-item")];
    expect(overrides.length).toBeGreaterThan(2);
    for (const body of overrides) {
      expect(body).not.toMatch(/flex-shrink:\s*[1-9]/);
      // `flex:` 简写会把 flex-shrink 一起重置成 1。
      expect(body).not.toMatch(/(^|[;\s])flex:\s/);
    }
  });

  it("手机卡片的 min-height 规则还在（上面那条断言才有意义）", () => {
    expect(rules(mobile, ".remote-session-list .chat-sidebar-item").some((body) => /min-height:\s*64px/.test(body))).toBe(true);
  });

  it("「目录已删除」标签不缩、负外边距抵掉边框，不把这一行撑高", () => {
    const [gone] = rules(styles, ".chat-sidebar-gone");
    expect(gone).toMatch(/flex:\s*none/);
    expect(gone).toMatch(/margin-block:\s*-2px/);
  });
});
