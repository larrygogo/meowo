// 按钮基件守护（UI 调研 sys3 / sys15）：主按钮只有一种长相、文字按钮只有三档高度。
// 此前全仓 35 个按钮基类、4 种 is-primary 写法，审批卡「允许一次」是黑白反色、
// 「更改」面板提交是蓝底，同一个对话窗里分不清哪个是主动作。
import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const SRC = join(__dirname);
const styles = readFileSync(join(SRC, "styles.css"), "utf8");
const mobile = readFileSync(join(SRC, "mobile", "mobile.css"), "utf8");
const css = (styles + "\n" + mobile).replace(/\/\*[\s\S]*?\*\//g, "");

function tsxFiles(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    if (name === "generated" || name === "node_modules") continue;
    const p = join(dir, name);
    if (statSync(p).isDirectory()) tsxFiles(p, out);
    else if (name.endsWith(".tsx") && !name.includes(".test.")) out.push(p);
  }
  return out;
}

/** 取某个选择器（精确匹配整条选择器）的声明体。 */
function ruleBody(selector: string): string {
  const esc = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  // 同一选择器可能分几段写（基础 + 后补），全部拼起来再查。
  const re = new RegExp(`(?:^|[}\\n])\\s*${esc}\\s*\\{([^}]*)\\}`, "g");
  return [...css.matchAll(re)].map((m) => m[1]).join(";");
}

describe("主按钮只有一种", () => {
  it(".btn-primary 是蓝底配 --cc-on-accent，悬停取 --cc-accent-dim", () => {
    expect(ruleBody(".btn-primary:is(button, a)")).toMatch(/background:\s*var\(--cc-accent\)/);
    expect(ruleBody(".btn-primary:is(button, a)")).toMatch(/(?<![-\w])color:\s*var\(--cc-on-accent\)/);
    expect(ruleBody(".btn-primary:is(button, a):hover:not(:disabled)")).toMatch(/background:\s*var\(--cc-accent-dim\)/);
  });

  // 灵动岛例外：固定黑底的系统级浮层，白色胶囊主按钮是它自己的材质（与刘海同一套视觉），不随主题。
  it("样式表里不再有各自为政的主按钮写法", () => {
    const sels = [...css.matchAll(/([^{}]+)\{/g)].map((m) => m[1].trim()).filter((s) => !s.startsWith("@"));
    const bad = sels.filter((s) => /(\.is-primary|\.primary\b|--primary\b|-action-primary\b|\.is-allow\b)/.test(s) && !/\.isl-btn\.is-primary/.test(s));
    expect(bad).toEqual([]);
  });

  it("tsx 里主按钮一律组合 btn-primary", () => {
    const bad: string[] = [];
    for (const f of tsxFiles(SRC)) {
      const text = readFileSync(f, "utf8");
      text.split("\n").forEach((l, i) => {
        if (/className=[^>]*(\bis-primary\b|["' ]primary["' ]|--primary\b|-action-primary\b|\bis-allow\b)/.test(l) && !/isl-btn is-primary/.test(l)) {
          bad.push(`${f.slice(SRC.length + 1)}:${i + 1}`);
        }
      });
    }
    expect(bad).toEqual([]);
  });
});

describe("文字按钮三档高度 24/28/32", () => {
  it("高度 token 定义为 24/28/32", () => {
    expect(styles).toMatch(/--btn-h-sm:\s*24px;/);
    expect(styles).toMatch(/--btn-h-md:\s*28px;/);
    expect(styles).toMatch(/--btn-h-lg:\s*32px;/);
  });

  // 文字按钮基类清单（图标按钮另走 --icon-button-*，标题栏窗控、灵动岛、手机触控按钮不在此列）。
  const TEXT_BUTTONS = [
    ".nsp-ws-btn", ".nsp-note-btn", ".sa-btn-q", ".chat-diff-layout button",
    ".chat-diff-cmt-actions button", ".scm-commit-btn", ".scm-forge-btn", ".files-roots button",
    ".chat-approval-reason button", ".chat-approval button", ".chat-review-bar button", ".managed-terminal-cover-actions > button",
    ".wt-opt-btn", ".wt-trust-actions .wt-opt-btn", ".chat-resume-permission .dd-btn",
  ];
  it.each(TEXT_BUTTONS)("%s 的高度取 --btn-h-*", (sel) => {
    const body = ruleBody(sel);
    expect(body, sel).toMatch(/(?<![-\w])height:\s*(var\(--btn-h-(sm|md|lg)\)|calc\(var\(--btn-h-(sm|md|lg)\) \* var\(--cc-ui\)\))/);
  });
});
