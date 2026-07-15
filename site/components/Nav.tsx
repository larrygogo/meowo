"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import { NAV_LINKS, REPO } from "@/lib/site";
import { GitHubIcon, MenuIcon } from "./icons";

export default function Nav() {
  const pathname = usePathname();
  const [scrolled, setScrolled] = useState(false);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 6);
    window.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  // 路由变化时收起移动菜单
  useEffect(() => setOpen(false), [pathname]);

  const isActive = (href: string) =>
    href === "/" ? pathname === "/" : pathname.startsWith(href);

  return (
    <nav className={`nav${scrolled ? " scrolled" : ""}`}>
      <div className="container nav-inner">
        <Link href="/" className="nav-brand">
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img src="/logo.png" alt="Meowo logo" width={26} height={26} />
          <span>
            Meowo <span className="cn">喵呜</span>
          </span>
        </Link>

        <div className="nav-menu">
          {NAV_LINKS.map((l) => (
            <Link
              key={l.href}
              href={l.href}
              className={`nav-link${isActive(l.href) ? " active" : ""}`}
            >
              {l.label}
            </Link>
          ))}
        </div>

        <div className="nav-right">
          <a
            className="nav-gh"
            href={REPO}
            target="_blank"
            rel="noopener noreferrer"
            aria-label="GitHub"
          >
            <GitHubIcon />
          </a>
          <Link className="btn btn-primary nav-cta-desktop" href="/download">
            下载
          </Link>
          <button
            className="nav-burger"
            aria-label="菜单"
            aria-expanded={open}
            onClick={() => setOpen((v) => !v)}
          >
            <MenuIcon />
          </button>
        </div>
      </div>

      {open && (
        <div className="nav-mobile">
          {NAV_LINKS.map((l) => (
            <Link key={l.href} href={l.href}>
              {l.label}
            </Link>
          ))}
          <Link className="btn btn-primary" href="/download">
            下载
          </Link>
        </div>
      )}
    </nav>
  );
}
