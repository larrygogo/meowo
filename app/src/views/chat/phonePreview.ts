/**
 * 手机上点对话里的本机链接（AI 起的 dev server）：让电脑上的 Meowo 为这个端口开一个预览代理，
 * 新开一页打开。后端见 src-tauri/src/remote_preview.rs，收口（只代理本会话起的端口、一次性码换
 * cookie）也在那边。
 *
 * 只在手机远程页生效；桌面端本机链接照旧在右侧面板的浏览器里开（panelLinks.ts）。
 */
import { invoke } from "@tauri-apps/api/core";
import { remoteUi } from "../../remoteMode";
import { isLocalDevUrl } from "./panelLinks";
import type { PreviewOpenArgs } from "../../generated/contracts/bridge/PreviewOpenArgs";

type PreviewTicket = { host: string; port: number; path: string };

// 当前打开的会话（ChatWindow 在手机端登记）：后端凭它认「这个端口是不是这个会话起的」。
let currentSession: number | null = null;

export function setPreviewSession(sessionId: number | null): void {
  currentSession = sessionId !== null && sessionId >= 0 ? sessionId : null;
}

/** 预览地址：主机用后端给的代理地址（就是手机连 Meowo 用的那个 IP）。不用地址栏里的主机名——
 *  经 MagicDNS、.local 名字连进来时，代理只认 IP（防 DNS rebinding）。 */
export function previewUrl(ticket: PreviewTicket): string {
  return `http://${ticket.host}:${ticket.port}${ticket.path}`;
}

/**
 * 能处理就返回一个 Promise（失败时 reject，由调用方就地显示原因）；不该由这里处理（不是手机页、
 * 不是本机地址、没有会话）返回 null，调用方照旧走自己的打开方式。
 *
 * 新页必须在点击的同一拍里开：iOS Safari 只认手势里同步调用的 window.open，等后端回话再开会被拦。
 * 所以先开一个空页写上「正在打开」，拿到地址再把它导过去；失败就关掉它。
 */
export function openPhonePreview(href: string, openingText: string): Promise<void> | null {
  if (!remoteUi() || currentSession === null || !isLocalDevUrl(href)) return null;
  const win = window.open("", "_blank");
  if (win) {
    try {
      // 预览页是 AI 写的代码：不给它经 window.opener 反过来改 Meowo 这一页的机会。
      win.opener = null;
      win.document.title = "Meowo";
      win.document.body.style.cssText = "font:16px/1.6 system-ui,-apple-system,sans-serif;padding:48px 24px;color:#888";
      win.document.body.textContent = openingText;
    } catch {
      /* 有的浏览器不让写空页：只是少了「正在打开」，不影响跳转 */
    }
  }
  const args = { sessionId: currentSession, url: href } satisfies PreviewOpenArgs;
  return invoke<PreviewTicket>("preview_open", args).then(
    (ticket) => {
      const url = previewUrl(ticket);
      if (win && !win.closed) win.location.replace(url);
      // 新页被浏览器拦了：就在这一页打开，返回键回到 Meowo。
      else window.location.assign(url);
    },
    (error: unknown) => {
      if (win && !win.closed) win.close();
      throw error;
    },
  );
}
