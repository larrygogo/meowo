import type { Metadata } from "next";
import { REPO, RELEASES } from "@/lib/site";
import { getReleaseNotes } from "@/lib/release";
import Reveal from "@/components/Reveal";

export const metadata: Metadata = {
  title: "更新日志 · Meowo",
  description: "Meowo 各版本的发布说明，取自 GitHub Releases。",
};

export default async function ChangelogPage() {
  const releases = await getReleaseNotes();

  return (
    <main>
      <section className="pagehead">
        <div className="container">
          <span className="eyebrow">更新日志</span>
          <h1 className="h1">更新日志</h1>
          <p className="lead">
            内容取自{" "}
            <a
              href={RELEASES}
              target="_blank"
              rel="noopener noreferrer"
              style={{ color: "var(--accent-ink)", textDecoration: "underline" }}
            >
              GitHub Releases
            </a>
            ，每次发版后自动同步。
          </p>
        </div>
      </section>

      <section className="section-sm">
        <div className="container">
          {releases.length === 0 ? (
            <p className="lead" style={{ textAlign: "center" }}>
              暂时拉不到发布记录，请直接看{" "}
              <a href={RELEASES} target="_blank" rel="noopener noreferrer">
                GitHub Releases
              </a>
              。
            </p>
          ) : (
            <div className="timeline">
              {releases.map((r) => (
                <Reveal key={r.tag}>
                  <div className="rel">
                    <div className="rel-meta">
                      <span className="ver">{r.tag}</span>
                      <div className="date">{r.date}</div>
                    </div>
                    <div className="rel-body">
                      {r.title && <h3>{r.title}</h3>}
                      {r.bodyHtml ? (
                        <div
                          className="rel-md"
                          dangerouslySetInnerHTML={{ __html: r.bodyHtml }}
                        />
                      ) : (
                        <p className="rel-empty">
                          这个版本没写发布说明，详见{" "}
                          <a
                            href={`${REPO}/releases/tag/${r.tag}`}
                            target="_blank"
                            rel="noopener noreferrer"
                          >
                            GitHub
                          </a>
                          。
                        </p>
                      )}
                    </div>
                  </div>
                </Reveal>
              ))}
            </div>
          )}
        </div>
      </section>
    </main>
  );
}
