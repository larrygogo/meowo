import type { MetadataRoute } from "next";

// 与 app/layout 相同的站点域名逻辑：sitemap 需要绝对 URL。
const siteUrl = process.env.NEXT_PUBLIC_SITE_URL || "https://meowo.io";

// 静态导出：元数据路由默认动态，需显式声明。
export const dynamic = "force-static";

// 全部页面路由（不含语言前缀）；英文版统一加 /en 前缀。
const ROUTES = ["", "features", "download", "docs", "changelog", "faq"];

// 每条 URL 都带上中英互为备选的 languages，与页面 metadata 里的 hreflang 保持一致。
export default function sitemap(): MetadataRoute.Sitemap {
  return ROUTES.flatMap((route) => {
    const zhPath = route ? `/${route}/` : "/";
    const enPath = route ? `/en/${route}/` : "/en/";
    const languages = {
      "zh-CN": `${siteUrl}${zhPath}`,
      en: `${siteUrl}${enPath}`,
    };
    return [
      { url: `${siteUrl}${zhPath}`, alternates: { languages } },
      { url: `${siteUrl}${enPath}`, alternates: { languages } },
    ];
  });
}
