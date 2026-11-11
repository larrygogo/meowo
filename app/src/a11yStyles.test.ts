// 无障碍样式守卫：对比度与字重这类「改一个 token 就能悄悄回退」的约束，用源码扫描钉住。
// 实测数字出自 2026-11-11 UI 调研（scratchpad ui-audit/island-a11y/contrast.js），
// 修复后的深浅两套复测脚本在 ui-lanes/a11y/contrast.mjs。
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const styles = readFileSync(join(here, "styles.css"), "utf8");
const mobile = readFileSync(join(here, "mobile/mobile.css"), "utf8");

describe("字重只用 400/500/600/700", () => {
  // 微软雅黑只有 400/700 两档：450/475/650 让中文按 400 或 700 渲染、Inter 按中间值渲染，
  // 同一行中英文粗细不一（调研 sys16）。
  it("styles.css 与 mobile.css 不出现 450/475/650", () => {
    for (const [name, css] of [["styles.css", styles], ["mobile.css", mobile]] as const) {
      const hits = css.split("\n").filter((l) => /font-weight:\s*(450|475|650)\b/.test(l));
      expect(hits, name).toEqual([]);
    }
  });
});

describe("对话窗焦点环", () => {
  // 柔光 accent-soft 合成后只有 1.11:1（调研 i2）；实线 accent 深 5.36:1、浅 3.67:1。
  it("--focus-ring 是实线 accent，不用 accent-soft", () => {
    const decl = styles.match(/--focus-ring:\s*([^;]+);/)?.[1] ?? "";
    expect(decl).toMatch(/var\(--cc-accent\)/);
    expect(decl).not.toMatch(/accent-soft/);
  });
  // 指针/触屏模态下程序化移焦不画环：画环的规则必须挂在 data-im="kbd" 下。
  it("box-shadow 焦点环只在键盘模态生效", () => {
    const lines = styles.split("\n").filter((l) => /:focus-visible[^{]*\{[^}]*box-shadow:\s*var\(--focus-ring/.test(l));
    expect(lines.length).toBeGreaterThan(0);
    for (const l of lines) expect(l).toMatch(/^:root\[data-im="kbd"\]/);
  });
});

describe("状态色当文字用走文字档", () => {
  // 浅色鲜亮色在白底只有 2.45～4.26:1（调研 i3），文字一律用 --cc-ok/warn/fail-text。
  // 例外：.obm-term 压在写死的 #16161a 深底上，两主题都是深底。
  const ALLOW = [".obm-term code"];
  it("color 属性不直接取 --cc-*-vivid", () => {
    const hits = styles.split("\n").filter((l) => /(?<![-\w])color:\s*var\(--cc-(ok|warn|err)-vivid\)/.test(l) && !ALLOW.some((a) => l.includes(a)));
    expect(hits).toEqual([]);
  });
  it("文字档在深色、浅色、终端视图三处都有定义", () => {
    for (const tok of ["--cc-ok-text", "--cc-warn-text", "--cc-fail-text"]) {
      const n = styles.split("\n").filter((l) => l.trimStart().startsWith(tok + ":")).length;
      expect(n, tok).toBe(3);
    }
  });
  // 写死 #fff 压 #1a88ff 只有 3.49:1（调研 sys3）。
  it("提交与会话管理主按钮字色走 --cc-on-accent", () => {
    for (const sel of [".scm-commit-btn", ".sa-btn-q--primary"]) {
      const line = styles.split("\n").find((l) => l.startsWith(sel + " {")) ?? "";
      expect(line, sel).toMatch(/color: var\(--cc-on-accent\)/);
    }
  });
});
