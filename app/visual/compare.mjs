#!/usr/bin/env node
// 视觉回归对比：逐像素比较两个目录里的同名 PNG，输出差异像素数，并给有差异的图生成差异图。
// 解码/编码用已有的 devDependency fast-png，不另装 pngjs/pixelmatch。
// 用法：node visual/compare.mjs <基线目录> <新目录> [--out <差异图目录>] [--tolerance 0] [--subset]
//   --tolerance：单个通道允许的最大差值（0~255），默认 0 = 严格逐像素一致。
//   --subset：新目录只拍了一部分（shoot 带了 --filter），基线里多出来的图不算缺图。
// 退出码：0 = 全部一致；1 = 有差异或有缺图；2 = 参数错误。
import { decode, encode } from "fast-png";
import { readdirSync, readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";

function parseArgs(argv) {
  const o = { a: null, b: null, out: null, tolerance: 0, subset: false };
  const rest = [];
  for (let i = 0; i < argv.length; i++) {
    const x = argv[i];
    if (x === "--out" || x === "-o") o.out = argv[++i];
    else if (x === "--tolerance" || x === "-t") o.tolerance = Math.max(0, Math.min(255, +argv[++i] || 0));
    else if (x === "--subset") o.subset = true;
    else if (x === "--help" || x === "-h") o.help = true;
    else rest.push(x);
  }
  [o.a, o.b] = rest;
  return o;
}

/// 统一成 8 位 RGBA，Playwright 截图本来就是，这里只是防御灰度/RGB/16 位图。
function toRgba(img) {
  const { width, height, channels, depth } = img;
  const src = img.data;
  const n = width * height;
  const out = new Uint8Array(n * 4);
  const scale = depth === 16 ? 1 / 257 : 1;
  for (let i = 0; i < n; i++) {
    const s = i * channels;
    const g = (k) => Math.round(src[s + k] * scale);
    if (channels === 4) { out[i * 4] = g(0); out[i * 4 + 1] = g(1); out[i * 4 + 2] = g(2); out[i * 4 + 3] = g(3); }
    else if (channels === 3) { out[i * 4] = g(0); out[i * 4 + 1] = g(1); out[i * 4 + 2] = g(2); out[i * 4 + 3] = 255; }
    else if (channels === 2) { out[i * 4] = out[i * 4 + 1] = out[i * 4 + 2] = g(0); out[i * 4 + 3] = g(1); }
    else { out[i * 4] = out[i * 4 + 1] = out[i * 4 + 2] = g(0); out[i * 4 + 3] = 255; }
  }
  return { width, height, data: out };
}

function load(file) {
  return toRgba(decode(readFileSync(file)));
}

/// 差异图：底图是 A 的淡灰版本，不同的像素涂成品红，一眼能看出改动落在哪。
function compareImages(a, b, tolerance) {
  const { width, height } = a;
  const diff = new Uint8Array(width * height * 4);
  let count = 0;
  let minX = width, minY = height, maxX = -1, maxY = -1;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 4;
      const differs =
        Math.abs(a.data[i] - b.data[i]) > tolerance ||
        Math.abs(a.data[i + 1] - b.data[i + 1]) > tolerance ||
        Math.abs(a.data[i + 2] - b.data[i + 2]) > tolerance ||
        Math.abs(a.data[i + 3] - b.data[i + 3]) > tolerance;
      if (differs) {
        count++;
        if (x < minX) minX = x; if (x > maxX) maxX = x;
        if (y < minY) minY = y; if (y > maxY) maxY = y;
        diff[i] = 255; diff[i + 1] = 0; diff[i + 2] = 200; diff[i + 3] = 255;
      } else {
        const l = Math.round(0.299 * a.data[i] + 0.587 * a.data[i + 1] + 0.114 * a.data[i + 2]);
        const v = 255 - Math.round((255 - l) * 0.25);
        diff[i] = diff[i + 1] = diff[i + 2] = v; diff[i + 3] = 255;
      }
    }
  }
  const box = count ? { x: minX, y: minY, w: maxX - minX + 1, h: maxY - minY + 1 } : null;
  return { count, diff, box };
}

function main() {
  const o = parseArgs(process.argv.slice(2));
  if (o.help || !o.a || !o.b) {
    console.log("用法：node visual/compare.mjs <基线目录> <新目录> [--out <差异图目录>] [--tolerance 0] [--subset]");
    process.exit(o.help ? 0 : 2);
  }
  const dirA = resolve(o.a);
  const dirB = resolve(o.b);
  for (const d of [dirA, dirB]) if (!existsSync(d)) { console.error("目录不存在：" + d); process.exit(2); }
  const outDir = resolve(o.out ?? join(dirB, "_diff"));
  const pngs = (d) => new Set(readdirSync(d).filter((f) => f.toLowerCase().endsWith(".png")));
  const a = pngs(dirA);
  const b = pngs(dirB);
  const names = [...new Set(o.subset ? [...b] : [...a, ...b])].sort();

  const rows = [];
  let changed = 0, missing = 0, same = 0;
  for (const name of names) {
    if (!a.has(name) || !b.has(name)) {
      missing++;
      rows.push({ name, status: a.has(name) ? "只在基线里" : "只在新目录里" });
      continue;
    }
    const ia = load(join(dirA, name));
    const ib = load(join(dirB, name));
    if (ia.width !== ib.width || ia.height !== ib.height) {
      changed++;
      rows.push({ name, status: "尺寸不同", sizeA: `${ia.width}x${ia.height}`, sizeB: `${ib.width}x${ib.height}` });
      continue;
    }
    const { count, diff, box } = compareImages(ia, ib, o.tolerance);
    const total = ia.width * ia.height;
    if (count === 0) { same++; rows.push({ name, status: "一致", diffPixels: 0, totalPixels: total }); continue; }
    changed++;
    mkdirSync(outDir, { recursive: true });
    const diffFile = join(outDir, name);
    writeFileSync(diffFile, encode({ width: ia.width, height: ia.height, data: diff, channels: 4, depth: 8 }));
    rows.push({ name, status: "有差异", diffPixels: count, totalPixels: total, ratio: +(count / total * 100).toFixed(4), box, diffFile });
  }

  for (const r of rows) {
    if (r.status === "一致") continue;
    if (r.status === "有差异") console.log(`✗ ${r.name}  差异像素 ${r.diffPixels}（${r.ratio}%），范围 x=${r.box.x} y=${r.box.y} ${r.box.w}x${r.box.h}`);
    else if (r.status === "尺寸不同") console.log(`✗ ${r.name}  尺寸不同 ${r.sizeA} → ${r.sizeB}`);
    else console.log(`? ${r.name}  ${r.status}`);
  }
  const totalDiff = rows.reduce((s, r) => s + (r.diffPixels ?? 0), 0);
  console.log(`共 ${names.length} 张：一致 ${same}，有差异 ${changed}，缺图 ${missing}；差异像素合计 ${totalDiff}`);
  if (changed) console.log(`差异图：${outDir}`);
  mkdirSync(outDir, { recursive: true });
  writeFileSync(join(outDir, "report.json"), JSON.stringify({ base: dirA, next: dirB, tolerance: o.tolerance, same, changed, missing, totalDiffPixels: totalDiff, rows }, null, 2));
  process.exit(changed || missing ? 1 : 0);
}

main();
