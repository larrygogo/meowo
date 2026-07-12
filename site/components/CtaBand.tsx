import { REPO, RELEASE_LATEST } from "@/lib/site";
import { DownloadIcon } from "./icons";

type Props = {
  title?: string;
  subtitle?: string;
};

export default function CtaBand({
  title = "下载 Meowo",
  subtitle = "免费开源，Windows 和 macOS 都能用。",
}: Props) {
  return (
    <section className="section-sm">
      <div className="container">
        <div className="cta-band">
          <h2 className="h1">{title}</h2>
          <p>{subtitle}</p>
          <div className="hero-cta" style={{ marginTop: 0 }}>
            <a
              className="btn btn-primary btn-lg"
              href={RELEASE_LATEST}
              target="_blank"
              rel="noopener noreferrer"
            >
              <DownloadIcon />
              下载最新版
            </a>
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
