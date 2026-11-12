// 名单与旧侧栏的刷新开销对比（规格 .lead/20261111-roster 的性能预算：12 行同时刷新时不比侧栏慢 10% 以上）。
// 同一份 12 个在跑的会话、同一个节奏（每 450ms 全部换一句「此刻在做什么」并发 board-changed），
// 用 CDP 的 Performance.getMetrics 取这段时间主线程的任务总耗时，除以刷新次数。
// 用法：node visual/perf-roster.mjs [轮数=5]
import { createServer } from "vite";
import { chromium } from "playwright";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { VISUAL_PORT } from "./vite.visual.config.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROUNDS = Number(process.argv[2]) || 5;
const TICKS = 40;
const GAP_MS = 450; // 比 board-changed 的 400ms 节流略长：每次都真的刷新一遍

const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];

async function measure(browser, base, scene) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1, reducedMotion: "reduce" });
  const page = await ctx.newPage();
  await page.goto(`${base}/visual/desktop.html?scene=${scene}&lang=zh&theme=dark&perf=1`, { waitUntil: "load" });
  await page.waitForFunction(() => window.__visualReady === true);
  await page.waitForTimeout(1500);
  const cdp = await ctx.newCDPSession(page);
  await cdp.send("Performance.enable");
  const read = async () => Object.fromEntries((await cdp.send("Performance.getMetrics")).metrics.map((m) => [m.name, m.value]));
  // 先空转同样长的时间：扣掉页面自己的动画、轮询。
  const idle0 = await read();
  await page.waitForTimeout(TICKS * GAP_MS);
  const idle1 = await read();
  const busy0 = await read();
  let listeners = 0;
  for (let i = 1; i <= TICKS; i++) {
    listeners = await page.evaluate((tick) => window.__perfTick(tick), i);
    await page.waitForTimeout(GAP_MS);
  }
  const busy1 = await read();
  const shown = await page.evaluate(() => ({ roster: document.querySelectorAll(".roster-row").length, sidebar: document.querySelectorAll(".chat-sidebar-item").length, text: (document.querySelector(".roster-now") ?? document.querySelector(".chat-sidebar-item"))?.textContent?.slice(0, 40) }));
  await ctx.close();
  const per = (key) => ((busy1[key] - busy0[key]) - (idle1[key] - idle0[key])) * 1000 / TICKS;
  return { task: per("TaskDuration"), script: per("ScriptDuration"), layout: per("LayoutDuration") + per("RecalcStyleDuration"), listeners, shown };
}

const server = await createServer({ configFile: join(HERE, "vite.visual.config.mjs") });
await server.listen();
const browser = await chromium.launch();
try {
  const base = `http://127.0.0.1:${VISUAL_PORT}`;
  for (const scene of ["chat", "roster"]) await measure(browser, base, scene).catch(() => {}); // 预热 vite
  const out = { chat: [], roster: [] };
  for (let r = 0; r < ROUNDS; r++) for (const scene of ["chat", "roster"]) {
    const m = await measure(browser, base, scene);
    out[scene].push(m);
    console.log(scene, `每次刷新 任务 ${m.task.toFixed(2)}ms 脚本 ${m.script.toFixed(2)}ms 样式布局 ${m.layout.toFixed(2)}ms`, JSON.stringify(m.shown), `监听 ${m.listeners}`);
  }
  const mc = median(out.chat.map((m) => m.task)), mr = median(out.roster.map((m) => m.task));
  console.log(`中位数：侧栏 ${mc.toFixed(2)}ms，名单 ${mr.toFixed(2)}ms，名单 / 侧栏 = ${(mr / mc).toFixed(2)}`);
} finally {
  await browser.close();
  await server.close();
}
