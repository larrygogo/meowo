"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { LICENSE, RELEASES, REPO } from "@/lib/site";
import { getDict, langFromPath, withLang } from "@/lib/i18n";

const EXTERNAL: Record<string, string> = { repo: REPO, releases: RELEASES, license: LICENSE };

export default function Footer() {
  const pathname = usePathname();
  const lang = langFromPath(pathname);
  const d = getDict(lang);

  return (
    <footer className="footer">
      <div className="container">
        <div className="footer-top">
          <div className="footer-brand">
            <Link href={withLang(lang, "/")} className="nav-brand">
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img src="/logo.png" alt="Meowo logo" width={26} height={26} />
              <span>
                Meowo <span className="cn">喵呜</span>
              </span>
            </Link>
            <p>{d.footer.tagline}</p>
          </div>

          <div className="footer-cols">
            {d.footer.cols.map((col) => (
              <div className="footer-col" key={col.title}>
                <h5>{col.title}</h5>
                {col.links.map((l) =>
                  l.href ? (
                    <a key={l.label} href={EXTERNAL[l.href]} target="_blank" rel="noopener noreferrer">
                      {l.label}
                    </a>
                  ) : (
                    <Link key={l.label} href={withLang(lang, l.path!)}>
                      {l.label}
                    </Link>
                  )
                )}
              </div>
            ))}
          </div>
        </div>

        <div className="footer-bottom">
          <span>{d.footer.license}</span>
          <span className="tip">{d.footer.tip}</span>
        </div>
      </div>
    </footer>
  );
}
