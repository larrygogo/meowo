import { convertFileSrc } from "@tauri-apps/api/core";

/**
 * 本地文件 → 页面能加载的地址（桌面 asset 协议，手机 /file）。生产代码一律走这里，不直接调
 * convertFileSrc（assetSrc.test.ts 有扫描守卫）。
 *
 * 为什么要守卫（做空第二轮）：Tauri 2.12.5 的 asset 协议在判白名单之前就对路径做
 * is_symlink / exists（scope/fs.rs），Windows 上对 `\\host\share\x.png` 做这两步就会发起 SMB
 * 连接、可能带出 NTLM 凭据。回执、Markdown 里的路径谁都能写，所以网络共享路径在变成 URL 之前拒掉。
 */

/** 与后端 `meowo_agent::images::is_remote_path` 同一套规则：两个分隔符开头（`\\`、`//`、混用；
 *  只放行 `\\?\C:\` 这种本机盘符长路径）、`file://<主机>/…`、`file:////…`。纯字符串判断。 */
export function isRemotePath(path: string): boolean {
  const s = path.trim();
  if (/^[\\/]{2}/.test(s)) return !/^[\\/]{2}\?[\\/][A-Za-z]:(?:[\\/]|$)/.test(s);
  const file = /^file:([\\/]*)(.*)$/i.exec(s);
  if (file) {
    const slashes = file[1].length;
    if ((slashes === 2 && file[2] !== "") || slashes >= 4) return true;
  }
  return false;
}

/** 网络共享路径返回 null（调用方显示成读不到），其余交给 convertFileSrc。 */
export function safeFileSrc(path: string): string | null {
  return isRemotePath(path) ? null : convertFileSrc(path);
}
