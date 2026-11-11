import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative } from "node:path";
import { Icon, ICON_STROKE_PX, type IconSize } from "./Icon";
import * as core from "./glyphs";
import * as chat from "./glyphs.chat";
import * as scm from "./glyphs.scm";
import * as settings from "./glyphs.settings";

const SRC = join(dirname(fileURLToPath(import.meta.url)), "..");

/**
 * 组件目录外还允许手写 <svg> 的地方，每条写明为什么不走 <Icon>。
 * 新增内联 svg 会让下面的扫描失败：要么改用 Icon，要么在这里登记理由。
 */
const SVG_EXCEPTIONS: Record<string, string> = {
  "providers.tsx": "各家 CLI 的品牌徽标：填充型、自带底色，形状和颜色由品牌决定",
  "views/CollapsedStrip.tsx": "收起细条上的一对眼睛：灵动岛形象的一部分，实心填充",
  "views/island/islandParts.tsx": "灵动岛眼睛表情与用量圆环：动画形象和数据驱动的圆环，线宽按岛的比例单独设计",
  "views/chat/ContextMeter.tsx": "上下文占用圆环：dasharray 随数据变化的进度图形",
  "views/chat/LeadPanel.tsx": "关卡进度圆环：dasharray 随数据变化的进度图形",
  "views/chat/TodoBadge.tsx": "实心圆徽章里的记号：尺寸跟徽章（8/10px）走，套 12px 会顶出圆框",
  "demo/DemoStage.tsx": "演示页里模拟的鼠标指针：白底黑边的填充图形，不进应用包",
  "demo/IslandStage.tsx": "演示页里模拟的鼠标指针：白底黑边的填充图形，不进应用包",
};

function walk(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (/\.tsx$/.test(name) && !/\.test\.tsx$/.test(name)) out.push(p);
  }
  return out;
}

const files = walk(SRC).map((p) => ({ rel: relative(SRC, p).replace(/\\/g, "/"), text: readFileSync(p, "utf8") }));
// 只认 JSX 标签：行首或 ( { > 之后紧跟 <svg，注释里提到的 <svg> 字样不算。
const SVG_TAG = /(?:^|[\s({>?:&])<svg[\s>]/m;

describe("图标统一", () => {
  it("icons/ 外的内联 svg 都在例外清单里", () => {
    const offenders = files
      .filter((f) => !f.rel.startsWith("icons/") && SVG_TAG.test(f.text.replace(/\{\/\*[\s\S]*?\*\/\}/g, "")))
      .map((f) => f.rel)
      .filter((rel) => !(rel in SVG_EXCEPTIONS));
    expect(offenders).toEqual([]);
  });

  it("例外清单里没有过期条目", () => {
    const stale = Object.keys(SVG_EXCEPTIONS).filter((rel) => {
      const f = files.find((x) => x.rel === rel);
      return !f || !SVG_TAG.test(f.text);
    });
    expect(stale).toEqual([]);
  });

  it("<Icon> 的字面尺寸只用 12/14/16/20", () => {
    const bad: string[] = [];
    for (const f of files) {
      for (const m of f.text.matchAll(/<Icon\b[^>]*?\bsize=\{(\d+)\}/g)) {
        if (![12, 14, 16, 20].includes(Number(m[1]))) bad.push(`${f.rel}: ${m[1]}`);
      }
    }
    expect(bad).toEqual([]);
  });

  it("每个图形只在一个注册表里出现一次（名字与形状都不重复）", () => {
    const all = [core, chat, scm, settings].flatMap((mod) => Object.keys(mod));
    expect(new Set(all).size).toBe(all.length);
    const shapes = [core, chat, scm, settings].flatMap((mod) =>
      Object.values(mod).map((g) => render(<svg>{g.body}</svg>).container.innerHTML),
    );
    expect(new Set(shapes).size).toBe(shapes.length);
  });

  it.each([12, 14, 16, 20] as IconSize[])("%ipx 下有效线宽是 1.25px", (size) => {
    const { container } = render(<Icon glyph={core.icClose} size={size} />);
    const svg = container.querySelector("svg")!;
    const sw = Number(svg.getAttribute("stroke-width"));
    expect((sw * size) / 24).toBeCloseTo(ICON_STROKE_PX, 5);
    expect(svg.style.width).toBe(`${size}px`);
    expect(svg.getAttribute("aria-hidden")).toBe("true");
  });

  it("给 label 时按有意义的图形读出，不再 aria-hidden", () => {
    const { container } = render(<Icon glyph={core.icCheck} size={12} label="通过" />);
    const svg = container.querySelector("svg")!;
    expect(svg.getAttribute("role")).toBe("img");
    expect(svg.getAttribute("aria-label")).toBe("通过");
    expect(svg.hasAttribute("aria-hidden")).toBe(false);
  });

  it("分档注册表不被拖进上游 chunk", () => {
    // 一个模块整体落在所有引用者的共同上游 chunk：入口一旦静态 import glyphs.chat，
    // 对话窗的几十个图形就全进了入口包（2026-11-11 包体积预算 #56）。
    const reach = (entry: string) => {
      const seen = new Set<string>();
      const queue = [entry];
      while (queue.length) {
        const rel = queue.pop()!;
        if (seen.has(rel)) continue;
        seen.add(rel);
        const f = files.find((x) => x.rel === rel) ?? { text: "" };
        for (const m of f.text.matchAll(/^\s*import\s+(?!type\b)[^;]*?from\s+"(\.[^"]+)"/gm)) {
          const base = join(dirname(join(SRC, rel)), m[1]);
          const hit = [`${base}.tsx`, `${base}.ts`, join(base, "index.tsx")]
            .map((p) => relative(SRC, p).replace(/\\/g, "/"))
            .find((p) => files.some((x) => x.rel === p));
          if (hit) queue.push(hit);
        }
      }
      return seen;
    };
    const entry = reach("main.tsx");
    expect(entry.has("icons/glyphs.tsx")).toBe(true);
    for (const m of ["icons/glyphs.chat.tsx", "icons/glyphs.scm.tsx", "icons/glyphs.settings.tsx"]) expect(entry.has(m), m).toBe(false);
    const chatWindow = reach("views/ChatWindow.tsx");
    expect(chatWindow.has("icons/glyphs.chat.tsx")).toBe(true);
    for (const m of ["icons/glyphs.scm.tsx", "icons/glyphs.settings.tsx"]) expect(chatWindow.has(m), m).toBe(false);
  });

  it("实心图形不描边", () => {
    const { container } = render(<Icon glyph={core.icMore} size={16} />);
    const svg = container.querySelector("svg")!;
    expect(svg.getAttribute("fill")).toBe("currentColor");
    expect(svg.hasAttribute("stroke-width")).toBe(false);
  });
});
