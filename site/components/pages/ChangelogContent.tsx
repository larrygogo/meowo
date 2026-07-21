import { REPO, RELEASES } from "@/lib/site";
import { getReleaseNotes } from "@/lib/release";
import Reveal from "@/components/Reveal";
import { type Lang } from "@/lib/i18n";

const T = {
  zh: {
    eyebrow: "更新日志",
    title: "更新日志",
    leadPre: "内容取自 ",
    leadPost: "，每次发版后自动同步。",
    releases: "GitHub Releases",
    emptyPre: "暂时拉不到发布记录，请直接看 ",
    emptyPost: "。",
    noNotesPre: "这个版本没写发布说明，详见 ",
    noNotesPost: "。",
    gh: "GitHub",
  },
  en: {
    eyebrow: "Changelog",
    title: "Changelog",
    leadPre: "Pulled from ",
    leadPost: ", synced automatically after each release.",
    releases: "GitHub Releases",
    emptyPre: "Couldn't load release records — see ",
    emptyPost: ".",
    noNotesPre: "No release notes for this version — see ",
    noNotesPost: ".",
    gh: "GitHub",
  },
};

export default async function ChangelogContent({ lang }: { lang: Lang }) {
  const releases = await getReleaseNotes();
  const t = T[lang];

  return (
    <main>
      <section className="pagehead">
        <div className="container">
          <span className="eyebrow">{t.eyebrow}</span>
          <h1 className="h1">{t.title}</h1>
          <p className="lead">
            {t.leadPre}
            <a href={RELEASES} target="_blank" rel="noopener noreferrer" className="link-inline">
              {t.releases}
            </a>
            {t.leadPost}
          </p>
        </div>
      </section>

      <section className="section-sm">
        <div className="container">
          {releases.length === 0 ? (
            <p className="lead" style={{ textAlign: "center" }}>
              {t.emptyPre}
              <a href={RELEASES} target="_blank" rel="noopener noreferrer">{t.releases}</a>
              {t.emptyPost}
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
                        <div className="rel-md" dangerouslySetInnerHTML={{ __html: r.bodyHtml }} />
                      ) : (
                        <p className="rel-empty">
                          {t.noNotesPre}
                          <a href={`${REPO}/releases/tag/${r.tag}`} target="_blank" rel="noopener noreferrer">{t.gh}</a>
                          {t.noNotesPost}
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
