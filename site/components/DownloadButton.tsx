"use client";

import { useEffect, useState } from "react";
import { DownloadIcon } from "./icons";
import type { Asset } from "@/lib/release";
import type { Lang } from "@/lib/i18n";

type Platform = "windows" | "macos" | null;

type Props = {
  windows: Asset | null;
  macos: Asset | null;
  /** 认不出平台、或该平台没有安装包时的去处（下载页 / GitHub releases）。 */
  fallbackHref: string;
  className?: string;
  lang?: Lang;
};

const LABELS = {
  zh: { macos: "下载 .dmg（macOS）", windows: "下载 .exe（Windows）", latest: "下载最新版" },
  en: { macos: "Download .dmg (macOS)", windows: "Download .exe (Windows)", latest: "Download latest" },
};

// 服务端渲染时还不知道访客的系统，先给一个中性按钮；hydrate 后换成对应平台的直链。
function detect(): Platform {
  if (typeof navigator === "undefined") return null;
  const ua = navigator.userAgent;
  if (/iPhone|iPad|iPod|Android/i.test(ua)) return null; // 移动端没得下
  if (/Mac/i.test(ua)) return "macos";
  if (/Win/i.test(ua)) return "windows";
  return null;
}

export default function DownloadButton({
  windows,
  macos,
  fallbackHref,
  className = "btn btn-primary btn-lg",
  lang = "zh",
}: Props) {
  const [platform, setPlatform] = useState<Platform>(null);
  useEffect(() => setPlatform(detect()), []);

  const t = LABELS[lang];
  const asset = platform === "macos" ? macos : platform === "windows" ? windows : null;
  const href = asset ? asset.url : fallbackHref;
  const label = asset ? (platform === "macos" ? t.macos : t.windows) : t.latest;
  const external = href.startsWith("http");

  return (
    <a
      className={className}
      href={href}
      {...(external ? { target: "_blank", rel: "noopener noreferrer" } : {})}
    >
      <DownloadIcon />
      {label}
    </a>
  );
}
