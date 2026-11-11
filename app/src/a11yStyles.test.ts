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
