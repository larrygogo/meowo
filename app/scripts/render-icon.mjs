// 把矢量母图 icon.svg 渲成指定尺寸的 PNG（Playwright 无头 Chromium，透明背景保留圆角外的镂空）。
// 用法：node app/scripts/render-icon.mjs [svg] [out.png] [size]
// 注意：Playwright 必须用 node 跑（bun 不兼容其原生绑定）。
import { chromium } from "playwright";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const dir = path.dirname(fileURLToPath(import.meta.url));
const svgPath = process.argv[2] || path.join(dir, "../src-tauri/icons/icon.svg");
const outPath = process.argv[3] || path.join(dir, "../src-tauri/icons/icon-master.png");
const size = Number(process.argv[4] || 1024);

const svg = readFileSync(svgPath, "utf8");
const html = `<!doctype html><html><head><meta charset="utf-8">
<style>*{margin:0;padding:0}html,body{background:transparent}svg{display:block}</style>
</head><body>${svg}</body></html>`;

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: size, height: size }, deviceScaleFactor: 1 });
await page.setContent(html, { waitUntil: "networkidle" });
await page.evaluate((s) => {
  const el = document.querySelector("svg");
  el.setAttribute("width", String(s));
  el.setAttribute("height", String(s));
}, size);
const buf = await page.screenshot({ omitBackground: true, clip: { x: 0, y: 0, width: size, height: size } });
writeFileSync(outPath, buf);
await browser.close();
console.log("wrote", outPath, `${size}x${size}`);
