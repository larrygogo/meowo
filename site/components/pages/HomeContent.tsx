import Link from "next/link";
import { REPO } from "@/lib/site";
import { getLatestRelease, getReleaseStats } from "@/lib/release";
import { getDict, withLang, type Lang } from "@/lib/i18n";
import { ArrowRightIcon, GitHubIcon } from "@/components/icons";
import DownloadButton from "@/components/DownloadButton";
import Reveal from "@/components/Reveal";
import CtaBand from "@/components/CtaBand";
import DemoFrame from "@/components/DemoFrame";
import Mascot, { type EyesCue, type EyesTone } from "@/components/Mascot";
import { AgentLogo, type ProviderId } from "@/components/SupportedAgents";

// 首页（2026-11-09 用户定的第二版方向）：
// - 主张是「完整工作台」，灵动岛是招牌视觉；
// - 内容按「一天里多开 AI 编程」的时间线讲，每一格配应用里的真实界面；
// - 视觉保留深色，灵动岛那双眼睛做贯穿全站的吉祥物（Mascot = 真实 Eyes 组件），
//   每个时间点的表情跟着那一刻的状态走，状态色参与配色，不再用打勾清单。
// 只写已发版（≤0.8.1）的能力；各家 Agent 能力不一样的地方要写清楚是哪几家。

/** 时间线一格的状态色：运行中 / 等你处理 / 跑完 / 额度告急。 */
type NodeTone = "run" | "warn" | "done" | "err";
type Demo = "chat" | "island-alert" | "mobile" | "panel";
type Node = {
  time: string;
  tone: NodeTone;
  eyes: { tone: EyesTone; cue?: EyesCue; quota?: number; busy?: boolean };
  label: string;
  title: string;
  body: string;
  demo?: Demo;
};

type Content = {
  hero: { eyebrow: string; h1a: string; accent: string; lead: string; github: string; note: string; works: string };
  day: { eyebrow: string; title: string; lead: string };
  nodes: Node[];
  agents: { eyebrow: string; title: string; body: string; more: string };
  trust: { eyebrow: string; title: string; cells: { big: string; title: string; body: string }[]; releases: (count: number, since: string) => { big: string; title: string; body: string } };
  platHead: { eyebrow: string; title: string };
  plats: { title: string; body: string }[];
  more: string;
};

const AGENTS: { id: ProviderId; name: string }[] = [
  { id: "claude", name: "Claude Code" },
  { id: "codex", name: "Codex" },
  { id: "kimi", name: "Kimi Code" },
  { id: "gemini", name: "Gemini CLI" },
  { id: "opencode", name: "OpenCode" },
];

const CONTENT: Record<Lang, Content> = {
  zh: {
    hero: {
      eyebrow: "// AI 编程代理工作台",
      h1a: "多个 AI 编程会话",
      accent: "在一个窗口里跑",
      lead: "一个桌面应用。Claude Code、Codex、Kimi、Gemini CLI 和 OpenCode 的会话都在它的对话窗里运行。有会话等你批准时，屏幕顶部的灵动岛会提醒；不在电脑前，可以用手机处理。",
      github: "GitHub",
      note: "开源 · MIT · Windows 与 macOS · 无需预装 AI CLI",
      works: "支持",
    },
    day: {
      eyebrow: "一天里的 Meowo",
      title: "一个常见的工作日",
      lead: "图里是应用的真实界面，数据是演示用的。",
    },
    nodes: [
      {
        time: "09:00", tone: "run", eyes: { tone: "running", cue: "surprised" }, label: "开工",
        title: "开三个会话",
        body: "在对话窗里选 Agent 和目录，输入第一句话，会话就开始了。这天的三个任务分给三家：Claude Code 重构登录页，Kimi 修支付回调，Codex 补单测。工具调用在对话里折叠成一行，终端弹出的菜单显示成按钮。",
        demo: "chat",
      },
      {
        time: "10:30", tone: "warn", eyes: { tone: "pending" }, label: "待批准",
        title: "Kimi 要跑一条需要批准的命令",
        body: "灵动岛在屏幕顶部弹出提醒，点「打开」进入那个会话。审批卡上写着这条命令会动到什么，比如工作区外的路径、删除文件或联网。",
        demo: "island-alert",
      },
      {
        time: "12:30", tone: "run", eyes: { tone: "running", busy: true }, label: "离开工位",
        title: "在手机上处理审批",
        body: "手机远程要先在设置里开启，手机扫码配对一次。之后会话需要你时，Bark（iPhone App）会推送通知，点开就是手机浏览器里的同一个对话，可以批准命令、回答提问或者发消息。",
        demo: "mobile",
      },
      {
        time: "14:00", tone: "done", eyes: { tone: "waiting" }, label: "回到电脑",
        title: "检查改动",
        body: "右侧面板的「更改」页签列出这次改过的文件，点开是 diff。对某一行有意见，可以在那一行写评论，再把评论放进输入框发给 Agent。同一排页签里还有文件、终端和浏览器。Claude Code、Codex 和 Kimi 的会话可以自己操作这个浏览器。",
        demo: "panel",
      },
      {
        time: "17:30", tone: "err", eyes: { tone: "running", quota: 92 }, label: "额度见底",
        title: "额度快用完了",
        body: "灵动岛的额度环显示 Claude Code、Codex 和 Kimi 的 5 小时和 7 天额度，Opus 这类模型单独计算的周额度也在里面。用到 85% 以上，环会变红。除了 Gemini CLI，每家都可以保存多个账号来回切换，也可以改用 API 中转。",
      },
      {
        time: "19:00", tone: "done", eyes: { tone: "running", cue: "star" }, label: "收工",
        title: "收尾",
        body: "跑完的会话，头像在灵动岛上会亮一个点。看过的会话可以标记完成，它会移到「已完成」里，以后还能找回来。如果还有会话没跑完，打开「运行时防止休眠」，电脑就不会进入睡眠，屏幕照常熄灭。",
      },
    ],
    agents: {
      eyebrow: "五家 Agent",
      title: "支持五家 AI 编程 CLI",
      body: "还没装的，可以在设置里安装和登录，Gemini CLI 需要填 API Key。所需的 hooks 由 Meowo 自动配置。各家能做的事不完全一样，比如额度读数、多账号、让 Agent 操作浏览器，功能页有逐项对照。",
      more: "查看能力对照",
    },
    trust: {
      eyebrow: "本地优先",
      title: "数据存在你自己的电脑上",
      cells: [
        { big: "0", title: "台自有服务器", body: "会话和设置只存在本机的 ~/.meowo/ 目录。唯一的例外是 Bark 推送：开启后，推送内容（会话标题、Agent 的最后一句话、打开会话的链接）会经过 Bark 的服务器。" },
        { big: "MIT", title: "开源", body: "源码和发版流程都在 GitHub 上。安装包由 GitHub Actions 用公开源码构建，应用内更新会校验 minisign 签名。" },
      ],
      releases: (count, since) => ({ big: String(count), title: "个版本", body: `第一个版本发布于 ${since.slice(0, 4)} 年 ${Number(since.slice(5, 7))} 月，每个版本的改动都写在更新日志里。` }),
    },
    platHead: { eyebrow: "平台", title: "Windows 与 macOS" },
    plats: [
      { title: "Windows", body: "灵动岛是屏幕顶部的一条胶囊，点开可以看会话和额度。位置可以左右拖动，也可以缩成一条细线。有全屏程序在前台时，它会自动隐藏。" },
      { title: "macOS", body: "有刘海的 Mac 上，灵动岛围在刘海周围。没有刘海时，它在菜单栏里，点图标弹出面板。菜单栏图标也是这双眼睛。" },
    ],
    more: "查看全部功能",
  },
  en: {
    hero: {
      eyebrow: "// AI coding agent workbench",
      h1a: "Run several AI coding sessions",
      accent: "in one window",
      lead: "A desktop app. Sessions from Claude Code, Codex, Kimi, Gemini CLI and OpenCode all run in its chat window. When one is waiting for your approval, the island at the top of the screen alerts you, and away from your computer you can handle it on your phone.",
      github: "GitHub",
      note: "Open source · MIT · Windows & macOS · no AI CLI needed upfront",
      works: "Works with",
    },
    day: {
      eyebrow: "A day with Meowo",
      title: "A typical workday",
      lead: "The screenshots are the real app with demo data.",
    },
    nodes: [
      {
        time: "09:00", tone: "run", eyes: { tone: "running", cue: "surprised" }, label: "Start",
        title: "Start three sessions",
        body: "In the chat window, pick an agent and a folder and type the first message. Today's three tasks go to three agents: Claude Code refactors the login page, Kimi fixes a payment callback, Codex adds tests. Tool calls fold into one line in the conversation, and menus from the terminal show up as buttons.",
        demo: "chat",
      },
      {
        time: "10:30", tone: "warn", eyes: { tone: "pending" }, label: "Pending approval",
        title: "Kimi wants to run a command that needs approval",
        body: "The island shows an alert at the top of the screen. Click Open to go to that session. The approval card lists what the command will touch, such as paths outside the workspace, deleted files or network access.",
        demo: "island-alert",
      },
      {
        time: "12:30", tone: "run", eyes: { tone: "running", busy: true }, label: "Away from the desk",
        title: "Handle the approval on your phone",
        body: "Turn on phone access in Settings and pair your phone once by scanning a QR code. After that, Bark (an iPhone app) sends a notification when a session needs you. It opens the same conversation in your phone's browser, where you can approve commands, answer questions or send messages.",
        demo: "mobile",
      },
      {
        time: "14:00", tone: "done", eyes: { tone: "waiting" }, label: "Back at the desk",
        title: "Review the changes",
        body: "The Changes tab in the right panel lists the files that were edited. Open one to see the diff. If you disagree with a line, comment on it and add the comment to the message box for the agent. Files, Terminal and Browser tabs sit in the same row. Claude Code, Codex and Kimi sessions can operate that browser on their own.",
        demo: "panel",
      },
      {
        time: "17:30", tone: "err", eyes: { tone: "running", quota: 92 }, label: "Quota running low",
        title: "Quota is almost used up",
        body: "The quota ring on the island shows 5-hour and 7-day quota for Claude Code, Codex and Kimi, including separate weekly quota for models such as Opus. Past 85%, the ring turns red. Every agent except Gemini CLI can keep several accounts to switch between, or you can use an API relay instead.",
      },
      {
        time: "19:00", tone: "done", eyes: { tone: "running", cue: "star" }, label: "Wrap up",
        title: "End of the day",
        body: "When a session finishes, its icon on the island shows a dot. Once you've looked at it, mark it done and it moves to Done, where you can find it later. If something is still running, turn on keep-awake: the computer won't sleep, and the screen still turns off.",
      },
    ],
    agents: {
      eyebrow: "Five agents",
      title: "Works with five AI coding CLIs",
      body: "If a CLI isn't installed yet, you can install it and sign in from Settings. Gemini CLI needs an API key. Meowo sets up the hooks it needs. The agents differ in what they support, such as quota readouts, multiple accounts and letting the agent operate the browser. The features page compares them.",
      more: "Compare agents",
    },
    trust: {
      eyebrow: "Local-first",
      title: "Your data stays on your computer",
      cells: [
        { big: "0", title: "servers of our own", body: "Sessions and settings are stored only in ~/.meowo/ on your machine. The one exception is Bark push: when it's on, each push (session title, the agent's last message, a link to the session) goes through Bark's server." },
        { big: "MIT", title: "Open source", body: "The source and the release process are on GitHub. Installers are built by GitHub Actions from the public source, and in-app updates check a minisign signature." },
      ],
      releases: (count, since) => ({
        big: String(count),
        title: "releases",
        body: `The first release came out in ${new Date(`${since}-01T00:00:00Z`).toLocaleString("en-US", { month: "long", year: "numeric", timeZone: "UTC" })}. Every release has notes in the changelog.`,
      }),
    },
    platHead: { eyebrow: "Platforms", title: "Windows and macOS" },
    plats: [
      { title: "Windows", body: "The island is a pill at the top of the screen. Click it to see sessions and quota. You can drag it left or right, or shrink it to a thin strip. It hides while a full-screen app is in front." },
      { title: "macOS", body: "On a Mac with a notch, the island sits around the notch. Without a notch, it lives in the menu bar and opens a panel when you click the icon. The menu-bar icon shows the same eyes." },
    ],
    more: "See all features",
  },
};

function NodeDemo({ demo, lang }: { demo: Demo; lang: Lang }) {
  if (demo === "mobile") {
    return (
      <div className="tl-demo tl-demo-phone">
        {/* 手机远程页的真实布局（app/src/demo/mobileEmbed.tsx） */}
        <div className="phone-shell">
          <DemoFrame lang={lang} page="mobile" />
        </div>
      </div>
    );
  }
  if (demo === "chat") {
    return (
      <div className="tl-demo tl-demo-chat">
        {/* 真实 ChatWindow + 假会话（app/src/demo/chatEmbed.tsx） */}
        <DemoFrame lang={lang} page="chat" />
      </div>
    );
  }
  return (
    <div className="tl-demo">
      {demo === "panel" ? (
        // 真实 ChatWindow，右侧面板开着更改页签（app/src/demo/panelEmbed.tsx）
        <DemoFrame lang={lang} page="panel" />
      ) : (
        // 真实灵动岛组件，定格在审批提醒那一刻（IslandStage scene="alert"）
        <DemoFrame lang={lang} page="island" scene="alert" />
      )}
    </div>
  );
}

export default async function HomeContent({ lang }: { lang: Lang }) {
  const [release, stats] = await Promise.all([getLatestRelease(), getReleaseStats()]);
  const d = getDict(lang);
  const c = CONTENT[lang];
  const trustCells = stats ? [...c.trust.cells, c.trust.releases(stats.count, stats.since)] : c.trust.cells;

  return (
    <main>
      {/* Hero：吉祥物眼睛（跟着鼠标看）+ 主张 + 双 CTA + 支持的五家 + 灵动岛演示 */}
      <section className="hero">
        <div className="container">
          <div className="hero-eyes">
            <Mascot tone="running" size={132} />
          </div>
          <span className="eyebrow">{c.hero.eyebrow}</span>
          <h1 className="h-display">
            {c.hero.h1a}
            <br />
            <span className="acc">{c.hero.accent}</span>
          </h1>
          <p className="lead">{c.hero.lead}</p>
          <div className="hero-cta">
            <DownloadButton
              lang={lang}
              windows={release?.windows ?? null}
              macosArm={release?.macosArm ?? null}
              macosIntel={release?.macosIntel ?? null}
              fallbackHref={withLang(lang, "/download")}
              className="btn btn-primary btn-lg"
            />
            <a className="btn btn-ghost btn-lg" href={REPO} target="_blank" rel="noopener noreferrer">
              <GitHubIcon />
              {c.hero.github}
            </a>
          </div>
          <p className="hero-note">
            {c.hero.note}
            {release ? ` · ${release.tag}` : ""}
          </p>
          <div className="hero-agents" aria-label={c.hero.works}>
            <span className="hero-agents-label">{c.hero.works}</span>
            {AGENTS.map((a) => (
              <span key={a.id} className="hero-agent">
                <AgentLogo id={a.id} size={22} />
                {a.name}
              </span>
            ))}
          </div>
          <div className="hero-stage">
            <div className="stage">
              {/* 灵动岛演示：iframe 里是应用真实的灵动岛组件 + 假数据（app/src/demo/IslandStage.tsx） */}
              <DemoFrame lang={lang} />
            </div>
          </div>
        </div>
      </section>

      {/* 一天的时间线：左边时间 + 那一刻的眼睛表情，右边说明 + 真实界面 */}
      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.day.eyebrow}</span>
            <h2 className="h1">{c.day.title}</h2>
            <p className="lead">{c.day.lead}</p>
          </div>
          <ol className="tl">
            {c.nodes.map((n) => (
              <li key={n.time} className="tl-node" data-tone={n.tone}>
                <div className="tl-rail">
                  <time className="tl-time">{n.time}</time>
                  <span className="tl-eyes">
                    <Mascot tone={n.eyes.tone} cue={n.eyes.cue} quota={n.eyes.quota} busy={n.eyes.busy} size={64} />
                  </span>
                </div>
                <div className="tl-body">
                  <span className="tl-label">{n.label}</span>
                  <h3 className="tl-title">{n.title}</h3>
                  <p className="tl-text">{n.body}</p>
                  {n.demo && (
                    <Reveal>
                      <NodeDemo demo={n.demo} lang={lang} />
                    </Reveal>
                  )}
                </div>
              </li>
            ))}
          </ol>
        </div>
      </section>

      {/* 五家 Agent */}
      <section className="section section-sunken">
        <div className="container">
          <div className="agents-band">
            <div>
              <span className="eyebrow">{c.agents.eyebrow}</span>
              <h2 className="h1">{c.agents.title}</h2>
              <p className="lead">{c.agents.body}</p>
              <Link className="btn btn-ghost agents-more" href={`${withLang(lang, "/features")}#agents`}>
                {c.agents.more} <ArrowRightIcon />
              </Link>
            </div>
            <ul className="agents-list">
              {AGENTS.map((a) => (
                <li key={a.id}>
                  <AgentLogo id={a.id} size={36} />
                  <span>{a.name}</span>
                </li>
              ))}
            </ul>
          </div>
        </div>
      </section>

      {/* 本地优先 / 可信 */}
      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.trust.eyebrow}</span>
            <h2 className="h1">{c.trust.title}</h2>
          </div>
          <div className="trust">
            {trustCells.map((t) => (
              <Reveal key={t.title}>
                <div className="trust-cell">
                  <div className="trust-big">{t.big}</div>
                  <div className="trust-title">{t.title}</div>
                  <p>{t.body}</p>
                </div>
              </Reveal>
            ))}
          </div>
        </div>
      </section>

      {/* 平台 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.platHead.eyebrow}</span>
            <h2 className="h1">{c.platHead.title}</h2>
          </div>
          <div className="grid grid-2">
            {c.plats.map((pl) => (
              <Reveal key={pl.title}>
                <div className="fcard plat-card">
                  <h3>{pl.title}</h3>
                  <p>{pl.body}</p>
                </div>
              </Reveal>
            ))}
          </div>
          <div className="center-cta">
            <Link className="btn btn-ghost" href={withLang(lang, "/features")}>
              {c.more} <ArrowRightIcon />
            </Link>
          </div>
        </div>
      </section>

      <CtaBand lang={lang} />
    </main>
  );
}
