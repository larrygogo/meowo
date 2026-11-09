import Link from "next/link";
import { REPO } from "@/lib/site";
import { getLatestRelease } from "@/lib/release";
import { getDict, withLang, type Lang } from "@/lib/i18n";
import { ArrowRightIcon, GitHubIcon } from "@/components/icons";
import DownloadButton from "@/components/DownloadButton";
import CheckItem from "@/components/CheckItem";
import FeatureGrid from "@/components/FeatureGrid";
import Reveal from "@/components/Reveal";
import CtaBand from "@/components/CtaBand";
import DemoFrame from "@/components/DemoFrame";

type SemTone = "err" | "warn" | "ok";
type Sec = { eyebrow: string; title: string; body: string; checks: string[] };

type Content = {
  hero: { eyebrow: string; h1a: string; accent: string; lead: string; github: string; note: string };
  sem: { tone: SemTone; label: string; text: string }[]; // 三色状态语义条：红 / 黄 / 绿
  chat: Sec;
  panel: Sec;
  mobile: Sec;
  featGridHead: { eyebrow: string; title: string };
  platHead: { eyebrow: string; title: string };
  win: { title: string; body: string; checks: string[] };
  mac: { title: string; body: string; checks: string[] };
};

// 首页主张（2026-11-09 用户定）：完整工作台——对话窗、托管终端、手机远程、五家 agent 都在一处；
// 灵动岛是招牌视觉，放在 Hero 演示里，不再是主张本身。只写已发版的能力。
const CONTENT: Record<Lang, Content> = {
  zh: {
    hero: {
      eyebrow: "// AI 编程代理工作台",
      h1a: "多开 AI 编程，",
      accent: "一切尽在计划之中",
      lead: "Claude Code、Codex、Kimi、Gemini CLI、OpenCode 的会话都在一个对话窗里跑。灵动岛常驻屏幕顶部，谁在等你一眼可见；离开电脑，手机上也能接着推进。",
      github: "GitHub",
      note: "开源 · MIT · Windows 与 macOS · 无需预装 AI CLI",
    },
    sem: [
      { tone: "err", label: "ERROR · 出错", text: "有会话出错停住了，红色第一时间跳出来，不会被其他窗口盖过去。" },
      { tone: "warn", label: "WAITING · 等你处理", text: "有会话在等你批准或回答，黄色提醒，灵动岛会弹出来告诉你。" },
      { tone: "ok", label: "RUNNING · 运行中", text: "其余会话正常推进，绿色表示不用管，安心做手头的事。" },
    ],
    chat: {
      eyebrow: "对话窗口",
      title: "会话在这里跑，不用翻终端",
      body: "选好 Agent 和目录，写下第一句话就开聊。消息、工具调用、子任务各就各位，等批准的命令、Agent 的提问、终端弹的菜单，都变成可以直接点的按钮。",
      checks: [
        "工具调用折叠成一行，展开看完整输入输出；子任务有独立时间线",
        "审批卡标出这次操作碰到哪里：工作区外的路径、删文件、联网",
        "Agent 的选择题直接在卡片上作答；信任确认、模型菜单也是按钮",
        "可以贴图片，也能截图后框出位置、写好说明再发",
        "会话跑在托管终端里，随时切到终端视图亲手操作",
      ],
    },
    panel: {
      eyebrow: "右侧面板",
      title: "改动、文件、终端、浏览器，就在对话旁边",
      body: "对话窗右侧是一排页签。看 Agent 改了什么、翻项目文件、开个终端跑命令、在浏览器里看效果，都不用离开这个会话。",
      checks: [
        "改动页看 diff，对某一行写评论，一键放进输入框发给 Agent",
        "文件页浏览项目、预览文件，菜单里把路径添加到对话",
        "在会话目录里开终端，可以开多个",
        "浏览器打开 localhost；Agent 能自己点击、输入、截图，你看得到它的鼠标，随时接管",
        "每个会话记住自己开了哪些页签，快捷键一键打开改动、文件、浏览器",
        "标题栏显示分支和 Pull Request，一键用编辑器打开项目",
      ],
    },
    mobile: {
      eyebrow: "手机远程",
      title: "离开电脑，会话照样往前走",
      body: "在设置里开启手机远程，手机扫码就能打开 Meowo 的手机页，不用装 App。看对话、发消息、批审批，和在电脑前一样。",
      checks: [
        "命令审批和 Agent 的提问，在手机上直接处理",
        "新建、分叉会话，输入框打 @ 引用文件",
        "看托管终端的画面和这次的改动",
        "会话需要你时，经 Bark 推送到手机",
        "局域网或 Tailscale 下可用，能添加到主屏幕",
      ],
    },
    featGridHead: { eyebrow: "完整工作流", title: "从安装到收尾，一处管理" },
    platHead: { eyebrow: "平台", title: "Windows 和 macOS，都是灵动岛" },
    win: {
      title: "Windows · 灵动岛",
      body: "停在屏幕顶部的一条胶囊，谁在跑、谁在等你一眼可见。",
      checks: [
        "点一下展开面板，看会话和各家额度，也能新建会话",
        "会话需要你时弹出提醒，点头像直接打开会话",
        "可以左右拖动位置；极简模式只留一条细线",
      ],
    },
    mac: {
      title: "macOS · 灵动岛",
      body: "有刘海的 Mac 上包住刘海，没刘海时收进菜单栏。",
      checks: [
        "有刘海时，额度和会话头像分在刘海两侧",
        "没刘海时，点菜单栏图标在下方弹出面板",
        "菜单栏上一双会动的眼睛，旁边显示最要紧的会话",
      ],
    },
  },
  en: {
    hero: {
      eyebrow: "// AI coding agent workbench",
      h1a: "Run AI coding in parallel,",
      accent: "all under control",
      lead: "Claude Code, Codex, Kimi, Gemini CLI and OpenCode sessions all run in one chat window. The island at the top of your screen shows who's waiting on you, and your phone keeps things moving when you step away.",
      github: "GitHub",
      note: "Open source · MIT · Windows & macOS · no AI CLI needed upfront",
    },
    sem: [
      { tone: "err", label: "ERROR", text: "A session errored out and stopped. Red jumps out first, never buried under other windows." },
      { tone: "warn", label: "WAITING", text: "A session is waiting for your approval or answer. Amber reminds you, and the island pops up to tell you." },
      { tone: "ok", label: "RUNNING", text: "Everything else is making progress. Green means leave it alone and focus on your work." },
    ],
    chat: {
      eyebrow: "Chat window",
      title: "Sessions run here, no terminal scrolling",
      body: "Pick an agent and a folder, then type your first message. Messages, tool calls and subagents each have their place, and pending commands, the agent's questions and terminal menus all become buttons you can click.",
      checks: [
        "Tool calls fold into one line; expand for full input and output. Subagents get their own timeline",
        "Approval cards show what the action touches: paths outside the workspace, file deletion, network",
        "Answer the agent's multiple-choice questions on the card; trust prompts and model menus are buttons too",
        "Paste images, or take a screenshot, mark it up and add a note before sending",
        "Sessions run in a managed terminal; switch to the terminal view and type yourself anytime",
      ],
    },
    panel: {
      eyebrow: "Right panel",
      title: "Changes, files, terminal and browser, right next to the chat",
      body: "A row of tabs sits on the right of the chat window. See what the agent changed, browse project files, run a command in a terminal, or check the result in a browser without leaving the session.",
      checks: [
        "The Changes tab shows the diff. Comment on a line and drop it into the composer for the agent",
        "The Files tab browses and previews the project; add a path to the chat from the menu",
        "Open terminals in the session folder, as many as you like",
        "The browser opens localhost. The agent can click, type and take screenshots there; you see its cursor and can take over anytime",
        "Each session remembers its tabs; shortcuts open Changes, Files and Browser in one keystroke",
        "The title bar shows the branch and pull request, and opens the project in your editor",
      ],
    },
    mobile: {
      eyebrow: "Phone access",
      title: "Step away, and your sessions keep moving",
      body: "Turn on phone access in Settings and scan the QR code to open Meowo's phone page. No app to install. Read the conversation, send messages and approve commands just like at your desk.",
      checks: [
        "Handle command approvals and the agent's questions right on your phone",
        "Start and fork sessions; type @ in the composer to reference a file",
        "See the managed terminal and this session's changes",
        "Get a push on your phone through Bark when a session needs you",
        "Works over your LAN or Tailscale, and can be added to your home screen",
      ],
    },
    featGridHead: { eyebrow: "Full workflow", title: "From setup to wrap-up, all in one place" },
    platHead: { eyebrow: "Platforms", title: "An island on both Windows and macOS" },
    win: {
      title: "Windows · Island",
      body: "A slim pill at the top of the screen that shows who's running and who's waiting on you.",
      checks: [
        "Click it to expand a panel with sessions and quota, or start a new session",
        "Pops an alert when a session needs you; click an icon to open it",
        "Drag it sideways; minimal mode leaves just a thin strip",
      ],
    },
    mac: {
      title: "macOS · Island",
      body: "Wraps around the notch on Macs that have one; tucks into the menu bar on those that don't.",
      checks: [
        "With a notch, quota and session icons sit on either side of it",
        "Without one, click the menu-bar icon for a panel right below it",
        "A pair of animated eyes in the menu bar, next to your most urgent session",
      ],
    },
  },
};

export default async function HomeContent({ lang }: { lang: Lang }) {
  const release = await getLatestRelease();
  const d = getDict(lang);
  const c = CONTENT[lang];

  return (
    <main>
      {/* Hero：mono eyebrow + display + 双 CTA + 灵动岛示意 */}
      <section className="hero">
        <div className="container">
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
          <div className="hero-stage">
            <div className="stage">
              {/* 灵动岛演示：iframe 里是应用真实的灵动岛组件 + 假数据（app/src/demo/IslandStage.tsx） */}
              <DemoFrame lang={lang} />
            </div>
          </div>
        </div>
      </section>

      {/* 三色状态语义条 */}
      <section className="section-sm section-sunken">
        <div className="container">
          <div className="sem-strip">
            {c.sem.map((s) => (
              <Reveal key={s.label}>
                <div className={`sem sem-${s.tone}`}>
                  <span className="sem-tag">
                    <span className="sem-dot" />
                    {s.label}
                  </span>
                  <p>{s.text}</p>
                </div>
              </Reveal>
            ))}
          </div>
        </div>
      </section>

      {/* 对话窗口 */}
      <section className="section">
        <div className="container">
          <div className="split">
            <div className="split-text">
              <span className="eyebrow">{c.chat.eyebrow}</span>
              <h2 className="h2">{c.chat.title}</h2>
              <p className="lead">{c.chat.body}</p>
              <ul className="checklist">
                {c.chat.checks.map((x) => (
                  <CheckItem key={x}>{x}</CheckItem>
                ))}
              </ul>
            </div>
            <div className="split-media">
              {/* 对话窗口演示：iframe 里是应用真实的 ChatWindow + 假会话（app/src/demo/chatEmbed.tsx） */}
              <DemoFrame lang={lang} page="chat" />
            </div>
          </div>
        </div>
      </section>

      {/* 右侧面板：演示 1180 宽，放半栏字会小到看不清，所以整宽大图 + 要点排在图下 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.panel.eyebrow}</span>
            <h2 className="h1">{c.panel.title}</h2>
            <p className="lead">{c.panel.body}</p>
          </div>
          <Reveal>
            <div className="stage wide-demo">
              {/* iframe 里是真实 ChatWindow，右侧面板开着改动页签（app/src/demo/panelEmbed.tsx） */}
              <DemoFrame lang={lang} page="panel" />
            </div>
          </Reveal>
          <ul className="checklist wide-checks">
            {c.panel.checks.map((x) => (
              <CheckItem key={x}>{x}</CheckItem>
            ))}
          </ul>
        </div>
      </section>

      {/* 手机远程 */}
      <section className="section">
        <div className="container">
          <div className="split rev">
            <div className="split-media">
              {/* iframe 里是手机远程页的真实布局 + 同一段假会话（app/src/demo/mobileEmbed.tsx） */}
              <div className="phone-shell">
                <DemoFrame lang={lang} page="mobile" />
              </div>
            </div>
            <div className="split-text">
              <span className="eyebrow">{c.mobile.eyebrow}</span>
              <h2 className="h2">{c.mobile.title}</h2>
              <p className="lead">{c.mobile.body}</p>
              <ul className="checklist">
                {c.mobile.checks.map((x) => (
                  <CheckItem key={x}>{x}</CheckItem>
                ))}
              </ul>
            </div>
          </div>
        </div>
      </section>

      {/* 特性网格 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.featGridHead.eyebrow}</span>
            <h2 className="h1">{c.featGridHead.title}</h2>
          </div>
          <FeatureGrid lang={lang} />
          <div className="center-cta">
            <Link className="btn btn-ghost" href={withLang(lang, "/features")}>
              {d.featuresMore} <ArrowRightIcon />
            </Link>
          </div>
        </div>
      </section>

      {/* 平台 */}
      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.platHead.eyebrow}</span>
            <h2 className="h1">{c.platHead.title}</h2>
          </div>
          <div className="grid grid-2">
            {[c.win, c.mac].map((pl) => (
              <Reveal key={pl.title}>
                <div className="fcard plat-card">
                  <h3>{pl.title}</h3>
                  <p>{pl.body}</p>
                  <ul className="checklist">
                    {pl.checks.map((x) => (
                      <CheckItem key={x}>{x}</CheckItem>
                    ))}
                  </ul>
                </div>
              </Reveal>
            ))}
          </div>
        </div>
      </section>

      <CtaBand lang={lang} />
    </main>
  );
}
