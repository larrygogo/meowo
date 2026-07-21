import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  reactStrictMode: true,
  // 纯静态导出（GitHub Pages 无 Node 服务器）：产出 out/ 目录。
  output: "export",
  // Pages 以目录形式提供路由，导出为 /features/index.html 更稳妥。
  trailingSlash: true,
  // 导出模式下 next/image 默认优化不可用；本站用普通 <img>，置为不优化以防未来引入。
  images: { unoptimized: true },
  // (zh)/en 两个路由组各自持有根 layout（为了输出正确的 <html lang>），没有公共根 layout；
  // 开启后未匹配 URL 由 app/global-not-found.tsx（整文档组件）渲染成 out/404.html。
  experimental: { globalNotFound: true },
};

export default nextConfig;
