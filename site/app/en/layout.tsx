import type { Metadata, Viewport } from "next";
import "../globals.css";
import Nav from "@/components/Nav";
import Footer from "@/components/Footer";

// 英文站点的根 layout：与 (zh) 路由组平级，各自输出带正确 lang 的 <html>。
// 英文默认社交卡片：各 /en/* 页面只定义了自己的 title/description，未单独定义
// openGraph 时统一回落到这套英文文案；页面如需更贴切的 og 文案可在自己的 metadata 里覆盖。
const title = "Meowo — Desktop workbench for AI coding agents";
const description =
  "Meowo is a desktop app for using several AI coding CLIs at once: Claude Code, Codex, Kimi, Gemini CLI and OpenCode. Sessions run in its chat window, next to the changes, a terminal and a browser. The island at the top of the screen alerts you when a session is waiting, and you can also handle it from your phone. Open source, MIT, Windows and macOS.";

// 与 (zh)/layout.tsx 相同的站点域名逻辑：社交分享图需要绝对 URL。
const siteUrl = process.env.NEXT_PUBLIC_SITE_URL || "https://meowo.io";

export const metadata: Metadata = {
  metadataBase: new URL(siteUrl),
  title,
  description,
  icons: { icon: "/logo.png" },
  openGraph: {
    type: "website",
    locale: "en_US",
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

export default function EnRootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>
        {/* 禁用 JS 时 .reveal 的 opacity:0 不会补 .in，这里兜底让内容直接可见 */}
        <noscript
          dangerouslySetInnerHTML={{
            __html: "<style>.reveal{opacity:1 !important;transform:none !important}</style>",
          }}
        />
        <Nav />
        {children}
        <Footer />
        {/* 吉祥物眼睛（components/Mascot.tsx）：应用里真实的 Eyes 组件，异步加载不挡首屏 */}
        <script type="module" src="/mascot/meowo-eyes.js" async />
      </body>
    </html>
  );
}
