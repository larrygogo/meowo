// 生成 NSIS 安装向导用的位图：顶部横幅 nsis-header.bmp(150x57) 与欢迎/结束页侧图
// nsis-sidebar.bmp(164x314)。NSIS MUI 只吃 BMP，而 Playwright 只出 PNG——故在 Chromium 里
// 用 canvas 合成构图、getImageData 取 RGBA，再由 node 手写 24-bit BMP（不引额外图像依赖）。
// 顺带各输出一份 .preview.png 供肉眼校对。
// 注意：Playwright 必须用 node 跑（bun 不兼容其原生绑定），同 render-icon.mjs。
// 用法：node app/scripts/gen-nsis-images.mjs
import { chromium } from "playwright";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const dir = path.dirname(fileURLToPath(import.meta.url));
const iconsDir = path.join(dir, "../src-tauri/icons");
// logo 用 PNG 而非 SVG：SVG data URL 画进 canvas 会污染 canvas 致 getImageData 报安全错，
// 且 Chromium 对 SVG image 加载偶发失败；PNG(same-origin data URL)两个问题都没有。
const logoPng = readFileSync(path.join(iconsDir, "icon.png"));
const logoDataUrl = "data:image/png;base64," + logoPng.toString("base64");

// 在浏览器里把「背景 + logo + 文字」画进 canvas，返回 {rgba, png}。
async function compose(page, spec) {
  return page.evaluate(async ({ w, h, logoUrl, kind }) => {
    const cv = document.createElement("canvas");
    cv.width = w;
    cv.height = h;
    const ctx = cv.getContext("2d");
    // 竖向柔和深色渐变背景（比 logo 面板略暖略浅，衬出黑色圆角面板边界）。
    const g = ctx.createLinearGradient(0, 0, 0, h);
    g.addColorStop(0, "#2b2925");
    g.addColorStop(1, "#201f1c");
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, w, h);

    const logo = new Image();
    await new Promise((res, rej) => {
      logo.onload = res;
      logo.onerror = rej;
      logo.src = logoUrl;
    });

    if (kind === "header") {
      // 顶部横幅：左侧应用名，右侧 logo。
      const s = 40;
      ctx.drawImage(logo, w - s - 12, (h - s) / 2, s, s);
      ctx.fillStyle = "#f0efec";
      ctx.font = "600 15px 'Segoe UI', 'Microsoft YaHei', sans-serif";
      ctx.textBaseline = "middle";
      ctx.fillText("cc-kanban", 16, h / 2 + 1);
    } else {
      // 侧图：logo 居中偏上 + 应用名 + 一行浅灰副标题。
      const s = 92;
      ctx.drawImage(logo, (w - s) / 2, 66, s, s);
      ctx.textAlign = "center";
      ctx.fillStyle = "#f4f3f0";
      ctx.font = "600 19px 'Segoe UI', 'Microsoft YaHei', sans-serif";
      ctx.fillText("cc-kanban", w / 2, 192);
      ctx.fillStyle = "#9a958c";
      ctx.font = "12px 'Segoe UI', 'Microsoft YaHei', sans-serif";
      ctx.fillText("Claude Code 会话看板", w / 2, 216);
    }

    const rgba = Array.from(ctx.getImageData(0, 0, w, h).data);
    const png = cv.toDataURL("image/png").split(",")[1];
    return { rgba, png };
  }, spec);
}

// RGBA(自顶向下) → 24-bit BMP(BGR, 行 4 字节对齐, 自底向上)。背景不透明，alpha 忽略。
function encodeBmp(width, height, rgba) {
  const rowSize = Math.floor((24 * width + 31) / 32) * 4;
  const pixels = rowSize * height;
  const buf = Buffer.alloc(54 + pixels);
  buf.write("BM", 0);
  buf.writeUInt32LE(54 + pixels, 2);
  buf.writeUInt32LE(54, 10);
  buf.writeUInt32LE(40, 14);
  buf.writeInt32LE(width, 18);
  buf.writeInt32LE(height, 22); // 正数 = 自底向上
  buf.writeUInt16LE(1, 26);
  buf.writeUInt16LE(24, 28);
  buf.writeUInt32LE(pixels, 34);
  buf.writeInt32LE(2835, 38);
  buf.writeInt32LE(2835, 42);
  for (let y = 0; y < height; y++) {
    const srcY = height - 1 - y; // 翻转行序
    let off = 54 + y * rowSize;
    for (let x = 0; x < width; x++) {
      const si = (srcY * width + x) * 4;
      buf[off++] = rgba[si + 2]; // B
      buf[off++] = rgba[si + 1]; // G
      buf[off++] = rgba[si];     // R
    }
  }
  return buf;
}

const browser = await chromium.launch();
const page = await browser.newPage({ deviceScaleFactor: 1 });
await page.setContent("<!doctype html><html><body></body></html>");

for (const { name, w, h, kind } of [
  { name: "nsis-header", w: 150, h: 57, kind: "header" },
  { name: "nsis-sidebar", w: 164, h: 314, kind: "sidebar" },
]) {
  const { rgba, png } = await compose(page, { w, h, logoUrl: logoDataUrl, kind });
  writeFileSync(path.join(iconsDir, `${name}.bmp`), encodeBmp(w, h, rgba));
  writeFileSync(path.join(iconsDir, `${name}.preview.png`), Buffer.from(png, "base64"));
  console.log("wrote", `${name}.bmp`, `${w}x${h}`);
}

await browser.close();
