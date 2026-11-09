// 官网吉祥物：把应用里真实的灵动岛眼睛（src/views/island/islandParts.tsx 的 Eyes）打包成
// 一个自定义元素 <meowo-eyes>，输出到 site/public/mascot/meowo-eyes.js，官网用 <script type="module"> 引。
// 用法：cd app && bun x vite build -c vite.mascot.config.ts（demo:build 会一并跑）
//
// 眼睛的样式在 styles.css 里（500KB+，整份带过去不现实）。这里在构建时只挑出眼睛用到的规则和
// isl-* 动画，作为虚拟模块 virtual:mascot-css 交给 mascot.tsx 塞进 Shadow DOM。应用里改了眼睛
// 的样式，官网重新构建就跟上，不另抄一份。
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

/** 眼睛 SVG 与它身边小符号（+ ! z 汗滴 腮红…）用到的类名前缀。 */
const KEEP = [".isl-eye", ".isl-spark", ".isl-sclera", ".isl-pupil", ".isl-glint", ".isl-cheeks", ".isl-spiral", ".isl-sweat", ".isl-zz", ".isl-tear"];

/** 按顶层大括号把 CSS 切成规则块（选择器 + 块体），注释先去掉。styles.css 没有 @media 包着眼睛规则。 */
function topLevelBlocks(css: string): { head: string; text: string }[] {
  const src = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const out: { head: string; text: string }[] = [];
  let start = 0;
  let depth = 0;
  let open = -1;
  for (let i = 0; i < src.length; i++) {
    const ch = src[i];
    if (ch === "{") {
      if (depth === 0) open = i;
      depth++;
    } else if (ch === "}") {
      depth--;
      if (depth === 0) {
        out.push({ head: src.slice(start, open).trim(), text: src.slice(start, i + 1).trim() });
        start = i + 1;
      }
    }
  }
  return out;
}

function mascotCss(): Plugin {
  const id = "virtual:mascot-css";
  return {
    name: "mascot-css",
    resolveId: (source) => (source === id ? "\0" + id : null),
    load(loaded) {
      if (loaded !== "\0" + id) return null;
      const css = readFileSync(resolve(__dirname, "src/styles.css"), "utf8");
      const blocks = topLevelBlocks(css);
      // 灵动岛的色板变量（--isl-ok / --isl-warn …）挂在 .isl-stage 上，这里改挂到 :host。
      const vars = blocks.find((b) => b.head.includes(".isl-stage"));
      const picked = blocks.filter((b) =>
        b.head.startsWith("@keyframes isl-") || (!b.head.startsWith("@") && KEEP.some((k) => b.head.includes(k))),
      );
      const host = vars ? `:host ${vars.text.slice(vars.text.indexOf("{"))}` : "";
      // 系统开了「减少动态效果」时岛上的眼睛不弹不抖（styles.css 里包在 @media 里），官网照办：
      // 从这类 @media 块里再挑出眼睛的规则，原样包回去（做空审查发现第一版漏了它）。
      const reduced = blocks
        .filter((b) => b.head.startsWith("@media") && b.head.includes("prefers-reduced-motion"))
        .map((b) => {
          const inner = topLevelBlocks(b.text.slice(b.text.indexOf("{") + 1, b.text.lastIndexOf("}")))
            .filter((r) => KEEP.some((k) => r.head.includes(k)));
          return inner.length ? `${b.head} {\n${inner.map((r) => r.text).join("\n")}\n}` : "";
        })
        .filter(Boolean);
      return `export default ${JSON.stringify([host, ...picked.map((b) => b.text), ...reduced].join("\n"))};`;
    },
  };
}

export default defineConfig({
  plugins: [react(), mascotCss()],
  define: { "process.env.NODE_ENV": JSON.stringify("production") },
  build: {
    outDir: resolve(__dirname, "../site/public/mascot"),
    emptyOutDir: true,
    // 体积大头是 React 运行时（gzip 约 57KB），官网用 type="module" 异步加载，不挡首屏。
    lib: {
      entry: resolve(__dirname, "src/demo/mascot.tsx"),
      formats: ["es"],
      fileName: () => "meowo-eyes.js",
    },
  },
});
