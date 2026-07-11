// 录制 demo.webp:起独立端口的 vite → Playwright 以 2x(deviceScaleFactor)逐帧 seek+截图
// → sharp 合成无损/高质量动画 WebP。相比旧的 256 色 GIF,WebP 无调色板色带、文字边缘在 2x 下
// 像素级锐利,且体积通常更小。
// 用法:cd app && bun run demo:webp   (实际由 node 执行——Playwright 在 Windows 下需要 node
// 的多 fd 子进程管道,bun 暂不支持;产物写到 ../docs/images/demo.webp)
// 可调环境变量:
//   CC_WEBP_QUALITY=100  有损质量(1–100)。默认 100:有损动画 WebP 的帧间混合,在窗口大幅
//                        收起后空出区域会留「残影」,质量越高残影越淡,q100 时几乎不可见;
//                        更低质量(如 90)残影明显。真正零残影需 CC_WEBP_LOSSLESS=1(但约 20MB)。
//   CC_WEBP_LOSSLESS=1   无损编码(零残影、像素级完美,但体积巨大 ~20MB)。
//   CC_WEBP_EFFORT=5     压缩耗时/体积权衡(0–6),越高越小越慢。
//   CC_KEEP_FRAMES=1     编码后保留 ../target/demo-frames 的逐帧 PNG(便于换参数重编,免重录)。
import { spawn } from "node:child_process";
import { mkdirSync, rmSync, statSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import sharp from "sharp";

const appDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outPath = resolve(appDir, "../docs/images/demo.webp");
const framesDir = resolve(appDir, "../target/demo-frames"); // 帧落盘,避免全量帧驻留内存
const PORT = 14210;
const W = 880, H = 600; // CSS 逻辑尺寸;实际截图为 2×(deviceScaleFactor)
const SCALE = 2;
const LOSSLESS = process.env.CC_WEBP_LOSSLESS === "1";
const QUALITY = Number(process.env.CC_WEBP_QUALITY || 100); // 默认 100:把帧间混合残影压到几乎不可见
const EFFORT = Number(process.env.CC_WEBP_EFFORT || 5);

// vite 可能已在跑(开发中):先探测,没有再自己起。
let vite = null;
if (!(await ping(`http://localhost:${PORT}/demo.html`))) {
  vite = spawn("bun", ["x", "vite", "--port", String(PORT), "--strictPort"], {
    cwd: appDir,
    stdio: "pipe",
    shell: true,
  });
}
try {
  await waitFor(`http://localhost:${PORT}/demo.html`);
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: W, height: H }, deviceScaleFactor: SCALE });
  page.on("console", (m) => {
    if (m.text().includes("[demo]")) console.warn("页面警告:", m.text());
  });
  await page.goto(`http://localhost:${PORT}/demo.html`);
  await page.waitForFunction(() => !!window.__demo, null, { timeout: 15000 });
  const frames = await page.evaluate(() => window.__demo.frames);
  const fps = await page.evaluate(() => window.__demo.fps);
  const DELAY_MS = Math.round(1000 / fps); // 帧延时跟随 demo 帧率
  console.log(`录制 ${frames} 帧 @${W * SCALE}x${H * SCALE}(${SCALE}x, ${fps}fps)`);

  // 逐帧 seek + 截图落盘。sharp 之后按文件路径惰性读取,内存占用与帧数无关。
  rmSync(framesDir, { recursive: true, force: true });
  mkdirSync(framesDir, { recursive: true });
  const paths = [];
  for (let f = 0; f < frames; f++) {
    await page.evaluate((n) => window.__demo.seek(n), f);
    const p = resolve(framesDir, `f${String(f).padStart(4, "0")}.png`);
    await page.screenshot({ path: p });
    paths.push(p);
    if (f % 24 === 0) console.log(`  ${f}/${frames}`);
  }
  await browser.close();

  // 合成动画 WebP:join.animated 把帧序列拼成多页动图,每页统一帧延时、无限循环。
  console.log(`编码 WebP(${LOSSLESS ? "无损" : `质量 ${QUALITY}`},effort ${EFFORT})…`);
  mkdirSync(dirname(outPath), { recursive: true });
  // delay 必须逐帧给数组:sharp 传单个数字只作用于首帧、其余回退 100ms(≈10fps)。
  await sharp(paths, { join: { animated: true } })
    .webp({ loop: 0, delay: paths.map(() => DELAY_MS), lossless: LOSSLESS, quality: QUALITY, effort: EFFORT, smartSubsample: true })
    .toFile(outPath);
  if (process.env.CC_KEEP_FRAMES !== "1") rmSync(framesDir, { recursive: true, force: true });

  console.log(`完成:${outPath} (${(statSync(outPath).size / 1024 / 1024).toFixed(2)} MB)`);
} finally {
  vite?.kill();
}

async function ping(url) {
  try {
    const r = await fetch(url);
    return r.ok;
  } catch {
    return false;
  }
}

async function waitFor(url) {
  for (let i = 0; i < 60; i++) {
    if (await ping(url)) return;
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error("vite dev server 启动超时");
}
