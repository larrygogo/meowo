import type { Metadata } from "next";
import { RELEASE_LATEST, RELEASES, DOCS_CLAUDE_CODE } from "@/lib/site";
import { getLatestRelease, formatSize, type Asset } from "@/lib/release";
import { AppleIcon, WindowsIcon, DownloadIcon } from "@/components/icons";
import Reveal from "@/components/Reveal";

export const metadata: Metadata = {
  title: "下载 · Meowo",
  description:
    "下载 Meowo：Windows x64 NSIS 安装包，macOS universal DMG（已签名公证）。含环境要求与安装步骤，应用内可以检查更新。",
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
    v: "WebView2 Runtime（Windows 11 自带），安装包会按需安装",
  },
  { k: "磁盘占用", v: "几十 MB。数据存在 ~/.meowo，本地 SQLite。" },
];

const STEPS = [
  { n: 1, title: "下载安装包", body: "在 GitHub Releases 里选择对应平台的安装包。" },
  { n: 2, title: "安装并打开", body: "Windows 双击 setup.exe。macOS 把应用拖进「应用程序」，双击打开。" },
  {
    n: 3,
    title: "自动接入",
    body: "Meowo 会为检测到的 AI 编程 CLI 接入所需 hooks，一般不需要手动编辑配置。",
  },
  { n: 4, title: "开始使用", body: "选择项目目录和 AI 工具，点「启动」即可新建会话，无需先在终端切目录或输入命令。" },
];

// 拿到了具体安装包就直连它，并把真实文件名和体积标出来；拿不到就退回 releases 页面。
function assetSub(asset: Asset | null, fallback: string) {
  return asset ? `${asset.name} · ${formatSize(asset.size)}` : fallback;
}

export default async function DownloadPage() {
  const release = await getLatestRelease();
  const version = release ? `最新版本 ${release.tag}。` : "";

  return (
    <main>
      <section className="pagehead">
        <div className="container">
          <span className="eyebrow">下载</span>
          <h1 className="h1">下载 Meowo</h1>
          <p className="lead">
            {version}开源，MIT 许可。安装后会自动检测本机已有的 AI 编程 CLI，应用内可以检查更新。
          </p>
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
                  href={release?.windows?.url ?? RELEASE_LATEST}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  <DownloadIcon />
                  下载 .exe
                </a>
                <p className="sub">{assetSub(release?.windows ?? null, "在 GitHub Releases 里选 x64-setup.exe")}</p>
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
                  href={release?.macos?.url ?? RELEASE_LATEST}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  <DownloadIcon />
                  下载 .dmg
                </a>
                <p className="sub">{assetSub(release?.macos ?? null, "在 GitHub Releases 里选 universal.dmg")}</p>
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
            <span className="eyebrow">安装</span>
            <h2 className="h1">安装步骤</h2>
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
            <span className="eyebrow">环境</span>
            <h2 className="h1">环境要求</h2>
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
            macOS 上首次点「跳转 / 恢复终端」会触发系统的「自动化」授权，允许 Meowo 控制
            Terminal / iTerm2 即可。
          </p>
        </div>
      </section>
    </main>
  );
}
