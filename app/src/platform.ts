import { invoke } from "@tauri-apps/api/core";

let hostOs: "macos" | "windows" | "other" | null = null;

export async function detectHostOs(): Promise<void> {
  try {
    hostOs = (await invoke<string>("host_os")) as typeof hostOs;
  } catch {
    hostOs = "other";
  }
}

/** 远程（手机浏览器）入口专用：显式把宿主平台钉为 "other"。
 *  hostOs 语义是「宿主桌面 OS」。远程桥虽能经 /rpc/host_os 拿到真实值，
 *  但 isMac()/isWindows() 门控的都是桌面窗口形态（红绿灯顶栏、窗口控制组），
 *  远程端一律走非 Mac/非 Windows 路径——置真值反而会让手机端渲染桌面 Mac 布局。 */
export function setHostOsUnknown(): void {
  hostOs = "other";
}

export function isMac(): boolean {
  return hostOs === "macos";
}

/** Windows 专属能力（Snap Layouts 覆盖窗等）的门控。 */
export function isWindows(): boolean {
  return hostOs === "windows";
}

/** 贴纸（Sticker）在 macOS 上的面板形态。贴纸已不在桌面应用里出现（2026-11-05 两个平台都换成
 *  灵动岛），只剩官网演示与海报还渲染它，这个判定留给它用。 */
export function isMacPanel(): boolean {
  return hostOs === "macos";
}

/** 灵动岛落在哪（island.rs 的 island_host）。Windows 恒为 floating：屏幕顶边、可左右拖。
 *  macOS（2026-11-05 起也取消贴纸）：有刘海为 notch，钉在刘海上；没刘海为 menubar，屏幕上
 *  不画岛，菜单栏图标就是迷你岛，点它在图标下方展开面板。 */
export type IslandHost = { kind: "floating" | "notch" | "menubar"; notchWidth: number; notchHeight: number };
export const FLOATING_HOST: IslandHost = { kind: "floating", notchWidth: 0, notchHeight: 0 };
