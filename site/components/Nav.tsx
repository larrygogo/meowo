"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import { REPO } from "@/lib/site";
import { getDict, langFromPath, switchLangPath, withLang } from "@/lib/i18n";
import { GitHubIcon, MenuIcon } from "./icons";

export default function Nav() {
  const pathname = usePathname();
  const [scrolled, setScrolled] = useState(false);
  const [open, setOpen] = useState(false);

  const lang = langFromPath(pathname);
  const d = getDict(lang);
  const base = lang === "en" ? pathname.replace(/^\/en/, "") || "/" : pathname;
  const otherLang = lang === "en" ? "zh" : "en";
  const switchHref = switchLangPath(pathname, otherLang);

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 6);
    window.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  // 路由变化时收起移动菜单
  useEffect(() => setOpen(false), [pathname]);

  // 让 <html lang> 跟随当前语言（静态导出下 html 标签固定，运行时同步以利可访问性/SEO）。
  useEffect(() => {
    document.documentElement.lang = d.htmlLang;
  }, [d.htmlLang]);

  const isActive = (path: string) =>
    path === "/" ? base === "/" : base === path || base.startsWith(path + "/");

  // 手动切换语言：记住选择，避免自动跳转再把用户拉回去。
  const rememberLang = () => {
    try {
      localStorage.setItem("meowo-lang", otherLang);
    } catch {
      /* ignore */
    }
  };

  return (
    <nav className={`nav${scrolled ? " scrolled" : ""}`}>
      <div className="container nav-inner">
        <Link href={withLang(lang, "/")} className="nav-brand">
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img src="/logo.png" alt="Meowo logo" width={26} height={26} />
          <span>
            Meowo <span className="cn">喵呜</span>
          </span>
        </Link>

        <div className="nav-menu">
          {d.nav.links.map((l) => (
            <Link
              key={l.path}
              href={withLang(lang, l.path)}
              className={`nav-link${isActive(l.path) ? " active" : ""}`}
            >
              {l.label}
            </Link>
          ))}
        </div>

        <div className="nav-right">
          <Link
            className="nav-lang"
            href={switchHref}
            onClick={rememberLang}
            aria-label={d.nav.switchTo}
            title={d.nav.switchTo}
          >
            {d.nav.switchTo}
          </Link>
          <a
            className="nav-gh"
            href={REPO}
            target="_blank"
            rel="noopener noreferrer"
            aria-label="GitHub"
          >
            <GitHubIcon />
          </a>
          <Link className="btn btn-primary nav-cta-desktop" href={withLang(lang, "/download")}>
            {d.nav.download}
          </Link>
          <button
            className="nav-burger"
            aria-label={d.nav.menu}
            aria-expanded={open}
            onClick={() => setOpen((v) => !v)}
          >
            <MenuIcon />
          </button>
        </div>
      </div>

      {open && (
        <div className="nav-mobile">
          {d.nav.links.map((l) => (
            <Link key={l.path} href={withLang(lang, l.path)}>
              {l.label}
            </Link>
          ))}
          <Link href={switchHref} onClick={rememberLang}>
            {d.nav.switchTo}
          </Link>
          <Link className="btn btn-primary" href={withLang(lang, "/download")}>
            {d.nav.download}
          </Link>
        </div>
      )}
    </nav>
  );
}
