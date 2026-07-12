import { cache } from "react";
import { marked } from "marked";
import { REPO_SLUG } from "./site";

export type Asset = { name: string; url: string; size: number };
export type Release = {
  tag: string;
  version: string;
  windows: Asset | null;
  macos: Asset | null;
};
export type ReleaseNote = {
  tag: string;
  /** release 的标题；只是「Meowo v0.5.0」这种和 tag 重复的自动命名时为 null。 */
  title: string | null;
  date: string;
  /** release 正文渲染出的 HTML；作者没写说明时为 null。 */
  bodyHtml: string | null;
};

// 安装包文件名带版本号，且随 productName 变过（cc-kanban → Meowo），
// 所以按后缀从 release 资产里挑，不去拼名字。
const pick = (assets: Asset[], ext: string) =>
  assets.find((a) => a.name.toLowerCase().endsWith(ext)) ?? null;

// 站点是静态导出，下面这些请求都发生在构建时。发新版后要重新部署站点内容才会更新——
// deploy-pages.yml 挂了 release: published 触发，你在 GitHub 上点 Publish 时会自动跑。
async function gh<T>(path: string): Promise<T | null> {
  const headers: Record<string, string> = {
    Accept: "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
  };
  // CI 里带上 token，避开匿名请求的 60 次/小时限额。
  if (process.env.GITHUB_TOKEN) {
    headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  }
  try {
    const res = await fetch(`https://api.github.com/repos/${REPO_SLUG}${path}`, {
      headers,
      cache: "force-cache",
    });
    if (!res.ok) {
      console.warn(`[release] GitHub API ${path} → ${res.status}`);
      return null;
    }
    return (await res.json()) as T;
  } catch (err) {
    console.warn(`[release] 请求 GitHub API ${path} 失败`, err);
    return null;
  }
}

type ApiRelease = {
  tag_name: string;
  name: string | null;
  body: string | null;
  draft: boolean;
  prerelease: boolean;
  published_at: string | null;
  assets: { name: string; browser_download_url: string; size: number }[];
};

/**
 * 最新 release：把安装包的真实下载地址嵌进页面。
 * 拿不到就返回 null，调用方回退到 GitHub 的 releases/latest 页面。
 */
export const getLatestRelease = cache(async (): Promise<Release | null> => {
  const json = await gh<ApiRelease>("/releases/latest");
  if (!json) return null;
  const assets: Asset[] = json.assets.map((a) => ({
    name: a.name,
    url: a.browser_download_url,
    size: a.size,
  }));
  return {
    tag: json.tag_name,
    version: json.tag_name.replace(/^v/, ""),
    windows: pick(assets, ".exe"),
    macos: pick(assets, ".dmg"),
  };
});

// tauri-action 不带 releaseBody 时塞的默认正文——不是发布说明，别展示。
const PLACEHOLDER_BODY = /^see the assets to download/i;

/**
 * 全部 release，供更新日志页渲染。正文按 markdown 渲染成 HTML。
 * 草稿要过滤掉：构建时带了 token，API 会把未发布的草稿一起返回。
 */
export const getReleaseNotes = cache(async (): Promise<ReleaseNote[]> => {
  const list = await gh<ApiRelease[]>("/releases?per_page=50");
  if (!list) return [];
  return list
    .filter((r) => !r.draft)
    .map((r) => {
      const body = (r.body ?? "").trim();
      const hasNotes = body.length > 0 && !PLACEHOLDER_BODY.test(body);
      // tauri-action 把 release 命名成「Meowo v0.5.0」，和左边的 tag 重复；只有作者
      // 另起了标题（不含 tag）才值得显示。
      const name = r.name?.trim() ?? "";
      return {
        tag: r.tag_name,
        title: name && !name.includes(r.tag_name) ? name : null,
        date: (r.published_at ?? "").slice(0, 10),
        bodyHtml: hasNotes ? (marked.parse(body, { async: false }) as string) : null,
      };
    });
});

export const formatSize = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;
