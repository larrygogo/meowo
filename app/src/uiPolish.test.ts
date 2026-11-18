// 界面细节对标（.lead/20261118-ui-polish）的样式守卫：暗色字重、断行、回弹、阴影替代边框、段落渐显。
// 这些都是「改一个 token、删一行就悄悄回退」的约束，用源码扫描钉住；视觉效果另有前后对比截图（progress.md）。
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const strip = (css: string) => css.replace(/\/\*[\s\S]*?\*\//g, "");
const styles = strip(readFileSync(join(here, "styles.css"), "utf8"));
const mobile = strip(readFileSync(join(here, "mobile/mobile.css"), "utf8"));
/** 取某个选择器（整段精确匹配）第一次出现的规则体。 */
const rule = (css: string, selector: string) => {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const m = new RegExp(String.raw`(?:^|[\n}])\s*${escaped} \{([^}]*)\}`).exec(css);
  return m ? m[1] : "";
};
const tokenIn = (body: string, name: string) => body.match(new RegExp(`${name}:\\s*([^;]+);`))?.[1]?.trim();

describe("U1 暗色字重降一档", () => {
  // 根元素默认是深色：字重四档写在带 --ind-face 的那个 :root 块末尾。
  const darkBlock = styles.slice(styles.indexOf("--ind-face: #000;"), styles.indexOf("}", styles.indexOf("--ind-face: #000;")));
  const light = rule(styles, ':root[data-theme="light"]');
  const names = ["--fw-regular", "--fw-medium", "--fw-semibold", "--fw-bold"];
  it("深色（根元素默认）每档都比浅色低，浅色是 400/500/600/700", () => {
    const d = names.map((n) => Number(tokenIn(darkBlock, n)));
    const l = names.map((n) => Number(tokenIn(light, n)));
    expect(l).toEqual([400, 500, 600, 700]);
    d.forEach((value, index) => {
      expect(value).toBeLessThan(l[index]);
      expect(l[index] - value).toBeLessThanOrEqual(50); // 一档，不是两档
    });
  });
  it("样式表里不再有字面 400/500/600/700 字重（灵动岛与 @font-face 除外）", () => {
    for (const [name, css] of [["styles.css", styles], ["mobile.css", mobile]] as const) {
      const hits: string[] = [];
      for (const m of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
        if (/@font-face|\.isl|\.island/.test(m[1])) continue;
        if (/font-weight:\s*(400|500|600|700)\b/.test(m[2])) hits.push(m[1].trim().slice(0, 60));
      }
      expect(hits, name).toEqual([]);
    }
  });
  it("灵动岛恒黑，字重钉回 400 档，不吃深色降档", () => {
    const island = rule(styles, ".isl-stage, .island-shell, .isl-strip");
    expect(tokenIn(island, "--fw-regular")).toBe("400");
    expect(tokenIn(island, "font-weight")).toBe("400");
  });
  it("body 与 UA 粗体元素都接到 token 上", () => {
    expect(styles).toMatch(/:where\(strong, b, th, h1, h2, h3, h4, h5, h6\) \{ font-weight: var\(--fw-bold\); \}/);
  });
});

describe("U2 断行", () => {
  it("正文 pretty、标题 balance，只写 text-wrap-style 长属性（不用会盖掉 nowrap 的 text-wrap 简写）", () => {
    expect(styles).toMatch(/html, body \{ text-wrap-style: pretty; \}/);
    expect(styles).toMatch(/:where\(h1, h2, h3, h4, h5, h6[^)]*\) \{ text-wrap-style: balance; \}/);
    const added = [...styles.matchAll(/text-wrap:\s*(pretty|balance)/g)].length;
    expect(added).toBe(2); // 改版前就有的两处 balance，不再新增简写
  });
  it("代码、终端、表格、输入框退回 auto", () => {
    const reset = styles.match(/:where\(([^)]*)\) \{ text-wrap-style: auto; \}/)?.[1] ?? "";
    for (const sel of ["pre", "code", "table", "textarea", "input", ".xterm"]) expect(reset.split(/,\s*/)).toContain(sel);
  });
});

describe("U3 即时反馈回弹", () => {
  it("按下缩到 .92、成功弹回，时长取 --dur-slow（≤300ms）且带弹性缓动", () => {
    expect(styles).toMatch(/\.fb-press:active \{ scale: 0\.92; \}/);
    const flash = rule(styles, ".fb-flash");
    expect(flash).toMatch(/var\(--dur-slow\) var\(--ease-spring\)/);
    const slow = Number(tokenIn(styles, "--dur-slow")?.replace("s", ""));
    expect(slow * 1000).toBeLessThanOrEqual(300);
  });
  it("减少动效时关闭", () => {
    const block = styles.slice(styles.indexOf(".fb-press:active { scale: none; }") - 60, styles.indexOf(".fb-press:active { scale: none; }") + 80);
    expect(block).toMatch(/prefers-reduced-motion: reduce/);
    expect(block).toMatch(/\.fb-flash \{ animation: none; \}/);
  });
});

describe("U4 阴影替代边框", () => {
  const darkTokens = styles.slice(0, styles.indexOf("\n:root {"));
  const light = rule(styles, ':root[data-theme="light"]');
  it("--shadow-card 与 --edge-float 深浅两套都定义", () => {
    for (const name of ["--shadow-card", "--edge-float", "--shadow-menu"]) {
      expect(tokenIn(darkTokens, name), `深色 ${name}`).toBeTruthy();
      expect(tokenIn(light, name), `浅色 ${name}`).toBeTruthy();
    }
  });
  it("卡片、浮层、菜单、弹窗不再画 --cc-border 实线外框", () => {
    const outlined = [".ctx-menu, .dd-menu, .relay-model-menu, .tip", ".row-card", ".chat-turn-changes", ".scm-pr", ".chat-modal", ".chat-sheet-panel",
      ".chat-model-picker", ".chat-todos-panel", ".dock-work-panel", ".chat-search", ".sa-toast", ".chat-crossmsg", ".chat-compact-summary"];
    for (const sel of outlined) {
      const body = rule(styles, sel);
      expect(body, sel).not.toBe("");
      expect(body, sel).not.toMatch(/border:\s*var\(--hairline\) solid var\(--cc-border\)/);
      expect(body, sel).not.toMatch(/0 0 0 var\(--hairline\) var\(--cc-border\)/);
    }
    for (const sel of [".remote-confirm", ".remote-gate-card", ".remote-settings-panel", ".remote-settings-screen .remote-settings-card"]) {
      const body = rule(mobile, sel);
      expect(body, sel).not.toBe("");
      expect(body, sel).not.toMatch(/var\(--hairline\)\)? ?(solid )?var\(--cc-border\)/);
    }
  });
  it("输入框与按钮保留细线（可用性需要）", () => {
    expect(rule(styles, ".ns-input")).toMatch(/border: var\(--hairline\) solid var\(--cc-border\)/);
    expect(rule(styles, ".sbtn")).toMatch(/border: var\(--hairline\) solid/);
  });
  it("手机列表选中项只用底色与阴影，不画高亮边", () => {
    expect(mobile).toMatch(/\.remote-session-list \.chat-sidebar-tabs button\.is-active \{[^}]*box-shadow: var\(--shadow-sm\);/);
  });
});

describe("U6 段落渐显", () => {
  it("只动 opacity 与 filter，约 200ms，减少动效时关闭", () => {
    const frames = styles.slice(styles.indexOf("@keyframes chat-enter"), styles.indexOf("}", styles.indexOf("to {", styles.indexOf("@keyframes chat-enter"))) + 1);
    const props = [...frames.matchAll(/([a-z-]+):/g)].map((m) => m[1]);
    expect(new Set(props)).toEqual(new Set(["opacity", "filter"]));
    expect(frames).toMatch(/blur\(3px\)/);
    expect(tokenIn(styles, "--dur-enter")).toBe("0.2s");
    expect(styles).toMatch(/@media \(prefers-reduced-motion: reduce\) \{ \.is-entering \{ animation: none; \} \}/);
  });
});
