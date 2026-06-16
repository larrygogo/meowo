// 外观（明暗模式 / 不透明度 / 界面密度）的运行时套用。
// 两个窗口（贴纸 main、设置 about）都在 main.tsx 里 boot：
//   - 明暗模式 + 不透明度：两窗都套用（不透明度只影响走 --cc-bg 的贴纸/缩略条，设置窗口无副作用）。
//   - 界面密度（--cc-ui）：仅贴纸窗口套用（设置窗口固定尺寸，不缩放）。
// 数据源是 ~/.cc-kanban/settings.json，经 get_settings 读取；任一窗口改设置后后端广播
// settings-changed，这里实时重套用（顺带做了设置窗口里的明暗即时预览）。
import { listen } from "@tauri-apps/api/event";
import { getSettings, type Settings, type ThemeMode } from "./api";

type Appearance = { theme: ThemeMode; opacity: number; ui_scale: number };

const CACHE_KEY = "cc-kanban-appearance";
const DEFAULTS: Appearance = { theme: "dark", opacity: 94, ui_scale: 100 };

let current: Appearance = DEFAULTS;
let scaleEnabled = false;

function clampPct(n: number, lo: number, hi: number): number {
  if (!Number.isFinite(n)) return hi;
  return Math.max(lo, Math.min(hi, n));
}

/** 把外观模式解析为实际生效的深/浅；system 时读系统偏好。 */
function resolveTheme(t: ThemeMode): "dark" | "light" {
  if (t === "light") return "light";
  if (t === "system") {
    return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  }
  return "dark";
}

function apply(a: Appearance): void {
  current = a;
  const root = document.documentElement;
  root.setAttribute("data-theme", resolveTheme(a.theme));
  // 不透明度下限与 UI 一致（25–100）：放低下限以便配合系统 acrylic 透出更明显的模糊桌面；
  // 仍留 25% 底，避免手改 settings.json 为极小值时渲染出全透明的空底板。
  root.style.setProperty("--cc-opacity", String(clampPct(a.opacity, 25, 100) / 100));
  if (scaleEnabled) {
    root.style.setProperty("--cc-ui", String(clampPct(a.ui_scale, 50, 200) / 100));
  }
}

function pick(s: Partial<Settings> | null | undefined): Appearance {
  return {
    theme: s?.theme ?? DEFAULTS.theme,
    opacity: s?.opacity ?? DEFAULTS.opacity,
    ui_scale: s?.ui_scale ?? DEFAULTS.ui_scale,
  };
}

function readCache(): Appearance {
  try {
    const c = JSON.parse(localStorage.getItem(CACHE_KEY) || "");
    if (c && typeof c === "object") return pick(c as Partial<Settings>);
  } catch {
    /* ignore */
  }
  return DEFAULTS;
}

function writeCache(a: Appearance): void {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(a));
  } catch {
    /* ignore */
  }
}

/**
 * 启动时调用：先用缓存同步套用（避免浅色用户首屏闪深色），再异步拉真实设置校正，
 * 并订阅 settings-changed 实时套用、prefers-color-scheme 跟随系统。
 * @param opts.scale 是否套用界面密度（仅贴纸窗口传 true）。
 */
export function bootAppearance(opts: { scale: boolean }): void {
  scaleEnabled = opts.scale;
  apply(readCache());
  // settings-changed 是权威实时源：先注册监听，一旦收到过，就让稍后才 resolve 的初始 getSettings
  // 不再覆盖（消除 fetch-vs-subscribe 竞态——否则 in-flight 的旧读取结果可能压掉刚到的新值）。
  let eventApplied = false;
  listen<Settings>("settings-changed", (e) => {
    eventApplied = true;
    const a = pick(e.payload);
    apply(a);
    writeCache(a);
  }).catch(() => {});
  getSettings()
    .then((s) => {
      if (eventApplied) return;
      const a = pick(s);
      apply(a);
      writeCache(a);
    })
    .catch(() => {});
  window
    .matchMedia("(prefers-color-scheme: light)")
    .addEventListener("change", () => {
      if (current.theme === "system") apply(current);
    });
}
