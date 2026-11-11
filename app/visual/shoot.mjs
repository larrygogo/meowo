#!/usr/bin/env node
// 视觉回归截图：起 vite（5191）→ Playwright 按固定场景矩阵逐张截图 → 关 vite。
// 用法见 app/visual/README.md。所有不确定因素（时钟、随机数、动画、光标闪烁、字体加载）都在这里钉死，
// 目标是同一份代码连拍两次逐像素一致。
import { createServer } from "vite";
import { chromium, webkit } from "playwright";
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve, join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { VISUAL_PORT } from "./vite.visual.config.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const FIXED_NOW = 1_780_000_000_000; // 与 fixtures.ts / src/demo 的冻结时刻一致

// ── 参数 ──────────────────────────────────────────────
function parseArgs(argv) {
  const o = { out: null, filter: null, jobs: 4, engine: "chromium", list: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--out" || a === "-o") o.out = argv[++i];
    else if (a === "--filter" || a === "-f") o.filter = new RegExp(argv[++i]);
    else if (a === "--jobs" || a === "-j") o.jobs = Math.max(1, +argv[++i] || 1);
    else if (a === "--engine") o.engine = argv[++i];
    else if (a === "--list") o.list = true;
    else if (a === "--help" || a === "-h") o.help = true;
    else if (!o.out && !a.startsWith("-")) o.out = a;
    else throw new Error("未知参数：" + a);
  }
  return o;
}

// ── 场景矩阵 ──────────────────────────────────────────
const THEMES = ["dark", "light"];
const DPRS = [1, 2];
const DESKTOP = [{ w: 620, h: 760 }, { w: 1280, h: 800 }, { w: 2560, h: 1440 }];
const PHONE = [{ w: 375, h: 812 }, { w: 430, h: 932 }];
// sel：截图前必须出现的元素，缺了说明场景没挂起来，直接报错而不是拍一张空白当基线。
const DESKTOP_SCENES = [
  { scene: "chat", sel: ".chat-approval-actions" },
  { scene: "chat-long", sel: ".chat-scroll table" },
  { scene: "panel-changes", sel: ".scm-row" },
  { scene: "panel-diff", sel: ".chat-diff-code" },
  { scene: "settings-general", sel: ".settings" },
  { scene: "settings-appearance", sel: ".settings" },
  { scene: "settings-account", sel: ".settings" },
  { scene: "settings-remote", sel: ".settings" },
  { scene: "settings-sessions", sel: ".settings" },
  { scene: "settings-shortcuts", sel: ".settings" },
  { scene: "settings-network", sel: ".settings" },
  { scene: "settings-extensions", sel: ".settings" },
  { scene: "settings-about", sel: ".settings" },
  { scene: "settings-search", sel: ".main-body.is-searching" },
  { scene: "settings-noresults", sel: ".settings-noresults" },
  { scene: "newsession-page", sel: ".nsp-composer" },
  { scene: "newsession-page-wt", sel: ".wt-opt-body" },
  // 欢迎页插图已换成对话窗样张（UI 优化 #36），旧的 .obm-panel 不复存在。
  { scene: "onboarding", sel: ".obm-chat" },
];
// 重设计补的对话窗场景（desktop.tsx 的 flow-* / pop-*）：只拍中文、620 与 1280 两个宽度。
const FLOW_SIZES = [{ w: 620, h: 760 }, { w: 1280, h: 800 }];
const FLOW_SCENES = [
  { scene: "flow-states", sel: ".chat-scroll" },
  { scene: "flow-states-mid", sel: ".chat-scroll" },
  { scene: "flow-states-top", sel: ".chat-scroll" },
  { scene: "flow-question", sel: ".chat-scroll" },
  { scene: "flow-empty", sel: ".chat-window textarea" },
  { scene: "pop-palette", sel: ".chat-window" },
  { scene: "pop-shortcuts", sel: ".chat-window" },
  { scene: "pop-add", sel: ".chat-window" },
  { scene: "pop-progress", sel: ".chat-window" },
  { scene: "panel-files", sel: ".chat-diff-panel" },
];
const ISLAND_SEL = "[data-visual-stage] > div > *";
// 收起三种样式 ×（平时 / 没有会话 / 会话很多 / macOS 刘海两翼）、极简细条、面板（会话页 / 单页 / 空态 /
// 额度页 / macOS 菜单栏落点）、提醒（审批 / 出错长文本 / 跑完 / 菜单栏落点）。
// langs：岛默认只拍中文；标题行与提醒里文案最挤的几个另拍英文。
const ISLAND_SCENES = [
  { scene: "island-compact", w: 700, h: 80, sel: ISLAND_SEL },
  { scene: "island-compact-calm", w: 700, h: 80, sel: ISLAND_SEL },
  { scene: "island-compact-many", w: 700, h: 80, sel: ISLAND_SEL },
  { scene: "island-compact-idle", w: 700, h: 80, sel: ISLAND_SEL },
  { scene: "island-compact-notch", w: 700, h: 80, sel: ISLAND_SEL },
  { scene: "island-focus", w: 700, h: 80, sel: ISLAND_SEL, langs: ["zh", "en"] },
  { scene: "island-focus-idle", w: 700, h: 80, sel: ISLAND_SEL },
  { scene: "island-focus-notch", w: 700, h: 80, sel: ISLAND_SEL },
  { scene: "island-minimal", w: 700, h: 80, sel: ISLAND_SEL },
  { scene: "island-minimal-many", w: 700, h: 80, sel: ISLAND_SEL },
  { scene: "island-minimal-notch", w: 700, h: 80, sel: ISLAND_SEL },
  { scene: "island-strip", w: 1000, h: 40, sel: ".isl-strip" },
  { scene: "island-panel", w: 700, h: 300, sel: ".island-shell", langs: ["zh", "en"] },
  { scene: "island-panel-single", w: 700, h: 300, sel: ".island-shell" },
  { scene: "island-panel-empty", w: 700, h: 300, sel: ".island-shell" },
  { scene: "island-panel-menubar", w: 700, h: 300, sel: ".island-shell" },
  { scene: "island-usage", w: 700, h: 300, sel: ".island-shell", langs: ["zh", "en"] },
  { scene: "island-alert", w: 700, h: 110, sel: ISLAND_SEL, langs: ["zh", "en"] },
  { scene: "island-alert-error", w: 700, h: 110, sel: ISLAND_SEL },
  { scene: "island-alert-done", w: 700, h: 110, sel: ISLAND_SEL },
  { scene: "island-alert-menubar", w: 700, h: 110, sel: ISLAND_SEL },
];
// 定尺寸的独立窗口：按 window.rs 里的真实窗口尺寸拍（设置 620×460、引导 460×600、更新 400×252 起、
// 确认框 420×208 起再按内容收拢、新建会话 460×520）。上面桌面矩阵里的 settings-* / onboarding 是拉宽拉高的压力测试。
const SETTINGS_SECTIONS = ["general", "sessions", "appearance", "shortcuts", "remote", "network", "account", "extensions", "about", "search", "noresults"];
const PANE_SCENES = [
  ...SETTINGS_SECTIONS.map((sec) => ({ scene: "settingswin-" + sec, w: 620, h: 460, sel: sec === "noresults" ? ".settings-noresults" : ".settings" })),
  { scene: "onboarding-1", w: 460, h: 600, sel: ".obm-chat" },
  { scene: "onboarding-2", w: 460, h: 600, sel: ".ob-agent" },
  { scene: "onboarding-3", w: 460, h: 600, sel: ".ob-points" },
  { scene: "onboarding-4", w: 460, h: 600, sel: ".obm-island" },
  { scene: "updater-checking", w: 400, h: 252, sel: ".up-status" },
  { scene: "updater-latest", w: 400, h: 252, sel: ".up-body .sbtn" },
  { scene: "updater-error", w: 400, h: 252, sel: ".up-err" },
  { scene: "updater-available", w: 400, h: 500, sel: ".up-notes-body" },
  { scene: "updater-downloading", w: 400, h: 500, sel: ".up-prog" },
  { scene: "updater-installing", w: 400, h: 500, sel: ".up-actions .btn-primary:disabled" },
  { scene: "confirm", w: 420, h: 132, sel: ".app-confirm-actions" },
  { scene: "confirm-danger", w: 420, h: 132, sel: ".app-confirm-actions .is-danger" },
  { scene: "confirm-alert", w: 420, h: 132, sel: ".app-confirm-actions" },
  { scene: "confirm-long", w: 420, h: 400, sel: ".app-confirm-actions" },
  { scene: "newsession-window", w: 460, h: 520, sel: ".ns-agent" },
  { scene: "newsession-window-full", w: 460, h: 520, sel: ".wt-opt-body" },
  { scene: "crash", w: 620, h: 460, sel: ".app-crash" },
];
const MOBILE_SCENES = [
  { scene: "mobile-list", sel: ".remote-session-list" },
  { scene: "mobile-chat", sel: ".chat-approval-actions" },
  { scene: "mobile-changes", sel: ".scm-row" },
];
// 手机页的其余状态：只拍 dpr 2。tag 给同一场景的不同视口起名（键盘弹出、横屏、iPad）。
const PHONE_SM = [{ w: 375, h: 812 }];
const MOBILE_EXTRA = [
  { scene: "mobile-settings", sel: ".remote-settings-screen .pswitch", sizes: PHONE, langs: ["zh", "en"] },
  { scene: "mobile-new", sel: ".remote-new-session-overlay .ns-window", sizes: PHONE, langs: ["zh", "en"] },
  { scene: "mobile-gate", sel: ".remote-gate-card", sizes: PHONE_SM, langs: ["zh", "en"] },
  { scene: "mobile-gate-filled", sel: ".remote-gate-card", sizes: PHONE_SM },
  { scene: "mobile-gate-invalid", sel: ".remote-gate-err", sizes: PHONE_SM },
  { scene: "mobile-gate-unreachable", sel: ".remote-gate-err", sizes: PHONE_SM, langs: ["zh", "en"] },
  { scene: "mobile-gate-expired", sel: ".remote-gate-err", sizes: PHONE_SM },
  { scene: "mobile-boot", sel: ".remote-boot", sizes: PHONE_SM },
  { scene: "mobile-keys", sel: ".chat-remote-keyrow", sizes: PHONE_SM },
  { scene: "mobile-list-empty", sel: ".chat-sidebar-empty", sizes: PHONE_SM },
  { scene: "mobile-list-error", sel: ".chat-sidebar-empty.is-error", sizes: PHONE_SM },
  { scene: "mobile-list", tag: "en", sel: ".remote-tabbar", sizes: PHONE_SM, langs: ["en"] },
  // 键盘弹出：iPhone 竖屏键盘约占 336px，可视高度剩 476；SE 更矮（667-291=376）。
  { scene: "mobile-chat", tag: "kbd", sel: ".chat-approval-actions", sizes: [{ w: 375, h: 476 }, { w: 375, h: 376 }] },
  { scene: "mobile-list", tag: "land", sel: ".remote-tabbar", sizes: [{ w: 812, h: 375 }] },
  { scene: "mobile-chat", tag: "land", sel: ".chat-approval-actions", sizes: [{ w: 812, h: 375 }] },
  { scene: "mobile-list", tag: "ipad", sel: ".remote-tabbar", sizes: [{ w: 820, h: 1180 }] },
  { scene: "mobile-settings", tag: "ipad", sel: ".remote-settings-screen .pswitch", sizes: [{ w: 820, h: 1180 }] },
  { scene: "mobile-chat", tag: "ipad", sel: ".chat-approval-actions", sizes: [{ w: 820, h: 1180 }] },
  // iPad 横屏走宽屏双栏：设置入口是侧栏头的齿轮。
  { scene: "mobile-chat", tag: "ipadland", sel: ".chat-approval-actions", sizes: [{ w: 1180, h: 820 }] },
];
// 英文只挑两个文案最密的场景跑全矩阵，其余只拍中文。
const EN_SCENES = new Set(["chat", "mobile-chat"]);

function buildMatrix() {
  const shots = [];
  const push = (s) => shots.push({ ...s, name: `${s.scene}__${s.lang}__${s.theme}__w${s.w}__dpr${s.dpr}` });
  for (const sc of DESKTOP_SCENES) for (const lang of EN_SCENES.has(sc.scene) ? ["zh", "en"] : ["zh"])
    for (const theme of THEMES) for (const { w, h } of DESKTOP) for (const dpr of DPRS)
      push({ ...sc, page: "desktop", lang, theme, w, h, dpr, mobile: false });
  for (const sc of FLOW_SCENES) for (const theme of THEMES) for (const { w, h } of FLOW_SIZES) for (const dpr of DPRS)
    push({ ...sc, page: "desktop", lang: "zh", theme, w, h, dpr, mobile: false });
  for (const sc of ISLAND_SCENES) for (const lang of sc.langs ?? ["zh"]) for (const theme of THEMES) for (const dpr of DPRS)
    push({ ...sc, page: "desktop", lang, theme, dpr, mobile: false });
  for (const sc of PANE_SCENES) for (const theme of THEMES) for (const dpr of DPRS)
    push({ ...sc, page: "desktop", lang: "zh", theme, dpr, mobile: false });
  for (const sc of MOBILE_SCENES) for (const lang of EN_SCENES.has(sc.scene) ? ["zh", "en"] : ["zh"])
    for (const theme of THEMES) for (const { w, h } of PHONE) for (const dpr of DPRS)
      push({ ...sc, page: "mobile", lang, theme, w, h, dpr, mobile: true });
  for (const sc of MOBILE_EXTRA) for (const lang of sc.langs ?? ["zh"]) for (const theme of THEMES) for (const { w, h } of sc.sizes) {
    const shot = { ...sc, page: "mobile", lang, theme, w, h, dpr: 2, mobile: true };
    shots.push({ ...shot, name: `${sc.scene}${sc.tag ? "-" + sc.tag : ""}__${lang}__${theme}__w${w}${sc.tag ? "h" + h : ""}__dpr2` });
  }
  return shots;
}

// ── 冻结 ──────────────────────────────────────────────
// 文档创建前注入：Math.random 换成定种子 PRNG（灵动岛眨眼/走神的随机间隔），
// 并插一段关动画、关过渡、隐藏文本光标的样式。
const INIT_SCRIPT = `(() => {
  let s = 0x9e3779b9;
  Math.random = () => { s = (s + 0x6d2b79f5) | 0; let t = Math.imul(s ^ (s >>> 15), 1 | s); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
  const css = "*,*::before,*::after{animation-duration:0s!important;animation-delay:0s!important;animation-iteration-count:1!important;transition-duration:0s!important;transition-delay:0s!important;caret-color:transparent!important;scroll-behavior:auto!important}";
  const add = () => { const st = document.createElement("style"); st.setAttribute("data-visual-freeze", ""); st.textContent = css; (document.head || document.documentElement).appendChild(st); };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", add); else add();
})();`;

// 推进假时钟的总量：够轮询/防抖/首帧测量跑完，又小于灵动岛最短眨眼间隔（2.5s，islandParts.tsx useBlink），
// 不让「拍到眨眼」变成碰运气。
const ADVANCE_MS = 1800;
const STEP_MS = 100;

function trackNetwork(page) {
  let inflight = 0;
  let last = Date.now();
  const bump = (d) => { inflight += d; last = Date.now(); };
  page.on("request", () => bump(1));
  page.on("requestfinished", () => bump(-1));
  page.on("requestfailed", () => bump(-1));
  return async (quietMs = 120, timeoutMs = 20_000) => {
    const start = Date.now();
    for (;;) {
      if (inflight <= 0 && Date.now() - last >= quietMs) return;
      if (Date.now() - start > timeoutMs) throw new Error(`网络迟迟不空闲（还有 ${inflight} 个请求）`);
      await new Promise((r) => setTimeout(r, 30));
    }
  };
}

async function shootOne(browser, base, shot, outDir, dryRun = false) {
  const ctx = await browser.newContext({
    viewport: { width: shot.w, height: shot.h },
    deviceScaleFactor: shot.dpr,
    colorScheme: shot.theme,
    reducedMotion: "reduce",
    locale: shot.lang === "en" ? "en-US" : "zh-CN",
    timezoneId: "Asia/Shanghai",
    isMobile: shot.mobile && browser.browserType().name() === "chromium",
    hasTouch: shot.mobile,
  });
  const errors = [];
  try {
    await ctx.addInitScript(INIT_SCRIPT);
    // 假时钟：Date / setTimeout / setInterval / rAF / performance 全归它管，且暂停不走，
    // 只在下面的循环里按固定步长推进——轮询、防抖、动画帧的触发次数与顺序因此固定。
    await ctx.clock.install({ time: FIXED_NOW });
    await ctx.clock.pauseAt(FIXED_NOW + 1000);
    const page = await ctx.newPage();
    page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
    const warnings = [];
    page.on("console", (m) => { if (m.type() === "error") warnings.push("console: " + m.text()); });
    // 资源 4xx/5xx（字体被拒、模块 404）会让画面悄悄变样，按失败处理，别把它拍进基线。
    page.on("response", (r) => { if (r.status() >= 400) errors.push(`HTTP ${r.status()} ${r.url()}`); });
    const netIdle = trackNetwork(page);
    const url = `${base}/visual/${shot.page}.html?scene=${shot.scene}&lang=${shot.lang}&theme=${shot.theme}`;
    await page.goto(url, { waitUntil: "load" });
    await netIdle();
    for (let t = 0; t < ADVANCE_MS; t += STEP_MS) {
      await ctx.clock.runFor(STEP_MS);
      await netIdle(60); // 期间可能有懒加载 chunk（ChatMarkdown、i18n en 字典）
    }
    if (errors.length) throw new Error(errors.slice(0, 3).join(" | "));
    const ready = await page.evaluate(() => window.__visualReady === true);
    if (!ready) throw new Error("harness 没有发出就绪信号");
    if (!(await page.locator(shot.sel).first().count())) {
      // 留一张失败现场图，方便看场景到底挂成了什么样（不进输出目录，免得被当成基线）。
      const failDir = join(outDir, "_failed");
      mkdirSync(failDir, { recursive: true });
      await page.screenshot({ path: join(failDir, shot.name + ".png") }).catch(() => {});
      throw new Error(`缺少关键元素 ${shot.sel}（现场图：${join(failDir, shot.name + ".png")}）`);
    }
    // 字体按 unicode-range 分片懒加载（@fontsource 的 Noto Sans SC 有上百片）：排版后才发请求，
    // 机器忙时 fonts.ready 可能在下一片开始加载前就 resolve，截到「回退字体 → 正式字体」换字前的
    // 一帧（实测「当前进度 3/4」胶囊宽了 1px）。所以等到连续两轮都没有 loading 中的字体才算稳。
    for (let quiet = 0, i = 0; quiet < 2 && i < 50; i++) {
      const busy = await page.evaluate(async () => {
        await document.fonts.ready;
        await Promise.all([...document.images].map((img) => (img.complete ? null : new Promise((r) => { img.onload = img.onerror = r; }))));
        return [...document.fonts].some((f) => f.status === "loading");
      });
      await netIdle(80);
      quiet = busy ? 0 : quiet + 1;
    }
    // 最后再要求连续两张截图逐字节相同：仍有迟到的重排/换字就多推一步假时钟再拍，最多 6 次。
    let shotBuf = null;
    for (let i = 0; i < 6; i++) {
      await ctx.clock.runFor(STEP_MS);
      await netIdle(60);
      const buf = await page.screenshot({ animations: "disabled", caret: "hide", scale: "device" });
      if (shotBuf && buf.equals(shotBuf)) break;
      if (i === 5) throw new Error("画面一直在变，连续两张截图对不上");
      shotBuf = buf;
    }
    if (!dryRun) writeFileSync(join(outDir, shot.name + ".png"), shotBuf);
    if (errors.length) throw new Error(errors.slice(0, 3).join(" | "));
    return { name: shot.name, errors: warnings };
  } finally {
    await ctx.close();
  }
}

async function main() {
  const o = parseArgs(process.argv.slice(2));
  const all = buildMatrix();
  const shots = o.filter ? all.filter((s) => o.filter.test(s.name)) : all;
  if (o.help || (!o.out && !o.list)) {
    console.log("用法：node visual/shoot.mjs --out <目录> [--filter <正则>] [--jobs 4] [--engine chromium|webkit] [--list]");
    process.exit(o.help ? 0 : 1);
  }
  if (o.list) { for (const s of shots) console.log(s.name); console.log(`共 ${shots.length} 张`); return; }
  const outDir = resolve(o.out);
  mkdirSync(outDir, { recursive: true });

  const server = await createServer({ configFile: join(HERE, "vite.visual.config.mjs") });
  let browser = null;
  const cleanup = async () => {
    await browser?.close().catch(() => {});
    await server.close().catch(() => {});
  };
  const onSignal = () => { void cleanup().then(() => process.exit(130)); };
  process.once("SIGINT", onSignal);
  process.once("SIGTERM", onSignal);
  const failures = [];
  const t0 = Date.now();
  try {
    await server.listen();
    const base = `http://127.0.0.1:${VISUAL_PORT}`;
    // Chromium 的文字光栅化/图层合成在 GPU、多线程下会有亚像素级抖动（实测同一代码连拍，
    // 「当前进度」胶囊等处偶发几十到几百个像素不同），全部走软件、单线程、灰阶抗锯齿。
    const chromiumArgs = [
      "--disable-gpu", "--disable-lcd-text", "--force-color-profile=srgb", "--disable-partial-raster",
      "--disable-skia-runtime-opts", "--disable-threaded-animation", "--disable-threaded-scrolling",
      "--disable-checker-imaging", "--disable-font-subpixel-positioning",
    ];
    browser = await (o.engine === "webkit" ? webkit : chromium).launch(o.engine === "webkit" ? {} : { args: chromiumArgs });
    // 预热：每个场景先各开一次，让 vite 把依赖预构建和模块转译做完，正式截图不会撞上
    // 「发现新依赖 → 重新优化」造成的半截页面。
    const warm = [...new Map(shots.map((s) => [s.scene, { ...s, w: Math.min(s.w, 800), h: Math.min(s.h, 600), dpr: 1 }])).values()];
    for (const s of warm) await shootOne(browser, base, s, outDir, true).catch(() => {});

    let next = 0;
    let done = 0;
    const worker = async () => {
      while (next < shots.length) {
        const s = shots[next++];
        try {
          const r = await shootOne(browser, base, s, outDir);
          if (r.errors.length) console.warn(`  ! ${s.name} 页面报错：${r.errors.slice(0, 3).join(" | ").slice(0, 300)}`);
        } catch (e) {
          failures.push({ name: s.name, error: String(e?.message ?? e) });
          console.error(`  ✗ ${s.name}：${e?.message ?? e}`);
        }
        done++;
        if (done % 10 === 0 || done === shots.length) console.log(`  ${done}/${shots.length}`);
      }
    };
    await Promise.all(Array.from({ length: Math.min(o.jobs, shots.length) }, worker));
  } finally {
    await cleanup();
    process.off("SIGINT", onSignal);
    process.off("SIGTERM", onSignal);
  }
  writeFileSync(join(outDir, "manifest.json"), JSON.stringify({
    shots: shots.map((s) => s.name), failures, engine: o.engine, fixedNow: FIXED_NOW,
  }, null, 2));
  console.log(`完成：${shots.length - failures.length}/${shots.length} 张，用时 ${((Date.now() - t0) / 1000).toFixed(0)}s → ${outDir}`);
  if (failures.length) process.exit(1);
}

main().catch((e) => { console.error(e); process.exit(1); });
