import { convertFileSrc } from "@tauri-apps/api/core";
import { isMac, isWindows } from "./platform";

/**
 * 本地文件 → 页面能加载的地址（桌面 asset 协议，手机 /file）。生产代码一律走这里，不直接调
 * convertFileSrc（assetSrc.test.ts 有扫描守卫）。
 *
 * 为什么要守卫（做空第二、三轮）：Tauri 2.12.5 的 asset 协议在判白名单之前就对路径做
 * is_symlink / exists（scope/fs.rs），Windows 上对网络共享路径做这两步就会发起 SMB 连接、可能
 * 带出 NTLM 凭据。回执、Markdown 里的路径谁都能写，所以不是本地绝对路径的在变成 URL 之前拒掉。
 */

export type PathOs = "windows" | "unix" | "unknown";

/** 本地绝对路径白名单（与后端 `meowo_agent::images::is_local_absolute` 同一规则，两边共用
 *  test/fixtures/local-paths.json 用例表）：
 *  - Windows：`盘符:\` / `盘符:/` 开头，或 `\\?\盘符:\` 开头；
 *  - Unix：单个 `/` 开头（`//`、`/\` 不算），且不在 `/net`、`/Network` 下（macOS 的 autofs 访问
 *    它们就会去挂 NFS/SMB，不分大小写）；
 *  - 不知道宿主平台（手机页、平台还没探测到）：Windows 规则，或 Unix 规则且路径里没有反斜杠、
 *    不以 `/?`、`/.` 开头——这样在 Windows 宿主上它只可能落到本机盘根，碰不到 NT 前缀。
 *  不 trim：判断用的字符串就是之后访问的字符串，前导空白一律不算本地。 */
export function isLocalAbsolute(path: string, os: PathOs = currentOs()): boolean {
  const windows = /^[A-Za-z]:[\\/]/.test(path) || /^\\\\\?\\[A-Za-z]:\\/.test(path);
  const unix = /^\/(?![/\\])/.test(path) && !/^\/(?:net|network)(?:\/|$)/i.test(path);
  if (os === "windows") return windows;
  if (os === "unix") return unix;
  return windows || (unix && !path.includes("\\") && !/^\/[?.]/.test(path));
}

function currentOs(): PathOs {
  if (isWindows()) return "windows";
  if (isMac()) return "unix";
  return "unknown";
}

/** 不是本地绝对路径就当远程（含相对路径：交给 asset 协议的只能是绝对路径）。 */
export function isRemotePath(path: string): boolean {
  return !isLocalAbsolute(path);
}

/** 页面显示用的地址。不是本地绝对路径返回 null（调用方显示成读不到）。
 *  SVG 一律换成后端栅格化的 `<原件>.png`（做空第四轮：浏览器渲染 SVG 会加载 CSS 里的
 *  `url(data:…)` 等内嵌位图，拦不完；后端 images::render_svg 渲染时什么位图都不加载）。 */
export function safeFileSrc(path: string): string | null {
  if (isRemotePath(path)) return null;
  return convertFileSrc(/\.svg$/i.test(path) ? `${path}.png` : path);
}

/** 下载用的地址（手机端「保存」）：原件，SVG 也给原件——下载不渲染。 */
export function downloadFileSrc(path: string): string | null {
  return isRemotePath(path) ? null : convertFileSrc(path);
}
