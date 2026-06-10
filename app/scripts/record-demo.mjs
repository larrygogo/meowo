// 录制 demo.gif:起独立端口的 vite → Playwright 逐帧 seek+截图 → gifenc 编码。
// 用法:cd app && bun run demo:gif   (实际由 node 执行——Playwright 在 Windows 下需要
// node 的多 fd 子进程管道,bun 暂不支持;产物写到 ../docs/images/demo.gif)
import { spawn } from "node:child_process";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import gifencPkg from "gifenc"; // CJS 包,node ESM 下走 default 解构
const { GIFEncoder, quantize, applyPalette } = gifencPkg;
import { decode } from "fast-png";

const appDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outPath = resolve(appDir, "../docs/images/demo.gif");
const PORT = 14210;
const W = 880, H = 560;
const DELAY_MS = 83; // ≈12fps
const TRANSPARENT = 255; // 调色板末位留给「与上帧相同」的像素

// vite 可能已在跑(开发中):先探测,没有再自己起。
let vite = null;
if (!(await ping(`http://localhost:${PORT}/demo.html`))) {
  vite = spawn("bunx", ["vite", "--port", String(PORT), "--strictPort"], {
    cwd: appDir,
    stdio: "pipe",
    shell: true,
  });
}
try {
  await waitFor(`http://localhost:${PORT}/demo.html`);
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: W, height: H }, deviceScaleFactor: 1 });
  page.on("console", (m) => {
    if (m.text().includes("[demo]")) console.warn("页面警告:", m.text());
  });
  await page.goto(`http://localhost:${PORT}/demo.html`);
  await page.waitForFunction(() => !!window.__demo, null, { timeout: 15000 });
  const frames = await page.evaluate(() => window.__demo.frames);
  console.log(`录制 ${frames} 帧 @${W}x${H}`);

  // 逐帧 seek + 截图,RGBA 暂存内存(880×560×4×240 ≈ 470MB,可接受)。
  const rgbaFrames = [];
  for (let f = 0; f < frames; f++) {
    await page.evaluate((n) => window.__demo.seek(n), f);
    const png = await page.screenshot({ type: "png" });
    if (process.env.CC_GIF_DUMP && (f === 6 || f === 80)) {
      writeFileSync(resolve(appDir, `../target/debug-frame-${f}.png`), png);
    }
    const { data, width, height, channels } = decode(png);
    if (width !== W || height !== H) throw new Error(`帧尺寸异常: ${width}x${height}`);
    rgbaFrames.push(toRGBA(data, width, height, channels));
    if (f % 24 === 0) console.log(`  ${f}/${frames}`);
  }
  await browser.close();

  // 全局调色板:每 12 帧采样合并量化(255 色),末位补占位色给透明索引。
  const samples = [];
  for (let f = 0; f < rgbaFrames.length; f += 12) samples.push(rgbaFrames[f]);
  const merged = new Uint8Array(samples.length * samples[0].length);
  samples.forEach((s, i) => merged.set(s, i * s.length));
  const palette = quantize(merged, 255);
  while (palette.length < 256) palette.push([0, 0, 0]);

  // 帧间差分:与上帧索引相同的像素写透明位(dispose=1 保留上帧),大幅压体积。
  // CC_GIF_NODIFF=1 时关闭差分(全帧编码),用于诊断。
  const NODIFF = process.env.CC_GIF_NODIFF === "1";
  const gif = GIFEncoder();
  let prev = null;
  for (const rgba of rgbaFrames) {
    const index = applyPalette(rgba, palette);
    if (!prev || NODIFF) {
      gif.writeFrame(index, W, H, { palette, delay: DELAY_MS });
    } else {
      const diff = new Uint8Array(index);
      for (let i = 0; i < index.length; i++) if (index[i] === prev[i]) diff[i] = TRANSPARENT;
      gif.writeFrame(diff, W, H, {
        palette,
        delay: DELAY_MS,
        transparent: true,
        transparentIndex: TRANSPARENT,
        dispose: 1,
      });
    }
    prev = index;
  }
  gif.finish();
  mkdirSync(dirname(outPath), { recursive: true });
  const bytes = gif.bytes();
  writeFileSync(outPath, bytes);
  console.log(`完成:${outPath} (${(bytes.length / 1024 / 1024).toFixed(2)} MB)`);
} finally {
  vite?.kill();
}

// gifenc 假定 RGBA 平铺;Playwright 截图是不带 alpha 的 RGB(3 通道),需扩成 4 通道。
function toRGBA(data, w, h, channels) {
  if (channels === 4) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  if (channels !== 3) throw new Error(`不支持的通道数: ${channels}`);
  const out = new Uint8Array(w * h * 4);
  for (let i = 0, j = 0; i < data.length; i += 3, j += 4) {
    out[j] = data[i];
    out[j + 1] = data[i + 1];
    out[j + 2] = data[i + 2];
    out[j + 3] = 255;
  }
  return out;
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
