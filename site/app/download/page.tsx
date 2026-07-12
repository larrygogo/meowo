import type { Metadata } from "next";
import { RELEASE_LATEST, RELEASES, DOCS_CLAUDE_CODE } from "@/lib/site";
import { AppleIcon, WindowsIcon, DownloadIcon } from "@/components/icons";
import Reveal from "@/components/Reveal";

export const metadata: Metadata = {
  title: "下载 · Meowo",
  description:
    "下载 Meowo：Windows x64 NSIS 安装包、macOS universal DMG（已签名公证）。含环境要求与安装说明，支持应用内自动更新。",
};

const REQS: { k: string; v: React.ReactNode }[] = [
  { k: "操作系统", v: "Windows 10 / 11，或 macOS 14 Sonoma 及以上" },
  {
    k: "AI 编程 CLI",
    v: (
      <>
        至少装了{" "}
        <a href={DOCS_CLAUDE_CODE} target="_blank" rel="noopener noreferrer">
          Claude Code
        </a>{" "}
        / Codex / Kimi 里的一个——不然没有会话可看
      </>
    ),
  },
  {
    k: "Windows 依赖",
    v: "WebView2 Runtime。Win11 自带，没有的话安装包会处理",
  },
  { k: "磁盘占用", v: "几十 MB。数据都在 ~/.meowo 里，一个本地 SQLite 文件" },
];

const STEPS = [
  { n: 1, title: "下载", body: "从 GitHub Releases 拿对应平台的安装包。" },
  { n: 2, title: "装上", body: "Windows 双击 setup.exe。macOS 拖进应用程序，再双击打开。" },
  {
    n: 3,
    title: "它自己接线",
    body: "第一次启动会把 reporter 挂到 Claude Code 的 hooks 上，一般不用你操心。",
  },
  { n: 4, title: "开个会话试试", body: "随便起一个 AI 会话，卡片马上就出现在贴纸里了。" },
];

export default function DownloadPage() {
  return (
    <main>
      <section className="pagehead">
        <div className="container">
          <span className="eyebrow">下载</span>
          <h1 className="h1">挑一个平台</h1>
          <p className="lead">免费，开源。装完打开就能用，之后的版本它会自己提醒你更新。</p>
        </div>
      </section>

      <section className="section-sm">
        <div className="container">
          <div className="platforms">
            <Reveal>
              <div className="platform">
                <div className="platform-top">
                  <WindowsIcon />
                  <h3>Windows</h3>
                </div>
                <p className="meta">x64 NSIS 安装包 · Windows 10 / 11</p>
                <a
                  className="btn btn-primary btn-lg"
                  href={RELEASE_LATEST}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  <DownloadIcon />
                  下载 .exe
                </a>
                <p className="sub">meowo_x.y.z_x64-setup.exe</p>
              </div>
            </Reveal>
            <Reveal>
              <div className="platform">
                <div className="platform-top">
                  <AppleIcon />
                  <h3>macOS</h3>
                </div>
                <p className="meta">universal DMG · Intel / Apple Silicon 通用 · ≥ Sonoma</p>
                <a
                  className="btn btn-primary btn-lg"
                  href={RELEASE_LATEST}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  <DownloadIcon />
                  下载 .dmg
                </a>
                <p className="sub">已签名公证，双击直接打开</p>
              </div>
            </Reveal>
          </div>
          <p style={{ textAlign: "center", marginTop: 24 }}>
            <a
              className="btn btn-ghost"
              href={RELEASES}
              target="_blank"
              rel="noopener noreferrer"
            >
              查看全部历史版本
            </a>
          </p>
        </div>
      </section>

      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">安装步骤</span>
            <h2 className="h1">装完之后</h2>
          </div>
          <div className="steps">
            {STEPS.map((s) => (
              <Reveal key={s.n}>
                <div className="step">
                  <span className="sn">{s.n}</span>
                  <h4>{s.title}</h4>
                  <p>{s.body}</p>
                </div>
              </Reveal>
            ))}
          </div>
        </div>
      </section>

      <section className="section section-sunken">
        <div className="container" style={{ maxWidth: 820 }}>
          <div className="section-head">
            <span className="eyebrow">环境要求</span>
            <h2 className="h1">需要什么</h2>
          </div>
          <div className="rows">
            {REQS.map((r) => (
              <div className="row" key={r.k}>
                <div className="k">{r.k}</div>
                <div className="v">{r.v}</div>
              </div>
            ))}
          </div>
          <p className="faint" style={{ textAlign: "center", marginTop: 22, fontSize: 13.5 }}>
            macOS 上第一次点「跳转 / 恢复终端」，系统会弹一个「自动化」授权框，允许 Meowo 控制
            Terminal / iTerm2 就行。
          </p>
        </div>
      </section>
    </main>
  );
}
