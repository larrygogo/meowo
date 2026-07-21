import type { Metadata, Viewport } from "next";
import Link from "next/link";
import "./globals.css";
import Nav from "@/components/Nav";
import Footer from "@/components/Footer";

// 全局 404（experimental.globalNotFound）：路由组多根 layout 下没有公共根 layout，
// 未匹配 URL 由这个自带 <html>/<body> 的整文档组件渲染。内容沿用旧 not-found.tsx 的中英双语 404。
export const metadata: Metadata = {
  title: "页面未找到 / Page not found",
};

export const viewport: Viewport = {
  themeColor: "#0e100f",
  width: "device-width",
  initialScale: 1,
};

export default function GlobalNotFound() {
  return (
    <html lang="zh-CN">
      <body>
        <Nav />
        <main>
          <section className="pagehead">
            <div className="container" style={{ textAlign: "center" }}>
              <span className="eyebrow">404</span>
              <h1 className="h1">页面走丢了 / Page not found</h1>
              <p className="lead">
                你要找的页面不存在或已被移动。
                <br />
                The page you are looking for doesn&rsquo;t exist or has been moved.
              </p>
              <p style={{ marginTop: 28 }}>
                <Link className="btn btn-primary" href="/">
                  返回首页 / Back to home
                </Link>
              </p>
            </div>
          </section>
        </main>
        <Footer />
      </body>
    </html>
  );
}
