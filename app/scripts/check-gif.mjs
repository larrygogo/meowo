// 调试用:在 chromium 里实际播放 demo.webp,按秒截图验证动图渲染正确。
import { resolve, dirname } from "node:path";
import { tmpdir } from "node:os";
import { mkdirSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright";

const appDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const gif = pathToFileURL(resolve(appDir, "../docs/images/demo.webp")).href;
const outDir = resolve(tmpdir(), "cc-demo-gifcheck");
mkdirSync(outDir, { recursive: true });

const secs = process.argv.slice(2).map(Number);
if (secs.length === 0) secs.push(0.5, 3, 6.8, 10.2, 12.5, 15.1, 16, 19);

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 880, height: 560 } });
// about:blank 页禁止引用 file:// 子资源 → 直接打开 GIF 本体(chromium 以图片文档播放)。
await page.goto(gif);
let elapsed = 0;
for (const s of secs.sort((a, b) => a - b)) {
  await page.waitForTimeout((s - elapsed) * 1000);
  elapsed = s;
  const p = resolve(outDir, `t${String(s).replace(".", "_")}.png`);
  await page.screenshot({ path: p });
  console.log(p);
}
await browser.close();
