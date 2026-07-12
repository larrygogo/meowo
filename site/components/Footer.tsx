import Link from "next/link";
import { LICENSE, RELEASES, REPO } from "@/lib/site";

export default function Footer() {
  return (
    <footer className="footer">
      <div className="container">
        <div className="footer-top">
          <div className="footer-brand">
            <Link href="/" className="nav-brand">
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img src="/logo.png" alt="Meowo logo" width={26} height={26} />
              <span>
                Meowo <span className="cn">喵呜</span>
              </span>
            </Link>
            <p>
              桌面小窗，显示 Claude Code、Codex、Kimi 的会话状态。
            </p>
          </div>

          <div className="footer-cols">
            <div className="footer-col">
              <h5>产品</h5>
              <Link href="/features">功能</Link>
              <Link href="/download">下载</Link>
              <Link href="/changelog">更新日志</Link>
            </div>
            <div className="footer-col">
              <h5>资源</h5>
              <Link href="/docs">文档</Link>
              <Link href="/faq">FAQ</Link>
              <a href={RELEASES} target="_blank" rel="noopener noreferrer">
                Releases
              </a>
            </div>
            <div className="footer-col">
              <h5>项目</h5>
              <a href={REPO} target="_blank" rel="noopener noreferrer">
                GitHub
              </a>
              <a href={LICENSE} target="_blank" rel="noopener noreferrer">
                License
              </a>
            </div>
          </div>
        </div>

        <div className="footer-bottom">
          <span>MIT © larrygogo</span>
          <span className="tip">名字来自猫叫 meow，中文译作「喵呜」🐱</span>
        </div>
      </div>
    </footer>
  );
}
