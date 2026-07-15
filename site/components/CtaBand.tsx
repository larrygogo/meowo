import { REPO } from "@/lib/site";
import { getLatestRelease } from "@/lib/release";
import DownloadButton from "./DownloadButton";

type Props = {
  title?: string;
  subtitle?: string;
};

export default async function CtaBand({
  title = "把多开 AI 编程，收进桌面一角",
  subtitle = "少切终端，少输命令。每个会话的状态、配额与待办，一切尽在计划之中。",
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
