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
 *  test/fixtures/local-paths.json 用例表）。做空第三轮查出 `\??\UNC\host\…` 这类写法绕过了
 *  「枚举坏写法」的旧规则，改成只认好写法：
 *  - Windows：`盘符:\` / `盘符:/` 开头，或 `\\?\盘符:\` 开头；
 *  - Unix：单个 `/` 开头（`//`、`/\` 不算）；
 *  - 不知道宿主平台（手机页、平台还没探测到）：Windows 规则，或 Unix 规则且路径里没有反斜杠、
 *    不以 `/?`、`/.` 开头——这样在 Windows 宿主上它只可能落到本机盘根，碰不到 NT 前缀。 */
export function isLocalAbsolute(path: string, os: PathOs = currentOs()): boolean {
  const s = path.trim();
  const windows = /^[A-Za-z]:[\\/]/.test(s) || /^\\\\\?\\[A-Za-z]:\\/.test(s);
  const unix = /^\/(?![/\\])/.test(s);
  if (os === "windows") return windows;
  if (os === "unix") return unix;
  return windows || (unix && !s.includes("\\") && !/^\/[?.]/.test(s));
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

/** 不是本地绝对路径返回 null（调用方显示成读不到），其余交给 convertFileSrc。 */
export function safeFileSrc(path: string): string | null {
  return isRemotePath(path) ? null : convertFileSrc(path);
}
