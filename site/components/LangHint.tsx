"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";

// 英文浏览器访问中文页时给一条可关闭的切换提示，替代过去的整页自动跳转
// （旧方案会先下载并闪现整页中文 HTML 再 location.replace，且与 hreflang「/ = zh-CN」的
// 声明互相干扰）。用户点切换或关闭都会记住选择（与 Nav 的语言开关共用 meowo-lang），
// 记住后任何页面都不再提示，也不再自动跳转。
export default function LangHint() {
  const pathname = usePathname();
  const [show, setShow] = useState(false);

  useEffect(() => {
    try {
      if (localStorage.getItem("meowo-lang")) return;
      const nav = (navigator.language || "").toLowerCase();
      if (nav.startsWith("zh")) return;
      setShow(true);
    } catch {
      /* localStorage 不可用时保持静默 */
    }
  }, []);

  if (!show) return null;
  const enHref = `/en${pathname === "/" ? "/" : pathname}`;
  const remember = (lang: string) => {
    try {
      localStorage.setItem("meowo-lang", lang);
    } catch {
      /* 同上 */
    }
  };

  return (
    <div className="lang-hint" role="status">
      <span>This page is also available in English.</span>
      <Link href={enHref} onClick={() => remember("en")}>
        Switch to English
      </Link>
      <button
        type="button"
        aria-label="Dismiss"
        onClick={() => {
          remember("zh");
          setShow(false);
        }}
      >
        ×
      </button>
    </div>
  );
}
