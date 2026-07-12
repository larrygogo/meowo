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
        已安装{" "}
        <a href={DOCS_CLAUDE_CODE} target="_blank" rel="noopener noreferrer">
          Claude Code
        </a>{" "}
        / Codex / Kimi 其一，用于产生会话事件
      </>
    ),
  },
  {
    k: "Windows 依赖",
    v: "WebView2 Runtime（Win11 自带）——安装包会按需处理",
  },
  { k: "磁盘占用", v: "约几十 MB，数据存于 ~/.meowo（纯本地 SQLite）" },
];

const STEPS = [
  { n: 1, title: "下载安装包", body: "选择你的平台，从 GitHub Releases 下载对应安装包。" },
  { n: 2, title: "安装并打开", body: "Windows 双击 setup.exe；macOS 拖入应用程序，双击打开。" },
  {
    n: 3,
    title: "自动接入",
    body: "首次启动会自动把 reporter 接到 Claude Code 设置，通常不用手动挂 hooks。",
  },
  { n: 4, title: "开始使用", body: "新开一个 AI 会话，卡片就会实时出现在贴纸里。" },
];

export default function DownloadPage() {
  return (
    <main>
      <section className="pagehead">
        <div className="container">
          <span className="eyebrow">下载</span>
          <h1 className="h1">选择你的平台</h1>
          <p className="lead">免费、开源。下载安装后打开即可使用，支持应用内自动检查更新。</p>
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
            <h2 className="h1">四步，开始用</h2>
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
            <h2 className="h1">跑起来需要什么</h2>
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
            macOS 首次点击「跳转 / 恢复终端」会触发系统「自动化」授权，允许 Meowo 控制
            Terminal / iTerm2 即可。
          </p>
        </div>
      </section>
    </main>
  );
}
