import { RELEASE_LATEST, RELEASES } from "@/lib/site";
import { getLatestRelease, formatSize, type Asset } from "@/lib/release";
import { AppleIcon, WindowsIcon, DownloadIcon } from "@/components/icons";
import Reveal from "@/components/Reveal";
import { type Lang } from "@/lib/i18n";

type Content = {
  eyebrow: string;
  title: string;
  leadVersion: (tag: string) => string;
  leadRest: string;
  win: { meta: string; btn: string; fallback: string };
  mac: {
    meta: string;
    btnArm: string;
    btnIntel: string;
    fallbackArm: string;
    fallbackIntel: string;
    hint: string;
  };
  allVersions: string;
  stepsEyebrow: string;
  stepsTitle: string;
  steps: { title: string; body: string }[];
  reqsEyebrow: string;
  reqsTitle: string;
  reqs: { k: string; v: string }[];
  macNote: string;
  winNote: string;
};

const CONTENT: Record<Lang, Content> = {
  zh: {
    eyebrow: "下载",
    title: "下载 Meowo",
    leadVersion: (tag) => `最新版本 ${tag}。`,
    leadRest: "开源，MIT 许可。无需提前配置 AI CLI：安装 Meowo 后，可直接在应用内一键安装、登录并自动接入。",
    win: { meta: "x64 安装包 · Windows 10 / 11", btn: "下载 .exe", fallback: "在 GitHub Releases 里选 x64-installer.exe（老版本为 x64-setup.exe）" },
    mac: {
      meta: "Apple 芯片与 Intel 各一个 DMG · ≥ Sonoma",
      btnArm: "下载 .dmg（Apple 芯片）",
      btnIntel: "下载 .dmg（Intel）",
      fallbackArm: "在 GitHub Releases 里选 aarch64.dmg",
      fallbackIntel: "在 GitHub Releases 里选 x64.dmg",
      hint: "不确定芯片：苹果菜单 → 关于本机，看「芯片」一行",
    },
    allVersions: "查看全部历史版本",
    stepsEyebrow: "安装",
    stepsTitle: "安装步骤",
    steps: [
      { title: "下载安装包", body: "在 GitHub Releases 里选择对应平台的安装包。" },
      { title: "安装并打开", body: "Windows 双击安装包一键安装（无 WebView2 的环境会自动回退经典安装界面）。macOS 把应用拖进「应用程序」，双击打开。" },
      { title: "一键安装并登录 AI CLI", body: "没有 Claude Code、Codex、Kimi、Gemini CLI 或 OpenCode 也没关系：在设置里选择工具，一键安装并发起账号登录。已有工具会自动检测。" },
      { title: "自动接入并开始使用", body: "Meowo 自动接入所需 hooks。在对话窗里选好 Agent 和项目目录，写下第一句话就开始。会话状态会出现在屏幕顶部的灵动岛上；没有刘海的 Mac 上，点菜单栏图标查看。想在手机上接着用，到设置里开启「手机远程」，扫码即可。" },
    ],
    reqsEyebrow: "环境",
    reqsTitle: "环境要求",
    reqs: [
      { k: "操作系统", v: "Windows 10 / 11，或 macOS 14 Sonoma 及以上" },
      { k: "AI 编程 CLI", v: "无需预装；可在 Meowo 内一键安装并登录 Claude Code / Codex / Kimi / Gemini CLI / OpenCode 其一，用于产生会话事件" },
      { k: "Windows 依赖", v: "WebView2 Runtime（Windows 11 自带），安装包会按需安装" },
      { k: "磁盘占用", v: "几十 MB。数据存在 ~/.meowo，本地 SQLite。" },
    ],
    macNote: "macOS 上第一次用外部终端（Terminal / iTerm2）打开会话时，系统会请求「自动化」授权，允许即可。",
    winNote: "Windows 首次运行安装包会提示「Windows 已保护你的电脑」：Windows 安装包暂时没有代码签名，点「更多信息」→「仍要运行」即可。安装包由 GitHub Actions 从公开源码构建，应用内的自动更新另有 minisign 签名校验。",
  },
  en: {
    eyebrow: "Download",
    title: "Download Meowo",
    leadVersion: (tag) => `Latest version ${tag}. `,
    leadRest: "Open source, MIT. No need to set up an AI CLI in advance: after installing Meowo, install, sign in, and connect right in the app.",
    win: { meta: "x64 installer · Windows 10 / 11", btn: "Download .exe", fallback: "Pick x64-installer.exe in GitHub Releases (older releases: x64-setup.exe)" },
    mac: {
      meta: "Separate DMG for Apple silicon and Intel · ≥ Sonoma",
      btnArm: "Download .dmg (Apple silicon)",
      btnIntel: "Download .dmg (Intel)",
      fallbackArm: "Pick aarch64.dmg in GitHub Releases",
      fallbackIntel: "Pick x64.dmg in GitHub Releases",
      hint: "Not sure which chip? Apple menu → About This Mac, see the Chip line",
    },
    allVersions: "See all past releases",
    stepsEyebrow: "Install",
    stepsTitle: "Install steps",
    steps: [
      { title: "Download the installer", body: "Pick the installer for your platform in GitHub Releases." },
      { title: "Install and open", body: "On Windows, double-click the installer for a one-click install (it falls back to the classic installer UI when WebView2 is unavailable). On macOS, drag the app to Applications and double-click." },
      { title: "One-click install & sign in to an AI CLI", body: "No Claude Code, Codex, Kimi, Gemini CLI, or OpenCode yet? In Settings, pick a tool and install & sign in with one click. Existing tools are detected automatically." },
      { title: "Auto-connect and start", body: "Meowo wires up the required hooks. In the chat window, pick an agent and a project folder and type your first message. Session status shows up on the island at the top of your screen; on a Mac without a notch, click the menu-bar icon. To keep going from your phone, turn on Phone access in Settings and scan the QR code." },
    ],
    reqsEyebrow: "Requirements",
    reqsTitle: "System requirements",
    reqs: [
      { k: "Operating system", v: "Windows 10 / 11, or macOS 14 Sonoma and later" },
      { k: "AI coding CLI", v: "None required upfront; install & sign in to one of Claude Code / Codex / Kimi / Gemini CLI / OpenCode inside Meowo to produce session events" },
      { k: "Windows dependency", v: "WebView2 Runtime (bundled on Windows 11); the installer adds it as needed" },
      { k: "Disk", v: "Tens of MB. Data lives in ~/.meowo, a local SQLite database." },
    ],
    macNote: "On macOS, the first time a session opens in an external terminal (Terminal / iTerm2), the system asks for Automation permission. Just allow it.",
    winNote: "On Windows the installer shows “Windows protected your PC”: the Windows installer isn't code-signed yet. Choose “More info” → “Run anyway” to continue. Installers are built by GitHub Actions from the public source, and in-app updates are verified with a minisign signature.",
  },
};

function assetSub(asset: Asset | null, fallback: string) {
  return asset ? `${asset.name} · ${formatSize(asset.size)}` : fallback;
}

export default async function DownloadContent({ lang }: { lang: Lang }) {
  const release = await getLatestRelease();
  const c = CONTENT[lang];

  return (
    <main>
      <section className="pagehead">
        <div className="container">
          <span className="eyebrow">{c.eyebrow}</span>
          <h1 className="h1">{c.title}</h1>
          <p className="lead">
            {release ? c.leadVersion(release.tag) : ""}
            {c.leadRest}
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
                <p className="meta">{c.win.meta}</p>
                <a className="btn btn-primary btn-lg" href={release?.windows?.url ?? RELEASE_LATEST}>
                  <DownloadIcon />
                  {c.win.btn}
                </a>
                <p className="sub">{assetSub(release?.windows ?? null, c.win.fallback)}</p>
              </div>
            </Reveal>
            <Reveal>
              <div className="platform">
                <div className="platform-top">
                  <AppleIcon />
                  <h3>macOS</h3>
                </div>
                <p className="meta">{c.mac.meta}</p>
                <a className="btn btn-primary btn-lg" href={release?.macosArm?.url ?? RELEASE_LATEST}>
                  <DownloadIcon />
                  {c.mac.btnArm}
                </a>
                <p className="sub">{assetSub(release?.macosArm ?? null, c.mac.fallbackArm)}</p>
                <a className="btn btn-ghost btn-lg" href={release?.macosIntel?.url ?? RELEASE_LATEST}>
                  <DownloadIcon />
                  {c.mac.btnIntel}
                </a>
                <p className="sub">{assetSub(release?.macosIntel ?? null, c.mac.fallbackIntel)}</p>
                <p className="sub sub-hint">{c.mac.hint}</p>
              </div>
            </Reveal>
          </div>
          <p className="more-row">
            <a className="btn btn-ghost" href={RELEASES} target="_blank" rel="noopener noreferrer">
              {c.allVersions}
            </a>
          </p>
        </div>
      </section>

      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.stepsEyebrow}</span>
            <h2 className="h1">{c.stepsTitle}</h2>
          </div>
          <div className="steps">
            {c.steps.map((s, i) => (
              <Reveal key={s.title}>
                <div className="step">
                  <span className="sn">{i + 1}</span>
                  <h3>{s.title}</h3>
                  <p>{s.body}</p>
                </div>
              </Reveal>
            ))}
          </div>
        </div>
      </section>

      <section className="section section-sunken">
        <div className="container container-narrow">
          <div className="section-head">
            <span className="eyebrow">{c.reqsEyebrow}</span>
            <h2 className="h1">{c.reqsTitle}</h2>
          </div>
          <div className="rows">
            {c.reqs.map((r) => (
              <div className="row" key={r.k}>
                <div className="k">{r.k}</div>
                <div className="v">{r.v}</div>
              </div>
            ))}
          </div>
          <p className="faint note-center">{c.winNote}</p>
          <p className="faint note-center">{c.macNote}</p>
        </div>
      </section>
    </main>
  );
}
