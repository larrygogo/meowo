// 生成 README 用的演示图（中英各一套，写到 ../docs/images/）：
//   island[.en].webp   灵动岛动图：收起 → 弹审批提醒 → 点开面板 → 收起（app/src/demo/IslandStage.tsx）
//   panel[.en].png     对话窗 + 右侧改动面板（panelEmbed.tsx）
//   mobile[.en].png    手机远程页（mobileEmbed.tsx）
// 素材就是官网 iframe 里的那几段演示：先 vite build -c vite.demo.config.ts 产出 site/public/demo，
// 再起一个只读静态服务去截。不录 vite dev——dev 下界面会挂「DEV」角标。
//
// 灵动岛剧本靠 setTimeout / performance.now 实时播放，没有逐帧 seek 接口。这里用 Playwright 的
// page.clock 接管页面时钟，每帧手动拨 1000/FPS 毫秒再截图：录出来逐帧确定、不掉帧，和机器快慢无关。
// （CSS 动画不归 page.clock 管，眨眼之类按真实时间走，不影响剧本。）
//
// 用法：cd app && bun run demo:webp   （实际由 node 执行：Playwright 在 Windows 下需要 node 的多 fd
// 子进程管道，bun 暂不支持）
// 可调环境变量：
//   CC_WEBP_QUALITY=90   有损质量(1–100)。动图大面积是静止桌面，90 已看不出帧间残影。
//   CC_WEBP_EFFORT=5     压缩耗时/体积权衡(0–6)，越高越小越慢。
//   CC_SKIP_BUILD=1      跳过演示构建，直接用现有的 site/public/demo（只改了录制参数时省时间）。
import { spawnSync } from "node:child_process";
import { createReadStream, existsSync, mkdirSync, rmSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, extname, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import sharp from "sharp";

const appDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const sitepublic = resolve(appDir, "../site/public");
const outDir = resolve(appDir, "../docs/images");
const framesDir = resolve(appDir, "../target/demo-frames"); // 帧落盘，避免全量帧驻留内存
const QUALITY = Number(process.env.CC_WEBP_QUALITY || 90);
const EFFORT = Number(process.env.CC_WEBP_EFFORT || 5);
const FPS = 15;
const ISLAND_LOOP_MS = 15_200; // IslandStage 剧本一轮的总时长（各段 sleep / move 相加）
const LANGS = [{ lang: "zh", suffix: "" }, { lang: "en", suffix: ".en" }];

if (process.env.CC_SKIP_BUILD !== "1") {
  console.log("构建演示（vite.demo.config.ts → site/public/demo）…");
  const r = spawnSync("bun", ["x", "vite", "build", "-c", "vite.demo.config.ts"], { cwd: appDir, stdio: "inherit", shell: true });
  if (r.status !== 0) process.exit(r.status ?? 1);
}

const MIME = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".png": "image/png", ".svg": "image/svg+xml", ".woff2": "font/woff2" };
const server = createServer((req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, "http://x").pathname)).replace(/^([/\\])+/, "");
  const file = join(sitepublic, path);
  if (!file.startsWith(sitepublic) || !existsSync(file) || statSync(file).isDirectory()) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { "content-type": MIME[extname(file)] ?? "application/octet-stream" });
  createReadStream(file).pipe(res);
});
await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
const base = `http://127.0.0.1:${server.address().port}/demo`;

const browser = await chromium.launch();
try {
  mkdirSync(outDir, { recursive: true });
  for (const { lang, suffix } of LANGS) {
    await recordIsland(`${base}/embed-island.html?lang=${lang}`, resolve(outDir, `island${suffix}.webp`));
    await still(`${base}/embed-panel.html?lang=${lang}`, 1180, 760, 2, resolve(outDir, `panel${suffix}.png`));
    await still(`${base}/embed-mobile.html?lang=${lang}`, 390, 780, 2, resolve(outDir, `mobile${suffix}.png`));
  }
} finally {
  await browser.close();
  server.close();
}

async function newPage(w, h, scale) {
  const page = await browser.newPage({ viewport: { width: w, height: h }, deviceScaleFactor: scale });
  page.on("pageerror", (e) => console.warn("页面报错:", String(e)));
  return page;
}

async function still(url, w, h, scale, out) {
  const page = await newPage(w, h, scale);
  await page.goto(url, { waitUntil: "networkidle" });
  await page.waitForTimeout(2500); // 等面板自动点开文件、字体落地
  await page.screenshot({ path: out });
  await page.close();
  console.log(`完成：${out}`);
}

async function recordIsland(url, out) {
  const W = 880, H = 600, SCALE = 2; // 与官网 DemoFrame 的 island 设计尺寸一致
  const page = await newPage(W, H, SCALE);
  await page.clock.install();
  await page.goto(url, { waitUntil: "networkidle" });
  await page.clock.runFor(1000); // 让首帧渲染、剧本开跑
  const step = Math.round(1000 / FPS);
  const frames = Math.round(ISLAND_LOOP_MS / step);
  rmSync(framesDir, { recursive: true, force: true });
  mkdirSync(framesDir, { recursive: true });
  const paths = [];
  for (let f = 0; f < frames; f++) {
    const p = resolve(framesDir, `f${String(f).padStart(4, "0")}.png`);
    await page.screenshot({ path: p });
    paths.push(p);
    await page.clock.runFor(step);
  }
  await page.close();
  console.log(`编码 ${frames} 帧 → ${out}（质量 ${QUALITY}）…`);
  // delay 必须逐帧给数组：sharp 传单个数字只作用于首帧、其余回退 100ms。
  await sharp(paths, { join: { animated: true } })
    .webp({ loop: 0, delay: paths.map(() => step), quality: QUALITY, effort: EFFORT, smartSubsample: true })
    .toFile(out);
  rmSync(framesDir, { recursive: true, force: true });
  console.log(`完成：${out}（${(statSync(out).size / 1024 / 1024).toFixed(2)} MB）`);
}
