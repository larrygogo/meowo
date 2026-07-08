// 生成 macOS 菜单栏空闲 logo 的单色模板：app/src-tauri/icons/menubar-template.rgba
// 三柱几何等比复刻自 icon.svg（保持基线对齐、统一圆角、等间距），缩放进 36×36；RGB 清零、只留 alpha
// 掩码（macOS 模板图按菜单栏明暗自动反色）。行优先、上到下，输出 36*36*4 = 5184 字节裸 RGBA。
// 另输出一张 12x 放大预览 PNG（menubar-preview.png）供肉眼检查剪影。
// 注意：Playwright 必须用 node 跑。真机效果需在 macOS 上确认。
import { chromium } from "playwright";
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";
import path from "node:path";

const dir = path.dirname(fileURLToPath(import.meta.url));
const iconsDir = path.join(dir, "../src-tauri/icons");

const SIZE = 36; // 模板边长（与现有资产一致）
const PAD = 3; // 水平留白（像素，控制整体大小，间距随之等比）
const SS = 8; // 超采样倍数，抗锯齿

// icon.svg 的三柱几何（逻辑单位，1024 画布）：x / 顶 y / 宽 / 高，底部基线统一 740。
const LOGO_BARS = [
  { x: 217, y: 380, w: 150, h: 360 }, // 绿
  { x: 437, y: 490, w: 150, h: 250 }, // 琥珀
  { x: 657, y: 300, w: 150, h: 440 }, // 陶土
];
const LOGO_RX = 42;
// 三柱整体包围盒
const BX0 = Math.min(...LOGO_BARS.map((b) => b.x));
const BX1 = Math.max(...LOGO_BARS.map((b) => b.x + b.w));
const BY0 = Math.min(...LOGO_BARS.map((b) => b.y));
const BY1 = Math.max(...LOGO_BARS.map((b) => b.y + b.h));
const scale = (SIZE - 2 * PAD) / (BX1 - BX0); // 按宽度铺满（块更宽）
const topMargin = (SIZE - (BY1 - BY0) * scale) / 2; // 竖直居中

const bars = LOGO_BARS.map((b) => ({
  x: PAD + (b.x - BX0) * scale,
  y: topMargin + (b.y - BY0) * scale,
  w: b.w * scale,
  h: b.h * scale,
}));
const radius = LOGO_RX * scale;

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: SIZE, height: SIZE } });

const rgba = await page.evaluate(
  ({ SIZE, SS, bars, radius }) => {
    // 超采样画黑柱 → 高质量缩小到 SIZE
    const hi = document.createElement("canvas");
    hi.width = SIZE * SS;
    hi.height = SIZE * SS;
    const h = hi.getContext("2d");
    h.scale(SS, SS);
    h.fillStyle = "#000";
    for (const b of bars) {
      h.beginPath();
      h.roundRect(b.x, b.y, b.w, b.h, radius);
      h.fill();
    }
    const lo = document.createElement("canvas");
    lo.width = SIZE;
    lo.height = SIZE;
    const l = lo.getContext("2d");
    l.imageSmoothingEnabled = true;
    l.imageSmoothingQuality = "high";
    l.drawImage(hi, 0, 0, SIZE, SIZE);
    return Array.from(l.getImageData(0, 0, SIZE, SIZE).data);
  },
  { SIZE, SS, bars, radius }
);

// RGB 清零、保留 alpha（模板图只用 alpha）
const buf = Buffer.alloc(SIZE * SIZE * 4);
for (let i = 0; i < SIZE * SIZE; i++) {
  buf[i * 4 + 3] = rgba[i * 4 + 3];
}
writeFileSync(path.join(iconsDir, "menubar-template.rgba"), buf);

// 放大预览：把 alpha 当灰度画在浅色底上，便于肉眼检查剪影
const previewPng = await page.evaluate(
  async ({ SIZE, alpha }) => {
    const Z = 12;
    const c = document.createElement("canvas");
    c.width = SIZE * Z;
    c.height = SIZE * Z;
    const ctx = c.getContext("2d");
    ctx.fillStyle = "#cfcfcf";
    ctx.fillRect(0, 0, c.width, c.height);
    for (let y = 0; y < SIZE; y++) {
      for (let x = 0; x < SIZE; x++) {
        const a = alpha[(y * SIZE + x) * 4 + 3] / 255;
        if (a > 0) {
          ctx.fillStyle = `rgba(20,20,22,${a})`;
          ctx.fillRect(x * Z, y * Z, Z, Z);
        }
      }
    }
    const blob = await new Promise((r) => c.toBlob(r, "image/png"));
    const ab = await blob.arrayBuffer();
    return Array.from(new Uint8Array(ab));
  },
  { SIZE, alpha: rgba }
);
const previewPath = path.join(tmpdir(), "meowo-menubar-preview.png");
writeFileSync(previewPath, Buffer.from(previewPng));

await browser.close();
const nonZero = buf.filter((_, i) => i % 4 === 3 && buf[i] > 0).length;
console.log(`menubar-template.rgba: ${buf.length} bytes (期望 5184), 非零 alpha 像素=${nonZero}`);
console.log(`预览图（仅供肉眼检查，不入库）: ${previewPath}`);
