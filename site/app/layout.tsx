import type { Metadata, Viewport } from "next";
import "./globals.css";
import Nav from "@/components/Nav";
import Footer from "@/components/Footer";

const title = "Meowo / 喵呜 — AI 编程代理桌面工作台";
const description =
  "Meowo（喵呜）是一款本地优先的 AI 编程代理桌面工作台，统一管理多个 CLI 会话的状态、待办提醒、启动与续接。少切终端，少输命令。开源，MIT 许可，支持 Windows 与 macOS。";

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
