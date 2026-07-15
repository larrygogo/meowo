import FeatureGrid from "@/components/FeatureGrid";
import Reveal from "@/components/Reveal";
import CtaBand from "@/components/CtaBand";
import ThemeShowcase from "@/components/ThemeShowcase";
import { CheckIcon } from "@/components/icons";
import { StickerWindow, CollapsedStrip } from "@/components/screenshots";
import type { CardData } from "@/components/screenshots/StickerWindow";
import { getDict, type Lang } from "@/lib/i18n";

function Check({ children }: { children: React.ReactNode }) {
  return (
    <li>
      <span className="ck">
        <CheckIcon />
      </span>
      <span>{children}</span>
    </li>
  );
}

type Sec = { eyebrow: string; title: string; body: string; checks: string[] };
type AcctRow = { av: string; bg: string; name: string; badge: string; cls: string };

type Content = {
  head: { eyebrow: string; title: string; lead: string };
  showcaseCards: CardData[];
  showcaseCap: string;
  overview: { eyebrow: string; title: string };
  s1: Sec;
  s1cards: CardData[];
  s2: Sec;
  s2card: CardData;
  s3: Sec;
  s3card: CardData;
  acctHead: { eyebrow: string; title: string; lead: string };
  acct1: { title: string; body: string; rows: AcctRow[] };
  acct2: { title: string; body: string; rows: AcctRow[] };
  proxyHead: { eyebrow: string; title: string; lead: string };
  proxy: { title: string; body: string }[];
  themeExtra: string;
  platHead: { eyebrow: string; title: string };
  win: { title: string; checks: string[] };
  mac: { title: string; checks: string[] };
};

const CONTENT: Record<Lang, Content> = {
  zh: {
    head: { eyebrow: "功能", title: "一个工作台，管理完整流程", lead: "从安装、登录 AI 编程代理，到查看状态、处理提醒、续接会话与切换账号，整个流程都在一个桌面工作台完成。" },
    showcaseCards: [
      { title: "重构吸边状态机", repo: "meowo", provider: "claude", state: "running", pct: 62, aiText: "把状态机拆成 3 个纯函数，正在补吸附边界单测。", time: "刚刚", model: "claude-opus-4" },
      { title: "接入账号用量面板", repo: "autopilot", provider: "codex", state: "waiting", pct: 43, aiText: "要应用这 3 处修改吗？(y/n)", time: "刚刚" },
      { title: "升级 tauri 到 2.3", repo: "cc-relay", provider: "kimi", state: "idle", aiText: "已更新 Cargo.toml，等你确认几处 breaking change。", time: "12 分钟前" },
      { title: "修复 statusline 兼容性", repo: "clawmo-ios", provider: "claude", state: "stopped", aiText: "兼容性修好并已合并，收工。", time: "3 小时前" },
    ],
    showcaseCap: "会话卡片、状态分类 tab、底栏用量读数",
    overview: { eyebrow: "概览", title: "功能一览" },
    s1: {
      eyebrow: "窗口形态",
      title: "展开是桌面贴纸，收起是电子红绿灯",
      body: "需要看细节时它是钉在桌面一角的贴纸；拖到屏幕边缘收起，就缩成一条竖排的电子红绿灯。红黄绿三色，一眼看清哪个会话报错、哪个在等你、哪个还在跑。",
      checks: ["拖到屏幕左 / 右 / 顶边松手，窗口缩成一条状态条，鼠标悬停展开", "红 = 报错、黄 = 待交互、绿 = 运行中，收起也不漏掉任何一个", "可以置顶；重启后沿用上次的窗口位置和吸附边", "macOS 上是菜单栏面板，图标显示运行中与待交互的会话数"],
    },
    s1cards: [
      { title: "重构吸边状态机", repo: "meowo", provider: "claude", state: "running", pct: 62, aiText: "把状态机拆成 3 个纯函数…", time: "刚刚" },
      { title: "接入账号用量面板", repo: "autopilot", provider: "codex", state: "waiting", pct: 43, aiText: "要应用这 3 处修改吗？", time: "刚刚" },
    ],
    s2: {
      eyebrow: "看板 · 通知 · 点击直达",
      title: "该你处理的会话，点一下就到",
      body: "会话按状态分成几个 tab。「待交互」里的会话按等待时长排序，等得最久的在最上面。需要回复或出错时弹一条系统通知，点通知或点卡片，直接切到对应终端。",
      checks: ["四个 tab：全部 / 待交互 / 运行中 / 已归档，各自带数量", "「待交互」内部按等待时长排序，等得最久的排最前", "系统通知会去重，同一件事只弹一次；点击后切到该会话", "连接中切到 Windows Terminal / Terminal / iTerm2 的对应标签页"],
    },
    s2card: { title: "接入账号用量面板", repo: "autopilot", provider: "codex", state: "waiting", pct: 43, aiText: "要应用这 3 处修改吗？(y/n)", time: "刚刚" },
    s3: {
      eyebrow: "安装 · 登录 · 启动 · 会话菜单",
      title: "从装好 Agent 到日常操作，都不用背命令",
      body: "AI CLI 尚未安装时，可直接一键安装并发起登录；准备好后选项目目录和工具即可开始。新建会话、打开项目目录、加星、便签、改名、归档，全在右键菜单或 ⋮ 按钮里。",
      checks: ["一键安装 Claude Code、Codex、Kimi、Gemini CLI 或 OpenCode，并直接发起登录", "自动接入所需 hooks；检测到连接缺失时，一键修复", "选目录、选工具，点一下新建会话；断开的会话一键续接", "一键打开项目目录，改名与 /rename 同步", "加星置顶、写只存本地的便签、归档收起，都在同一个菜单"],
    },
    s3card: { title: "重构吸边状态机", repo: "meowo", provider: "claude", state: "running", pct: 62, aiText: "把状态机拆成 3 个纯函数，正在补吸附边界单测。", time: "刚刚", note: "记得先确认 API key", starred: true },
    acctHead: { eyebrow: "账号", title: "官方多账号一键切，也支持 API 中转", lead: "同一个工具保存多个官方账号，随时切换；没有官方账号时按模型接入 API 中转。两种接入方式互斥，配置中转期间仍走官方账号。" },
    acct1: { title: "官方多账号", body: "每个账号有独立的登录凭据与会话历史，互不影响。切换后配额读数、登录状态立刻跟着走。", rows: [{ av: "工", bg: "#d97757", name: "Claude · 工作", badge: "使用中", cls: "on" }, { av: "个", bg: "#5b8db8", name: "Claude · 个人", badge: "切换到此账号", cls: "off" }] },
    acct2: { title: "API 中转", body: "为模型填入中转地址、模型名与密钥即可启用；可从推荐项选择，也能填中转商提供的任意模型 ID。", rows: [{ av: "↳", bg: "#7a5bb8", name: "Opus · 7 天", badge: "中转", cls: "relay" }, { av: "官", bg: "#0f9e78", name: "Sonnet · 官方账号", badge: "官方", cls: "on" }] },
    proxyHead: { eyebrow: "网络与代理", title: "每个 AI 工具，走适合自己的网络路径", lead: "一份默认规则覆盖日常使用，需要时再按 AI 工具单独设置。Meowo 发起的用量查询、CLI 安装和新会话会复用对应配置。" },
    proxy: [
      { title: "全局默认", body: "选择直连、跟随系统环境变量或自定义代理，未单独设置的 AI 工具自动跟随。" },
      { title: "按工具覆盖", body: "不同 AI 工具可以使用不同代理，也可以让其中一部分保持直连，互不影响。" },
      { title: "常见格式直接填写", body: "支持 HTTP、SOCKS5 及带认证的代理，包括 host:port:user:pass。工具不支持某种协议时会明确提示。" },
    ],
    themeExtra: "另外还能调整不透明度（配合系统毛玻璃透出桌面）与界面密度。",
    platHead: { eyebrow: "平台", title: "Windows 和 macOS 的形态不同" },
    win: { title: "Windows · 桌面贴纸", checks: ["拖到屏幕左 / 右 / 顶边会缩成一条红绿灯，鼠标悬停展开", "可以置顶；重启后沿用上次的窗口位置和吸附边", "鼠标停在托盘图标上，能看到待交互和运行中的会话数"] },
    mac: { title: "macOS · 菜单栏面板", checks: ["左键点图标弹出原生面板，失焦自动收起，不占 Dock", "菜单栏图标上显示运行中和待交互的会话数", "universal 包，已签名公证，双击打开"] },
  },
  en: {
    head: { eyebrow: "Features", title: "One workbench, the whole flow", lead: "From installing and signing into AI coding agents to checking status, handling alerts, resuming sessions, and switching accounts — the whole flow lives in one desktop workbench." },
    showcaseCards: [
      { title: "Refactor edge-snap state machine", repo: "meowo", provider: "claude", state: "running", pct: 62, aiText: "Split the state machine into 3 pure functions; adding boundary tests.", time: "just now", model: "claude-opus-4" },
      { title: "Wire up the usage panel", repo: "autopilot", provider: "codex", state: "waiting", pct: 43, aiText: "Apply these 3 changes? (y/n)", time: "just now" },
      { title: "Bump tauri to 2.3", repo: "cc-relay", provider: "kimi", state: "idle", aiText: "Updated Cargo.toml; a few breaking changes to confirm.", time: "12 min ago" },
      { title: "Fix statusline compatibility", repo: "clawmo-ios", provider: "claude", state: "stopped", aiText: "Compatibility fixed and merged. Done.", time: "3 hr ago" },
    ],
    showcaseCap: "Session cards, status tabs, and quota readouts in the bottom bar",
    overview: { eyebrow: "Overview", title: "Feature list" },
    s1: {
      eyebrow: "Window form",
      title: "A sticker expanded, a traffic light collapsed",
      body: "When you need detail it's a sticker pinned to a corner of your desktop; drag it to a screen edge to collapse into a vertical electronic traffic light. Red, amber, green tell each session's state at a glance.",
      checks: ["Drag to the left / right / top edge and the window shrinks into a status strip; hover to expand", "Red = error, amber = needs you, green = running — nothing slips by even collapsed", "Pin it on top; it remembers its last position and snap edge across restarts", "On macOS it's a menu-bar panel; the icon shows running and needs-you counts"],
    },
    s1cards: [
      { title: "Refactor edge-snap state machine", repo: "meowo", provider: "claude", state: "running", pct: 62, aiText: "Split the state machine into 3 pure functions…", time: "just now" },
      { title: "Wire up the usage panel", repo: "autopilot", provider: "codex", state: "waiting", pct: 43, aiText: "Apply these 3 changes?", time: "just now" },
    ],
    s2: {
      eyebrow: "Board · Notifications · Click to jump",
      title: "The session that needs you, one click away",
      body: "Sessions split into tabs by status. In “Needs you”, they sort by wait time, the longest-waiting on top. On a reply or error it can fire a system notification; click the notification or the card to switch to the terminal.",
      checks: ["Four tabs: All / Needs you / Running / Archived, each with a count", "Inside “Needs you”, sorted by wait time — the longest-waiting first", "Notifications dedupe: the same event pings once; clicking switches to that session", "Connected sessions switch to the matching Windows Terminal / Terminal / iTerm2 tab"],
    },
    s2card: { title: "Wire up the usage panel", repo: "autopilot", provider: "codex", state: "waiting", pct: 43, aiText: "Apply these 3 changes? (y/n)", time: "just now" },
    s3: {
      eyebrow: "Install · Sign in · Launch · Session menu",
      title: "From installing an agent to daily use, no commands to memorize",
      body: "If an AI CLI isn't installed yet, install it and start the login in one click; once ready, pick a project directory and tool to begin. New session, open directory, star, note, rename, archive — all in the right-click or ⋮ menu.",
      checks: ["One-click install Claude Code, Codex, Kimi, Gemini CLI, or OpenCode and start the login", "Required hooks are wired up automatically; a missing connection is fixed in one click", "Pick a directory and tool, click to start a new session; resume disconnected ones in one click", "Open the project directory in one click; rename stays in sync with /rename", "Star to pin, jot a local-only note, archive — all in the same menu"],
    },
    s3card: { title: "Refactor edge-snap state machine", repo: "meowo", provider: "claude", state: "running", pct: 62, aiText: "Split the state machine into 3 pure functions; adding boundary tests.", time: "just now", note: "Confirm the API key first", starred: true },
    acctHead: { eyebrow: "Accounts", title: "Switch official accounts in a click, or use an API relay", lead: "Keep multiple official accounts per tool and switch anytime; without one, connect an API relay per model. The two are mutually exclusive — you keep using the official account while configuring the relay." },
    acct1: { title: "Official accounts", body: "Each account has its own login credentials and session history. After switching, quota readouts and login state follow immediately.", rows: [{ av: "W", bg: "#d97757", name: "Claude · Work", badge: "In use", cls: "on" }, { av: "P", bg: "#5b8db8", name: "Claude · Personal", badge: "Switch to this", cls: "off" }] },
    acct2: { title: "API relay", body: "Fill in the relay address, model name, and key to enable it; pick from suggestions or enter any model ID your relay provides.", rows: [{ av: "↳", bg: "#7a5bb8", name: "Opus · 7-day", badge: "Relay", cls: "relay" }, { av: "O", bg: "#0f9e78", name: "Sonnet · Official", badge: "Official", cls: "on" }] },
    proxyHead: { eyebrow: "Network & proxy", title: "Each AI tool takes the network path that suits it", lead: "One default rule covers everyday use; set per-tool when needed. Meowo's usage queries, CLI installs, and new sessions reuse the matching config." },
    proxy: [
      { title: "Global default", body: "Choose direct, follow system env vars, or a custom proxy; tools without their own setting follow it." },
      { title: "Per-tool override", body: "Different AI tools can use different proxies, or some can stay direct — independently." },
      { title: "Common formats accepted", body: "Supports HTTP, SOCKS5, and authenticated proxies including host:port:user:pass. If a tool doesn't support a protocol, it says so clearly." },
    ],
    themeExtra: "You can also tune opacity (to let the frosted desktop show through) and UI density.",
    platHead: { eyebrow: "Platform", title: "Different forms on Windows and macOS" },
    win: { title: "Windows · Desktop sticker", checks: ["Drag to the left / right / top edge to shrink into a traffic light; hover to expand", "Pin on top; remembers last position and snap edge across restarts", "Hover the tray icon to see needs-you and running counts"] },
    mac: { title: "macOS · Menu-bar panel", checks: ["Left-click the icon for a native panel that auto-hides on blur, no Dock space", "The menu-bar icon shows running and needs-you counts", "Universal build, signed & notarized, double-click to open"] },
  },
};

export default function FeaturesContent({ lang }: { lang: Lang }) {
  const c = CONTENT[lang];
  const t = getDict(lang).theme;
  return (
    <main>
      <section className="pagehead">
        <div className="container">
          <span className="eyebrow">{c.head.eyebrow}</span>
          <h1 className="h1">{c.head.title}</h1>
          <p className="lead">{c.head.lead}</p>
        </div>
      </section>

      <section className="section-sm">
        <div className="container" style={{ display: "flex", justifyContent: "center" }}>
          <StickerWindow lang={lang} activeTab="all" cards={c.showcaseCards} />
        </div>
        <p className="showcase-cap">{c.showcaseCap}</p>
      </section>

      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.overview.eyebrow}</span>
            <h2 className="h1">{c.overview.title}</h2>
          </div>
          <FeatureGrid lang={lang} />
        </div>
      </section>

      {/* 双形态 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="split">
            <div className="split-text">
              <span className="eyebrow">{c.s1.eyebrow}</span>
              <h2 className="h2">{c.s1.title}</h2>
              <p className="lead" style={{ fontSize: 17, marginTop: 14 }}>{c.s1.body}</p>
              <ul className="checklist">
                {c.s1.checks.map((x) => <Check key={x}>{x}</Check>)}
              </ul>
            </div>
            <div className="scene-stage stage-dark forms-stage" style={{ minHeight: 340 }}>
              <StickerWindow lang={lang} activeTab="all" cards={c.s1cards} />
              <CollapsedStrip edge="right" className="forms-edge-strip" />
            </div>
          </div>
        </div>
      </section>

      {/* 看板 & 通知 & 点击直达 */}
      <section className="section">
        <div className="container">
          <div className="split">
            <div className="split-text">
              <span className="eyebrow">{c.s2.eyebrow}</span>
              <h2 className="h2">{c.s2.title}</h2>
              <p className="lead" style={{ fontSize: 17, marginTop: 14 }}>{c.s2.body}</p>
              <ul className="checklist">
                {c.s2.checks.map((x) => <Check key={x}>{x}</Check>)}
              </ul>
            </div>
            <div className="split-media" style={{ padding: 22 }}>
              <StickerWindow lang={lang} activeTab="waiting" cards={[c.s2card]} />
            </div>
          </div>
        </div>
      </section>

      {/* 安装/登录/启动/会话菜单 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="split rev">
            <div className="split-media" style={{ padding: 22, overflow: "visible" }}>
              <StickerWindow lang={lang} activeTab="all" cards={[c.s3card]} showNote showMenu />
            </div>
            <div className="split-text">
              <span className="eyebrow">{c.s3.eyebrow}</span>
              <h2 className="h2">{c.s3.title}</h2>
              <p className="lead" style={{ fontSize: 17, marginTop: 14 }}>{c.s3.body}</p>
              <ul className="checklist">
                {c.s3.checks.map((x) => <Check key={x}>{x}</Check>)}
              </ul>
            </div>
          </div>
        </div>
      </section>

      {/* 账号 */}
      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.acctHead.eyebrow}</span>
            <h2 className="h1">{c.acctHead.title}</h2>
            <p className="lead">{c.acctHead.lead}</p>
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

      {/* 代理 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.proxyHead.eyebrow}</span>
            <h2 className="h1">{c.proxyHead.title}</h2>
            <p className="lead">{c.proxyHead.lead}</p>
          </div>
          <div className="grid grid-3">
            {c.proxy.map((p) => (
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

      {/* 主题与配色 */}
      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{t.eyebrowFeat}</span>
            <h2 className="h1">{t.headingFeat}</h2>
            <p className="lead">{t.subFeat}</p>
          </div>
          <Reveal>
            <ThemeShowcase lang={lang} />
          </Reveal>
          <p className="faint" style={{ textAlign: "center", marginTop: 22, fontSize: 13.5 }}>{c.themeExtra}</p>
        </div>
      </section>

      {/* 平台差异 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.platHead.eyebrow}</span>
            <h2 className="h1">{c.platHead.title}</h2>
          </div>
          <div className="grid grid-2">
            {[c.win, c.mac].map((pl) => (
              <Reveal key={pl.title}>
                <div className="fcard">
                  <h3 style={{ fontSize: 19 }}>{pl.title}</h3>
                  <ul className="checklist" style={{ marginTop: 16 }}>
                    {pl.checks.map((x) => <Check key={x}>{x}</Check>)}
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
