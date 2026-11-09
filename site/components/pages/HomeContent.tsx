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
      h1a: "多开 AI 编程，",
      accent: "一切尽在计划之中",
      lead: "五家 AI 编程 CLI 的会话在一个对话窗里跑。灵动岛替你盯着，谁要你处理就叫你；人走开了，手机接着批。",
      github: "GitHub",
      note: "开源 · MIT · Windows 与 macOS · 无需预装 AI CLI",
      works: "支持",
    },
    day: {
      eyebrow: "一天里的 Meowo",
      title: "开着好几个 Agent 写代码，是这样过的",
      lead: "下面每一格都是应用里的真实界面，数据是演示用的。",
    },
    nodes: [
      {
        time: "09:00", tone: "run", eyes: { tone: "running", cue: "surprised" }, label: "开工",
        title: "三个会话，一起开",
        body: "在对话窗里选好 Agent 和目录，写下第一句话就开聊。Claude 重构登录页，Kimi 修支付回调，Codex 补单测，各跑各的。工具调用折成一行，终端弹的菜单变成按钮，不用在终端里翻屏。",
        demo: "chat",
      },
      {
        time: "10:30", tone: "warn", eyes: { tone: "pending" }, label: "待批准",
        title: "有人要你点头，灵动岛会叫你",
        body: "Kimi 要跑一条需要批准的命令。灵动岛从屏幕顶部弹出提醒，你在写别的也看得见，点「打开」就回到那个会话的审批卡。卡片标出这次会碰到哪里：工作区外的路径、删文件、联网。",
        demo: "island-alert",
      },
      {
        time: "12:30", tone: "run", eyes: { tone: "running", busy: true }, label: "离开工位",
        title: "吃饭路上，手机上批掉",
        body: "在设置里开好手机远程，手机扫一次码就配对，不用装 App。会话需要你时经 Bark（iPhone App）推到手机，点开就是同一个对话：批审批、答提问、回一句「继续」，会话不用等你回来。",
        demo: "mobile",
      },
      {
        time: "14:00", tone: "done", eyes: { tone: "waiting" }, label: "回到电脑",
        title: "看看它到底改了什么",
        body: "右侧面板的「更改」页签列出这次动过的文件，点开看 diff。哪行不对，就在那一行写评论，一键放进输入框发给 Agent。终端、文件、浏览器也在同一排页签里；Claude Code、Codex、Kimi 的会话还能自己操作这个浏览器，验证改动。",
        demo: "panel",
      },
      {
        time: "17:30", tone: "err", eyes: { tone: "running", quota: 92 }, label: "额度见底",
        title: "额度快用完，眼睛先发愁",
        body: "灵动岛的额度环显示 Claude Code、Codex、Kimi 的 5 小时和 7 天额度，Opus 这类模型单独的周额度也算在内。快用完时环变红，眼睛跟着发愁。同一个 Agent 可以存多个账号一键切换（Gemini CLI 除外），也可以接 API 中转。",
      },
      {
        time: "19:00", tone: "done", eyes: { tone: "running", cue: "star" }, label: "收工",
        title: "跑完的会话，明天接着看",
        body: "会话跑完，岛上那个头像亮起一个点，眼睛冒星星。点「标记完成」把它收进「已完成」，随时找回。还有会话没跑完？开着「运行时防止休眠」，电脑不会睡，屏幕照常熄灭。",
      },
    ],
    agents: {
      eyebrow: "五家 Agent",
      title: "装哪家都行，一起用也行",
      body: "没装也没关系：在设置里一键安装、登录（Gemini CLI 填 API Key），hooks 自动接好。各家 CLI 能力不完全一样，比如额度读数、多账号、Agent 操作浏览器，功能页有逐项对照。",
      more: "看能力对照",
    },
    trust: {
      eyebrow: "本地优先",
      title: "你的会话，留在你的电脑上",
      cells: [
        { big: "0", title: "台自己的服务器", body: "会话和设置只存在本机的 ~/.meowo/。只有你开了 Bark 推送时，推送内容（会话标题、Agent 最后一句话、打开会话的链接）会经过 Bark 的服务器。" },
        { big: "MIT", title: "开源，构建可查", body: "源码和发版流程都在 GitHub 上。安装包由 GitHub Actions 从公开源码构建，应用内更新有 minisign 签名校验。" },
      ],
      releases: (count, since) => ({ big: String(count), title: "个版本", body: `从 ${since.slice(0, 4)} 年 ${Number(since.slice(5, 7))} 月起持续发版，更新日志里有每一版的说明。` }),
    },
    platHead: { eyebrow: "平台", title: "Windows 和 macOS，都是这双眼睛" },
    plats: [
      { title: "Windows", body: "停在屏幕顶部的一条胶囊。点开看会话和额度，能左右拖动换位置，也能收成一道细线。全屏程序在前台时自动藏起来。" },
      { title: "macOS", body: "有刘海的 Mac 上，岛包住刘海；没刘海时住在菜单栏，点图标弹出面板。菜单栏图标也是这双眼睛，表情和岛上一样。" },
    ],
    more: "查看全部功能",
  },
  en: {
    hero: {
      eyebrow: "// AI coding agent workbench",
      h1a: "Run AI coding in parallel,",
      accent: "all under control",
      lead: "Sessions from five AI coding CLIs run in one chat window. The island keeps watch and calls you when something needs you; step away, and your phone takes the approvals.",
      github: "GitHub",
      note: "Open source · MIT · Windows & macOS · no AI CLI needed upfront",
      works: "Works with",
    },
    day: {
      eyebrow: "A day with Meowo",
      title: "What a day with several agents running looks like",
      lead: "Every frame below is the real app UI, with demo data.",
    },
    nodes: [
      {
        time: "09:00", tone: "run", eyes: { tone: "running", cue: "surprised" }, label: "Kickoff",
        title: "Three sessions, all at once",
        body: "Pick an agent and a folder in the chat window and type your first message. Claude refactors the login page, Kimi fixes a payment callback, Codex fills in tests, each on its own. Tool calls fold into one line and terminal menus become buttons, so there's no scrolling through a terminal.",
        demo: "chat",
      },
      {
        time: "10:30", tone: "warn", eyes: { tone: "pending" }, label: "Needs approval",
        title: "When someone needs a yes, the island calls you",
        body: "Kimi wants to run a command that needs approval. The island drops an alert from the top of the screen, visible while you work on something else; click Open to jump to that session’s approval card. The card shows what the action touches: paths outside the workspace, file deletion, network.",
        demo: "island-alert",
      },
      {
        time: "12:30", tone: "run", eyes: { tone: "running", busy: true }, label: "Away from desk",
        title: "Approve it from your phone at lunch",
        body: "Turn on phone access in Settings and scan the QR code once to pair. No app to install. When a session needs you, Bark (an iPhone app) pushes it to your phone and opens the same conversation: approve, answer a question, or say “go on”. The session doesn't wait for you to get back.",
        demo: "mobile",
      },
      {
        time: "14:00", tone: "done", eyes: { tone: "waiting" }, label: "Back at the desk",
        title: "See what it actually changed",
        body: "The Changes tab in the right panel lists the files touched; open one to see the diff. If a line looks wrong, comment on it and drop the comment into the composer for the agent. Terminal, Files and Browser share the same row of tabs, and Claude Code, Codex and Kimi sessions can drive that browser themselves to check their work.",
        demo: "panel",
      },
      {
        time: "17:30", tone: "err", eyes: { tone: "running", quota: 92 }, label: "Quota running low",
        title: "Quota almost gone? The eyes worry first",
        body: "The island's quota ring shows 5-hour and 7-day quota for Claude Code, Codex and Kimi, including per-model weekly quota such as Opus. Near the limit the ring turns red and the eyes start to worry. Keep several accounts per agent and switch in one click (except Gemini CLI), or connect an API relay.",
      },
      {
        time: "19:00", tone: "done", eyes: { tone: "running", cue: "star" }, label: "Wrap-up",
        title: "Finished sessions wait for tomorrow",
        body: "When a session finishes, its icon on the island lights up and the eyes go starry. Mark it done and it moves to Done, ready to bring back. Something still running? With keep-awake on, the computer won't sleep, though the screen still turns off.",
      },
    ],
    agents: {
      eyebrow: "Five agents",
      title: "Use any of them, or all of them together",
      body: "Not installed yet? Install and sign in from Settings in one click (Gemini CLI takes an API key), and the hooks are wired for you. The CLIs don't all support the same things, such as quota readouts, multiple accounts or the agent driving the browser; the features page compares them item by item.",
      more: "Compare agents",
    },
    trust: {
      eyebrow: "Local-first",
      title: "Your sessions stay on your computer",
      cells: [
        { big: "0", title: "servers of our own", body: "Sessions and settings live only in ~/.meowo/ on your machine. The one exception: if you turn on Bark push, each push (session title, the agent's last message, a link back to the session) goes through Bark's server." },
        { big: "MIT", title: "Open source, verifiable builds", body: "The source and the release process are on GitHub. Installers are built by GitHub Actions from the public source, and in-app updates are verified with a minisign signature." },
      ],
      releases: (count, since) => ({
        big: String(count),
        title: "releases",
        body: `Shipping steadily since ${new Date(`${since}-01T00:00:00Z`).toLocaleString("en-US", { month: "long", year: "numeric", timeZone: "UTC" })}; the changelog covers every release.`,
      }),
    },
    platHead: { eyebrow: "Platforms", title: "The same eyes on Windows and macOS" },
    plats: [
      { title: "Windows", body: "A slim pill at the top of the screen. Click it for sessions and quota, drag it sideways, or shrink it to a thin strip. It hides while a full-screen app is in front." },
      { title: "macOS", body: "On a Mac with a notch, the island wraps the notch; without one, it lives in the menu bar and opens a panel on click. The menu-bar icon is the same pair of eyes, with the same expressions." },
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
