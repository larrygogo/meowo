import Link from "next/link";
import { getLatestRelease } from "@/lib/release";
import { getDict, withLang, type Lang } from "@/lib/i18n";
import { ArrowRightIcon } from "@/components/icons";
import DownloadButton from "@/components/DownloadButton";
import FeatureGrid from "@/components/FeatureGrid";
import Reveal from "@/components/Reveal";
import CtaBand from "@/components/CtaBand";
import ProductShowcase from "@/components/ProductShowcase";
import SupportedAgents from "@/components/SupportedAgents";
import ThemeShowcase from "@/components/ThemeShowcase";
import { StickerWindow, CollapsedStrip, TerminalMock } from "@/components/screenshots";
import type { CardData } from "@/components/screenshots/StickerWindow";

type Content = {
  hero: { pill: string; h1a: string; grad: string; lead: string; seeFeatures: string; note: string };
  problems: { title: string; body: string }[];
  scenesHead: { eyebrow: string; title: string; lead: string };
  sceneA: { eyebrow: string; title: string; body: string };
  sceneB: { eyebrow: string; title: string; body: string; checks: string[] };
  sceneC: { eyebrow: string; title: string; body: string };
  sceneF: { eyebrow: string; title: string; body: string };
  accountsHead: { eyebrow: string; title: string; lead: string };
  acct1: { title: string; body: string; rows: { av: string; bg: string; name: string; badge: string; cls: string }[] };
  acct2: { title: string; body: string; rows: { av: string; bg: string; name: string; badge: string; cls: string }[] };
  featGridHead: { eyebrow: string; title: string };
  cards: CardData[]; // 3 张运行卡
  cardWaiting: CardData;
  cardMenu: CardData;
};

const CONTENT: Record<Lang, Content> = {
  zh: {
    hero: {
      pill: "开源 · MIT · Windows 与 macOS",
      h1a: "多开 AI 编程",
      grad: "一切尽在计划之中",
      lead: "本地优先的 AI 编程代理工作台。展开是桌面贴纸，收起是电子红绿灯，点一下直达对应终端。",
      seeFeatures: "查看功能",
      note: "无需预装 AI CLI · 应用内一键安装、登录与接入",
    },
    problems: [
      { title: "会话散在多个终端窗口", body: "多个 AI 编程代理各跑各的。想知道某个会话到哪一步了，得逐个窗口切过去看。" },
      { title: "等待确认的会话容易被忽略", body: "会话在等一个确认，或者工具调用失败停住了。终端被别的窗口压着，几分钟后才发现。" },
      { title: "常用操作离不开命令行", body: "换项目、启动不同的 AI 工具、恢复旧会话，都要反复切目录、找会话 ID、输入不同命令。" },
    ],
    scenesHead: { eyebrow: "一个工作台", title: "接住每一个会话", lead: "从桌面一角的状态，到点一下就回到终端，常用操作全部就位。" },
    sceneA: { eyebrow: "窗口形态", title: "展开是桌面贴纸，收起是电子红绿灯", body: "需要看细节时，它是钉在桌面一角的贴纸，卡片、状态、用量一览无余。拖到屏幕边缘收起，就缩成一条竖排的电子红绿灯——红、黄、绿三色，一眼看清哪个会话报错、哪个在等你、哪个还在跑。鼠标悬停立刻展开。" },
    sceneB: {
      eyebrow: "点击直达",
      title: "点一下会话，跳到它所在的终端",
      body: "每个会话跑在各自的终端里——不同项目、不同 AI 工具。点卡片，Meowo 直接把你带到它所在的那个终端标签页，不用在一堆窗口里翻找。",
      checks: [
        "Windows 切到 Windows Terminal 的对应 tab，macOS 聚焦 Terminal / iTerm2",
        "开启系统通知后，点通知同样一步直达该会话",
        "已断开的会话，自动回到原目录并按对应工具续接",
      ],
    },
    sceneC: { eyebrow: "尽在掌握", title: "配额与上下文，都在计划之中", body: "底栏实时显示 5 小时 / 7 天配额的使用比例，越接近上限颜色越偏红；每张卡片显示会话的上下文已用百分比。快到限额、上下文快满，你都提前知道——不用焦虑，也不会被突然中断打个措手不及。" },
    sceneF: { eyebrow: "会话菜单", title: "常用操作，全集成进一个菜单", body: "右键卡片，或点右上角的 ⋮：一键新建会话、打开项目目录、加星置顶、写一条只存在本地的便签、改名、归档。想做的都在这——不用导出切换，也不用回终端敲命令。" },
    accountsHead: { eyebrow: "账号与网络", title: "多账号一键切，网络自己说了算", lead: "官方账号、API 中转、按工具设置代理，全部在设置里点选，不碰配置文件。" },
    acct1: {
      title: "官方多账号，一键切换",
      body: "同一个工具保存多个官方账号，各自独立登录与会话历史，互不影响。点一下切换，配额、登录状态立刻跟着走。",
      rows: [
        { av: "工", bg: "#d97757", name: "Claude · 工作", badge: "使用中", cls: "on" },
        { av: "个", bg: "#5b8db8", name: "Claude · 个人", badge: "切换", cls: "off" },
        { av: "C", bg: "#6fae6a", name: "Codex · 默认账号", badge: "切换", cls: "off" },
      ],
    },
    acct2: {
      title: "API 中转 + 按工具代理",
      body: "没有官方账号也能用：按模型接入 API 中转，配置期间仍走官方账号。每个工具还能单独走直连、跟随系统或自定义代理。",
      rows: [
        { av: "↳", bg: "#7a5bb8", name: "Opus · API 中转", badge: "中转", cls: "relay" },
        { av: "P", bg: "#0f9e78", name: "代理 · SOCKS5", badge: "自定义", cls: "on" },
        { av: "≡", bg: "#8a938e", name: "其余工具", badge: "跟随系统", cls: "off" },
      ],
    },
    featGridHead: { eyebrow: "完整工作流", title: "从会话到环境，一处管理" },
    cards: [
      { title: "重构吸边状态机", repo: "meowo", provider: "claude", state: "running", pct: 62, aiText: "把状态机拆成 3 个纯函数，正在补吸附边界单测。", time: "刚刚", model: "claude-opus-4" },
      { title: "接入账号用量面板", repo: "autopilot", provider: "codex", state: "waiting", pct: 43, aiText: "要应用这 3 处修改吗？(y/n)", time: "刚刚" },
      { title: "升级 tauri 到 2.3", repo: "cc-relay", provider: "kimi", state: "idle", aiText: "已更新 Cargo.toml，等你确认几处 breaking change。", time: "12 分钟前" },
    ],
    cardWaiting: { title: "接入账号用量面板", repo: "autopilot", provider: "codex", state: "waiting", pct: 43, aiText: "要应用这 3 处修改吗？(y/n)", time: "刚刚" },
    cardMenu: { title: "重构吸边状态机", repo: "meowo", provider: "claude", state: "running", pct: 62, aiText: "把状态机拆成 3 个纯函数，正在补吸附边界单测。", time: "刚刚", note: "记得先确认 API key", starred: true },
  },
  en: {
    hero: {
      pill: "Open source · MIT · Windows & macOS",
      h1a: "Run AI coding in parallel",
      grad: "all under control",
      lead: "A local-first workbench for AI coding agents. A sticker when expanded, an electronic traffic light when collapsed — one click jumps to the terminal.",
      seeFeatures: "See features",
      note: "No AI CLI needed upfront · install, sign in & connect in-app",
    },
    problems: [
      { title: "Sessions scattered across terminals", body: "Multiple AI coding agents each run on their own. To see where a session is, you switch to each window one by one." },
      { title: "Sessions awaiting a reply get missed", body: "A session waits for confirmation, or stalls on a failed tool call. Its terminal is buried under other windows — you notice minutes later." },
      { title: "Common actions need the command line", body: "Switching projects, launching different AI tools, resuming old sessions — all mean cd-ing around, hunting session IDs, and typing different commands." },
    ],
    scenesHead: { eyebrow: "One workbench", title: "Catches every session", lead: "From status in a desktop corner to one click back to the terminal — every common action is in place." },
    sceneA: { eyebrow: "Window form", title: "A sticker expanded, a traffic light collapsed", body: "When you need detail, it's a sticker pinned to a corner of your desktop — cards, status, usage at a glance. Drag it to a screen edge to collapse into a vertical electronic traffic light: red, amber, green tell you which session errored, which needs you, which is still running. Hover to expand instantly." },
    sceneB: {
      eyebrow: "Click to jump",
      title: "Click a session, land in its terminal",
      body: "Each session runs in its own terminal — different projects, different AI tools. Click a card and Meowo takes you straight to the terminal tab it lives in, no digging through windows.",
      checks: [
        "Windows switches to the matching Windows Terminal tab; macOS focuses Terminal / iTerm2",
        "With notifications on, clicking one jumps straight to that session too",
        "Disconnected sessions return to their directory and resume the tool's way",
      ],
    },
    sceneC: { eyebrow: "Under control", title: "Quota and context, all in the plan", body: "The bottom bar shows 5-hour / 7-day quota usage in real time, redder as it nears the cap; each card shows the session's context usage. You know before you hit a limit or fill the context — no anxiety, no sudden interruption catching you off guard." },
    sceneF: { eyebrow: "Session menu", title: "Every common action, in one menu", body: "Right-click a card, or the ⋮ at the top right: new session, open project directory, star to pin, jot a local-only note, rename, archive. Whatever you need is here — no exporting, switching, or going back to type commands." },
    accountsHead: { eyebrow: "Accounts & network", title: "Switch accounts in a click; the network is yours to decide", lead: "Official accounts, API relay, per-tool proxy — all point-and-click in settings, never touching a config file." },
    acct1: {
      title: "Official accounts, one-click switch",
      body: "Keep several official accounts per tool, each with its own login and session history. Switch with a click and quota + login state follow along.",
      rows: [
        { av: "W", bg: "#d97757", name: "Claude · Work", badge: "In use", cls: "on" },
        { av: "P", bg: "#5b8db8", name: "Claude · Personal", badge: "Switch", cls: "off" },
        { av: "C", bg: "#6fae6a", name: "Codex · Default", badge: "Switch", cls: "off" },
      ],
    },
    acct2: {
      title: "API relay + per-tool proxy",
      body: "No official account? Connect an API relay per model — still using the official account while you set it up. Each tool can also go direct, follow-system, or use a custom proxy.",
      rows: [
        { av: "↳", bg: "#7a5bb8", name: "Opus · API relay", badge: "Relay", cls: "relay" },
        { av: "P", bg: "#0f9e78", name: "Proxy · SOCKS5", badge: "Custom", cls: "on" },
        { av: "≡", bg: "#8a938e", name: "Other tools", badge: "System", cls: "off" },
      ],
    },
    featGridHead: { eyebrow: "Full workflow", title: "From sessions to environment, managed in one place" },
    cards: [
      { title: "Refactor edge-snap state machine", repo: "meowo", provider: "claude", state: "running", pct: 62, aiText: "Split the state machine into 3 pure functions; adding boundary tests.", time: "just now", model: "claude-opus-4" },
      { title: "Wire up the usage panel", repo: "autopilot", provider: "codex", state: "waiting", pct: 43, aiText: "Apply these 3 changes? (y/n)", time: "just now" },
      { title: "Bump tauri to 2.3", repo: "cc-relay", provider: "kimi", state: "idle", aiText: "Updated Cargo.toml; a few breaking changes to confirm.", time: "12 min ago" },
    ],
    cardWaiting: { title: "Wire up the usage panel", repo: "autopilot", provider: "codex", state: "waiting", pct: 43, aiText: "Apply these 3 changes? (y/n)", time: "just now" },
    cardMenu: { title: "Refactor edge-snap state machine", repo: "meowo", provider: "claude", state: "running", pct: 62, aiText: "Split the state machine into 3 pure functions; adding boundary tests.", time: "just now", note: "Confirm the API key first", starred: true },
  },
};

export default async function HomeContent({ lang }: { lang: Lang }) {
  const release = await getLatestRelease();
  const d = getDict(lang);
  const c = CONTENT[lang];

  return (
    <main>
      {/* Hero */}
      <section className="hero-dark">
        <div className="container">
          <span className="pill pill-dark">
            <span className="dot" />
            {c.hero.pill}
            {release ? ` · ${release.tag}` : ""}
          </span>
          <h1 className="h-display">
            {c.hero.h1a}
            <br />
            <span className="grad">{c.hero.grad}</span>
          </h1>
          <p className="lead lead-light">{c.hero.lead}</p>
          <div className="hero-cta">
            <DownloadButton
              lang={lang}
              windows={release?.windows ?? null}
              macos={release?.macos ?? null}
              fallbackHref={withLang(lang, "/download")}
              className="btn btn-light btn-lg"
            />
            <Link className="btn btn-ghost-light btn-lg" href={withLang(lang, "/features")}>
              {c.hero.seeFeatures} <ArrowRightIcon />
            </Link>
          </div>
          <p className="hero-note">{c.hero.note}</p>
          <ProductShowcase className="hero-showcase" />
        </div>
      </section>

      <SupportedAgents lang={lang} />

      {/* 痛点 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{lang === "en" ? "Why Meowo" : "为什么是 Meowo"}</span>
            <h2 className="h1">{lang === "en" ? "Running AI agents in parallel shouldn't be this tiring" : "并行使用 AI 编程代理，本不该这么累"}</h2>
          </div>
          <div className="grid grid-3">
            {c.problems.map((p) => (
              <Reveal key={p.title}>
                <div className="fcard">
                  <h3>{p.title}</h3>
                  <p>{p.body}</p>
                </div>
              </Reveal>
            ))}
          </div>
        </div>
      </section>

      {/* 场景展示 */}
      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.scenesHead.eyebrow}</span>
            <h2 className="h1">{c.scenesHead.title}</h2>
            <p className="lead">{c.scenesHead.lead}</p>
          </div>

          <div className="scenes">
            {/* a. 双形态 */}
            <Reveal>
              <div className="scene">
                <div className="scene-shot">
                  <div className="scene-stage stage-dark forms-stage">
                    <StickerWindow lang={lang} activeTab="all" cards={c.cards.slice(0, 2)} />
                    <CollapsedStrip edge="right" className="forms-edge-strip" />
                  </div>
                </div>
                <div className="scene-text">
                  <span className="eyebrow">{c.sceneA.eyebrow}</span>
                  <h3 className="h2">{c.sceneA.title}</h3>
                  <p className="lead">{c.sceneA.body}</p>
                </div>
              </div>
            </Reveal>

            {/* b. 点击直达 */}
            <Reveal>
              <div className="scene scene-rev">
                <div className="scene-shot">
                  <div className="scene-stage stage-dark">
                    <div className="route-scene">
                      <StickerWindow lang={lang} activeTab="waiting" cards={[c.cardWaiting]} />
                      <div className="route-arrow">
                        <ArrowRightIcon style={{ transform: "rotate(90deg)" }} />
                      </div>
                      <TerminalMock lang={lang} />
                    </div>
                  </div>
                </div>
                <div className="scene-text">
                  <span className="eyebrow">{c.sceneB.eyebrow}</span>
                  <h3 className="h2">{c.sceneB.title}</h3>
                  <p className="lead">{c.sceneB.body}</p>
                  <ul className="checklist">
                    {c.sceneB.checks.map((ck) => (
                      <li key={ck}>
                        <span className="ck">
                          <ArrowRightIcon />
                        </span>
                        <span>{ck}</span>
                      </li>
                    ))}
                  </ul>
                </div>
              </div>
            </Reveal>

            {/* c. 用量与上下文 */}
            <Reveal>
              <div className="scene">
                <div className="scene-shot">
                  <div className="scene-stage stage-dark">
                    <StickerWindow lang={lang} activeTab="all" cards={c.cards} />
                  </div>
                </div>
                <div className="scene-text">
                  <span className="eyebrow">{c.sceneC.eyebrow}</span>
                  <h3 className="h2">{c.sceneC.title}</h3>
                  <p className="lead">{c.sceneC.body}</p>
                </div>
              </div>
            </Reveal>

            {/* f. 会话菜单 */}
            <Reveal>
              <div className="scene scene-rev">
                <div className="scene-shot">
                  <div className="scene-stage" style={{ paddingBottom: 64 }}>
                    <StickerWindow lang={lang} activeTab="all" cards={[c.cardMenu]} showNote showMenu />
                  </div>
                </div>
                <div className="scene-text">
                  <span className="eyebrow">{c.sceneF.eyebrow}</span>
                  <h3 className="h2">{c.sceneF.title}</h3>
                  <p className="lead">{c.sceneF.body}</p>
                </div>
              </div>
            </Reveal>
          </div>
        </div>
      </section>

      {/* 账号与网络 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.accountsHead.eyebrow}</span>
            <h2 className="h1">{c.accountsHead.title}</h2>
            <p className="lead">{c.accountsHead.lead}</p>
          </div>
          <div className="accounts">
            {[c.acct1, c.acct2].map((card) => (
              <Reveal key={card.title}>
                <div className="acct-card">
                  <h3>{card.title}</h3>
                  <p>{card.body}</p>
                  <div className="acct-rows">
                    {card.rows.map((r) => (
                      <div className="acct-row" key={r.name}>
                        <span className="avatar" style={{ background: r.bg }}>{r.av}</span>
                        <span className="aname">{r.name}</span>
                        <span className={`abadge ${r.cls}`}>{r.badge}</span>
                      </div>
                    ))}
                  </div>
                </div>
              </Reveal>
            ))}
          </div>
        </div>
      </section>

      {/* 多风格多配色 */}
      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{d.theme.eyebrowHome}</span>
            <h2 className="h1">{d.theme.headingHome}</h2>
            <p className="lead">{d.theme.subHome}</p>
          </div>
          <Reveal>
            <ThemeShowcase lang={lang} />
          </Reveal>
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
          <div style={{ textAlign: "center", marginTop: 44 }}>
            <Link className="btn btn-ghost" href={withLang(lang, "/features")}>
              {d.featuresMore} <ArrowRightIcon />
            </Link>
          </div>
        </div>
      </section>

      <CtaBand lang={lang} />
    </main>
  );
}
