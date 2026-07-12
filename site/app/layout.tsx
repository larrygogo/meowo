import type { Metadata, Viewport } from "next";
import "./globals.css";
import Nav from "@/components/Nav";
import Footer from "@/components/Footer";

const title = "Meowo / 喵呜 — 桌面上的 AI 会话贴纸";
const description =
  "Meowo（喵呜）是个常驻桌面角落的小窗口，把 Claude Code、Codex、Kimi 的会话摆在一起：谁在跑、谁卡住了、谁在等你确认。点一下就能回到它所在的终端。免费开源，Windows 和 macOS 都能用。";

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
  themeColor: "#17181a",
  width: "device-width",
  initialScale: 1,
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="zh-CN">
      <body>
        <Nav />
        {children}
        <Footer />
      </body>
    </html>
  );
}
