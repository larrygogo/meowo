import { REPO } from "@/lib/site";
import { getLatestRelease } from "@/lib/release";
import DownloadButton from "./DownloadButton";

type Props = {
  title?: string;
  subtitle?: string;
};

export default async function CtaBand({
  title = "下载 Meowo",
  subtitle = "开源，MIT 许可。Windows 与 macOS。",
}: Props) {
  const release = await getLatestRelease();

  return (
    <section className="section-sm">
      <div className="container">
        <div className="cta-band">
          <h2 className="h1">{title}</h2>
          <p>{subtitle}</p>
          <div className="hero-cta" style={{ marginTop: 0 }}>
            <DownloadButton
              windows={release?.windows ?? null}
              macos={release?.macos ?? null}
              fallbackHref="/download"
            />
            <a
              className="btn btn-secondary btn-lg"
              href={REPO}
              target="_blank"
              rel="noopener noreferrer"
            >
              在 GitHub 上 Star
            </a>
          </div>
        </div>
      </div>
    </section>
  );
}
