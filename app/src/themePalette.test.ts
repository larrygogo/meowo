// 主题调色板结构守护（UI 调研 sys7）：深色板只有一份，浅色靠 token 而不是逐个组件打补丁。
// 此前深色值在 :root、.chat-window、.chat-window.is-terminal 抄了三份，0.5.15 起多次漏抄；
// 浅色主题另有 76 条 `:root[data-theme="light"] .xxx` 组件补丁，每加一个写死的深色值都要记得补。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const raw = readFileSync(join(__dirname, "styles.css"), "utf8") + "\n" + readFileSync(join(__dirname, "mobile", "mobile.css"), "utf8");
const css = raw.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "));
const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => ({
  sel: m[1].trim().replace(/\s+/g, " "),
  body: m[2],
  // m[1] 带着上一条规则之后的空白与（已抹成空格的）注释，行号从选择器第一个字符算。
  line: css.slice(0, (m.index ?? 0) + m[1].length - m[1].trimStart().length).split("\n").length,
}));

describe("深色调色板只有一份", () => {
  it("共用块同时挂在根元素与终端页签上", () => {
    expect(rules.filter((r) => r.sel === ":where(.chat-window.is-terminal), :root")).toHaveLength(1);
  });
  it("终端页签块不再重抄调色板（只剩不透明底色）", () => {
    const own = rules.filter((r) => r.sel === ".chat-window.is-terminal").flatMap((r) => [...r.body.matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1]));
    expect(own).toEqual(["--cc-bg"]);
  });
  it("调色板变量在深色侧只定义一次", () => {
    for (const tok of ["--cc-accent", "--cc-border", "--fill-hover", "--shadow-menu", "--cc-menu-bg", "--cc-syn-func"]) {
      const darkDefs = rules.filter((r) => !r.sel.includes('data-theme="light"') && new RegExp(`(^|[;\\s])${tok}\\s*:`).test(r.body));
      expect(darkDefs.map((r) => r.sel), tok).toEqual([":where(.chat-window.is-terminal), :root"]);
    }
  });
});

describe("浅色组件级补丁收敛", () => {
  // 只数「浅色主题下改组件外观」的规则；flat 贴纸风格为压特异度带上的浅色变体不算。
  const patches = rules.filter((r) => r.sel.includes('data-theme="light"]') && r.sel !== ':root[data-theme="light"]' && !r.sel.includes("data-sticker-style"));
  it("不超过 20 条", () => {
    expect(patches.map((r) => `${r.line} ${r.sel}`).length).toBeLessThanOrEqual(20);
  });
  it("每条上方都写了理由（紧邻的注释）", () => {
    const lines = raw.split("\n");
    const missing = patches.filter((r) => {
      // 往上找最近的非空行：必须是注释的一部分，或同一组补丁里上一条（共享同一段注释）
      for (let i = r.line - 2; i >= 0; i--) {
        const l = lines[i].trim();
        if (!l) continue;
        return !(l.endsWith("*/") || l.startsWith("/*") || l.startsWith("*") || l.includes('data-theme="light"]'));
      }
      return true;
    });
    expect(missing.map((r) => `${r.line} ${r.sel}`)).toEqual([]);
  });
});
