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
    leadRest: "开源，MIT 许可。AI 编程 CLI 不需要提前装，可以在装好 Meowo 之后，从设置里安装和登录。",
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
      { title: "安装并打开", body: "Windows 上双击安装包安装，没有 WebView2 的环境会换成经典安装界面。macOS 上把应用拖进「应用程序」，再双击打开。" },
      { title: "安装并登录 AI CLI", body: "在设置里选择要用的 CLI，安装后登录账号，Gemini CLI 需要填 API Key。已经装好的 CLI 会被自动识别。" },
      { title: "开始使用", body: "Meowo 会自动配置所需的 hooks。在对话窗里选 Agent 和项目目录，输入第一句话就开始了。会话状态显示在屏幕顶部的灵动岛上，没有刘海的 Mac 上点菜单栏图标查看。要在手机上使用，在设置里开启「手机远程」并扫码。" },
    ],
    reqsEyebrow: "环境",
    reqsTitle: "环境要求",
    reqs: [
      { k: "操作系统", v: "Windows 10 / 11，或 macOS 14 Sonoma 及以上" },
      { k: "AI 编程 CLI", v: "不需要提前安装。可以在 Meowo 里安装和登录 Claude Code、Codex、Kimi、Gemini CLI 或 OpenCode 中的任意一家" },
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
    leadRest: "Open source, MIT. You don't need an AI coding CLI beforehand; install and sign in to one from Meowo's Settings after installing Meowo.",
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
      { title: "Install and open", body: "On Windows, double-click the installer. Without WebView2, it switches to the classic installer UI. On macOS, drag the app to Applications and double-click it." },
      { title: "Install and sign in to an AI CLI", body: "In Settings, pick the CLI you want, install it and sign in. Gemini CLI needs an API key. CLIs that are already installed are detected automatically." },
      { title: "Start", body: "Meowo sets up the hooks it needs. In the chat window, pick an agent and a project folder and type the first message. Session status appears on the island at the top of the screen; on a Mac without a notch, click the menu-bar icon. To use Meowo from your phone, turn on Phone access in Settings and scan the QR code." },
    ],
    reqsEyebrow: "Requirements",
    reqsTitle: "System requirements",
    reqs: [
      { k: "Operating system", v: "Windows 10 / 11, or macOS 14 Sonoma and later" },
      { k: "AI coding CLI", v: "Not needed beforehand. You can install and sign in to Claude Code, Codex, Kimi, Gemini CLI or OpenCode from Meowo" },
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
