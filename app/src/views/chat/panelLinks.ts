/**
 * 对话里的链接要不要在右侧面板的浏览器里打开。
 *
 * 只接本机地址（localhost / 127.x / 0.0.0.0 / [::1]）：agent 起的 dev server 就在旁边看，
 * 不必切到系统浏览器。别的网址照旧交给系统浏览器——登录态、密码管理器、扩展都在那边。
 * 处理函数由对话窗注册；没注册（手机端、测试、贴纸窗）时一律走系统浏览器。
 */
let handler: ((url: string) => void) | null = null;

export function setPanelLinkHandler(next: ((url: string) => void) | null): void {
  handler = next;
}

export function isLocalDevUrl(href: string): boolean {
  try {
    const url = new URL(href);
    if (url.protocol !== "http:" && url.protocol !== "https:") return false;
    const host = url.hostname;
    return host === "localhost" || host === "0.0.0.0" || host === "[::1]" || /^127\.\d+\.\d+\.\d+$/.test(host);
  } catch {
    return false;
  }
}

/** 能在面板里打开就打开并返回 true；否则返回 false，调用方自己走系统浏览器。 */
export function openInPanel(href: string): boolean {
  if (!handler || !isLocalDevUrl(href)) return false;
  handler(href);
  return true;
}
