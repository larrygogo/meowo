// 调试用:截 demo.html 指定帧到临时目录。用法:bun scripts/snap-frame.mjs 0 12 60 ...
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { tmpdir } from "node:os";
import { chromium } from "playwright";

const frames = process.argv.slice(2).map(Number);
if (frames.length === 0) frames.push(0);
const outDir = resolve(tmpdir(), "cc-demo-frames");
mkdirSync(outDir, { recursive: true });

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 880, height: 560 }, deviceScaleFactor: 1 });
page.on("console", (m) => {
  if (m.text().includes("[demo]")) console.warn("页面警告:", m.text());
});
await page.goto("http://localhost:14210/demo.html");
await page.waitForFunction(() => !!window.__demo, null, { timeout: 15000 });
const total = await page.evaluate(() => window.__demo.frames);
console.log(`总帧数 ${total}`);
let last = -1;
for (const f of frames.sort((a, b) => a - b)) {
  for (let i = last + 1; i <= f; i++) await page.evaluate((n) => window.__demo.seek(n), i);
  last = f;
  const p = resolve(outDir, `f${String(f).padStart(3, "0")}.png`);
  await page.screenshot({ path: p });
  console.log(p);
}
await browser.close();
