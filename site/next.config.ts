import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  reactStrictMode: true,
  // 纯静态导出（GitHub Pages 无 Node 服务器）：产出 out/ 目录。
  output: "export",
  // Pages 以目录形式提供路由，导出为 /features/index.html 更稳妥。
  trailingSlash: true,
  // 导出模式下 next/image 默认优化不可用；本站用普通 <img>，置为不优化以防未来引入。
  images: { unoptimized: true },
};

export default nextConfig;
