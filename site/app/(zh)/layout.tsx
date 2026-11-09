import type { Metadata, Viewport } from "next";
import "../globals.css";
import Nav from "@/components/Nav";
import Footer from "@/components/Footer";
import LangHint from "@/components/LangHint";

const title = "Meowo / 喵呜 — AI 编程代理桌面工作台";
const description =
  "Meowo（喵呜）是本地优先的 AI 编程代理桌面工作台：Claude Code、Codex、Kimi、Gemini CLI、OpenCode 的会话在一个对话窗里跑，审批、改动、终端和浏览器都在手边，灵动岛常驻屏幕顶部，离开电脑用手机接着推进。开源，MIT 许可，支持 Windows 与 macOS。";

// 站点域名，用于生成社交分享图（Open Graph / Twitter）的绝对 URL。
// 默认自定义域名；可用环境变量 NEXT_PUBLIC_SITE_URL 覆盖（如预览环境）。
const siteUrl = process.env.NEXT_PUBLIC_SITE_URL || "https://meowo.io";

export const metadata: Metadata = {
  metadataBase: new URL(siteUrl),
  title,
  description,
  icons: { icon: "/logo.png" },
  openGraph: {
    type: "website",
    title,
    description,
    images: [{ url: "/logo.png" }],
  },
  twitter: {
    card: "summary",
    title,
    description,
    images: ["/logo.png"],
  },
};

export const viewport: Viewport = {
  themeColor: "#0e100f",
  width: "device-width",
  initialScale: 1,
};

export default function ZhRootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="zh-CN">
      <body>
        {/* 禁用 JS 时 .reveal 的 opacity:0 不会补 .in，这里兜底让内容直接可见 */}
        <noscript
          dangerouslySetInnerHTML={{
            __html: "<style>.reveal{opacity:1 !important;transform:none !important}</style>",
          }}
        />
        <Nav />
        <LangHint />
        {children}
        <Footer />
        {/* 吉祥物眼睛（components/Mascot.tsx）：应用里真实的 Eyes 组件，异步加载不挡首屏 */}
        <script type="module" src="/mascot/meowo-eyes.js" async />
      </body>
    </html>
  );
}
