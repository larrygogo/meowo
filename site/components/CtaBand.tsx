import { REPO, RELEASE_LATEST } from "@/lib/site";
import { DownloadIcon } from "./icons";

type Props = {
  title?: string;
  subtitle?: string;
};

export default function CtaBand({
  title = "装一个试试",
  subtitle = "Windows 和 macOS 都有安装包。MIT 协议，源码在 GitHub 上。",
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
              去 GitHub 看看
            </a>
          </div>
        </div>
      </div>
    </section>
  );
}
