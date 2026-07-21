import FeatureGrid from "@/components/FeatureGrid";
import Reveal from "@/components/Reveal";
import CtaBand from "@/components/CtaBand";
import AccountCard, { type AcctRow } from "@/components/AccountCard";
import CheckItem from "@/components/CheckItem";
import ThemeShowcase from "@/components/ThemeShowcase";
import { StickerWindow, ChatWindowMock, MenuButtonsMock } from "@/components/screenshots";
import type { CardData } from "@/components/screenshots/StickerWindow";
import { getDict, type Lang } from "@/lib/i18n";

type Sec = { eyebrow: string; title: string; body: string; checks: string[] };

type Content = {
  head: { eyebrow: string; title: string; lead: string };
  showcaseCards: CardData[];
  overview: { eyebrow: string; title: string };
  s4: Sec;
  s5: Sec;
  acctHead: { eyebrow: string; title: string; lead: string };
  acct1: { title: string; body: string; rows: AcctRow[] };
  acct2: { title: string; body: string; rows: AcctRow[] };
  proxyHead: { eyebrow: string; title: string; lead: string };
  proxy: { title: string; body: string }[];
  platHead: { eyebrow: string; title: string };
  win: { title: string; body: string; checks: string[] };
  mac: { title: string; body: string; checks: string[] };
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
    overview: { eyebrow: "概览", title: "功能一览" },
    s5: {
      eyebrow: "菜单按钮化",
      title: "终端弹的菜单，在这里变成按钮",
      body: "CLI 在终端里弹出的交互菜单——信任确认、长会话恢复、模型选择——Meowo 实时识别出来，渲染成可以直接点的按钮。不用再记「上一项、下一项、回车」那套键盘操作。",
      checks: ["信任文件夹、恢复长会话等启动确认，一点即选", "按菜单光标位置换算按键序列发回，菜单首尾循环也不会选错", "命令审批卡附命令全文，允许 / 拒绝一键即发", "发出 /model 之类命令后，弹出的菜单同样自动转成按钮"],
    },
    s4: {
      eyebrow: "对话窗口",
      title: "打开卡片，是完整的结构化对话",
      body: "点开任意一张会话卡片，就进入这个会话的对话窗口：消息、工具调用、子任务各就各位，不用在终端滚屏里翻找。等批准的命令、弹出的选择菜单，都变成可以直接点的按钮。",
      checks: ["工具调用折叠成一行摘要，展开看完整输入输出；子任务有独立时间线", "Agent 拆解任务后，待办清单实时显示在对话里，进度跟着刷新", "信任确认、长会话恢复、模型选择等终端菜单，直接渲染成按钮", "命令审批卡在输入区上方，批准 / 拒绝一点即发", "斜杠命令带补全，模型与协作模式在输入区直接切换", "对话与终端双视图并排，随时切到底层终端亲自操作"],
    },
    acctHead: { eyebrow: "账号", title: "官方多账号一键切，也支持 API 中转", lead: "同一个工具保存多个官方账号，随时切换；没有官方账号时按模型接入 API 中转。两种接入方式互斥，配置中转期间仍走官方账号。" },
    acct1: { title: "官方多账号", body: "每个账号有独立的登录凭据与会话历史，互不影响。切换后配额读数、登录状态立刻跟着走。", rows: [{ av: "工", bg: "#d97757", name: "Claude · 工作", badge: "使用中", cls: "on" }, { av: "个", bg: "#5b8db8", name: "Claude · 个人", badge: "切换到此账号", cls: "off" }] },
    acct2: { title: "API 中转", body: "为模型填入中转地址、模型名与密钥即可启用；可从推荐项选择，也能填中转商提供的任意模型 ID。", rows: [{ av: "↳", bg: "#7a5bb8", name: "Opus · 7 天", badge: "中转", cls: "relay" }, { av: "官", bg: "#0f9e78", name: "Sonnet · 官方账号", badge: "官方", cls: "on" }] },
    proxyHead: { eyebrow: "网络与代理", title: "每个 AI 工具，走适合自己的网络路径", lead: "一份默认规则覆盖日常使用，需要时再按 AI 工具单独设置。Meowo 发起的用量查询、CLI 安装和新会话会复用对应配置。" },
    proxy: [
      { title: "全局默认", body: "选择直连、跟随系统环境变量或自定义代理，未单独设置的 AI 工具自动跟随。" },
      { title: "按工具覆盖", body: "不同 AI 工具可以使用不同代理，也可以让其中一部分保持直连，互不影响。" },
      { title: "常见格式直接填写", body: "支持 HTTP、SOCKS5 及带认证的代理，包括 host:port:user:pass。工具不支持某种协议时会明确提示。" },
    ],
    platHead: { eyebrow: "平台", title: "Windows 和 macOS 的形态不同" },
    win: { title: "Windows · 桌面贴纸", body: "钉在桌面一角的常驻贴纸，拖到屏幕边缘就收成一条红绿灯。", checks: ["拖到屏幕左 / 右 / 顶边会缩成一条红绿灯，鼠标悬停展开", "可以置顶；重启后沿用上次的窗口位置和吸附边", "鼠标停在托盘图标上，能看到待交互和运行中的会话数"] },
    mac: { title: "macOS · 菜单栏面板", body: "菜单栏图标弹出的原生面板，失焦自动收起，不占 Dock。", checks: ["左键点图标弹出原生面板，失焦自动收起，不占 Dock", "菜单栏图标上显示运行中和待交互的会话数", "universal 包，已签名公证，双击打开"] },
  },
  en: {
    head: { eyebrow: "Features", title: "One workbench, the whole flow", lead: "From installing and signing into AI coding agents to checking status, handling alerts, resuming sessions, and switching accounts — the whole flow lives in one desktop workbench." },
    showcaseCards: [
      { title: "Refactor edge-snap state machine", repo: "meowo", provider: "claude", state: "running", pct: 62, aiText: "Split the state machine into 3 pure functions; adding boundary tests.", time: "just now", model: "claude-opus-4" },
      { title: "Wire up the usage panel", repo: "autopilot", provider: "codex", state: "waiting", pct: 43, aiText: "Apply these 3 changes? (y/n)", time: "just now" },
      { title: "Bump tauri to 2.3", repo: "cc-relay", provider: "kimi", state: "idle", aiText: "Updated Cargo.toml; a few breaking changes to confirm.", time: "12 min ago" },
      { title: "Fix statusline compatibility", repo: "clawmo-ios", provider: "claude", state: "stopped", aiText: "Compatibility fixed and merged. Done.", time: "3 hr ago" },
    ],
    overview: { eyebrow: "Overview", title: "Feature list" },
    s5: {
      eyebrow: "Menus as buttons",
      title: "Terminal menus, turned into buttons",
      body: "Interactive menus the CLI pops up in the terminal — trust prompts, long-session resume, model pickers — are recognized by Meowo and rendered as buttons you can simply click. No more “up, down, Enter” keyboard choreography.",
      checks: ["Startup prompts like trusting a folder or resuming a long session are one click away", "Clicks are translated into key sequences from the menu cursor — wrap-around menus can't misfire", "Approval cards show the full command; allow or deny in one click", "Menus that pop after commands like /model turn into buttons just the same"],
    },
    s4: {
      eyebrow: "Chat window",
      title: "Open a card for the full structured conversation",
      body: "Open any session card and you're in that session's chat window: messages, tool calls and subagents all in place — no scrolling terminal output. Commands awaiting approval and popup menus become buttons you can just click.",
      checks: ["Tool calls fold into one-line summaries; expand for full I/O — subagents get their own timeline", "When the agent breaks a task down, its todo list shows live in the conversation", "Trust prompts, long-session resume and model pickers render as buttons", "Approval cards sit above the composer — approve or deny in one click", "Slash commands come with completion; model and mode switch right in the composer", "Chat and terminal views side by side — drop into the raw terminal anytime"],
    },
    acctHead: { eyebrow: "Accounts", title: "Switch official accounts in a click, or use an API relay", lead: "Keep multiple official accounts per tool and switch anytime; without one, connect an API relay per model. The two are mutually exclusive — you keep using the official account while configuring the relay." },
    acct1: { title: "Official accounts", body: "Each account has its own login credentials and session history. After switching, quota readouts and login state follow immediately.", rows: [{ av: "W", bg: "#d97757", name: "Claude · Work", badge: "In use", cls: "on" }, { av: "P", bg: "#5b8db8", name: "Claude · Personal", badge: "Switch to this", cls: "off" }] },
    acct2: { title: "API relay", body: "Fill in the relay address, model name, and key to enable it; pick from suggestions or enter any model ID your relay provides.", rows: [{ av: "↳", bg: "#7a5bb8", name: "Opus · 7-day", badge: "Relay", cls: "relay" }, { av: "O", bg: "#0f9e78", name: "Sonnet · Official", badge: "Official", cls: "on" }] },
    proxyHead: { eyebrow: "Network & proxy", title: "Each AI tool takes the network path that suits it", lead: "One default rule covers everyday use; set per-tool when needed. Meowo's usage queries, CLI installs, and new sessions reuse the matching config." },
    proxy: [
      { title: "Global default", body: "Choose direct, follow system env vars, or a custom proxy; tools without their own setting follow it." },
      { title: "Per-tool override", body: "Different AI tools can use different proxies, or some can stay direct — independently." },
      { title: "Common formats accepted", body: "Supports HTTP, SOCKS5, and authenticated proxies including host:port:user:pass. If a tool doesn't support a protocol, it says so clearly." },
    ],
    platHead: { eyebrow: "Platform", title: "Different forms on Windows and macOS" },
    win: { title: "Windows · Desktop sticker", body: "A resident sticker pinned to a desktop corner; drag it to a screen edge and it folds into a traffic light.", checks: ["Drag to the left / right / top edge to shrink into a traffic light; hover to expand", "Pin on top; remembers last position and snap edge across restarts", "Hover the tray icon to see needs-you and running counts"] },
    mac: { title: "macOS · Menu-bar panel", body: "A native panel from the menu-bar icon that auto-hides on blur — no Dock space taken.", checks: ["Left-click the icon for a native panel that auto-hides on blur, no Dock space", "The menu-bar icon shows running and needs-you counts", "Universal build, signed & notarized, double-click to open"] },
  },
};

export default function FeaturesContent({ lang }: { lang: Lang }) {
  const c = CONTENT[lang];
  const d = getDict(lang);
  return (
    <main>
      <section className="pagehead">
        <div className="container">
          <span className="eyebrow">{c.head.eyebrow}</span>
          <h1 className="h1">{c.head.title}</h1>
          <p className="lead">{c.head.lead}</p>
        </div>
      </section>

      {/* 贴纸 mock 展示 */}
      <section className="section-sm">
        <div className="container">
          <Reveal>
            <div className="stage">
              <StickerWindow lang={lang} activeTab="all" cards={c.showcaseCards} />
            </div>
          </Reveal>
        </div>
      </section>

      {/* 特性网格 */}
      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.overview.eyebrow}</span>
            <h2 className="h1">{c.overview.title}</h2>
          </div>
          <FeatureGrid lang={lang} />
        </div>
      </section>

      {/* 对话窗口 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="split">
            <div className="split-text">
              <span className="eyebrow">{c.s4.eyebrow}</span>
              <h2 className="h2">{c.s4.title}</h2>
              <p className="lead">{c.s4.body}</p>
              <ul className="checklist">
                {c.s4.checks.map((x) => <CheckItem key={x}>{x}</CheckItem>)}
              </ul>
            </div>
            <div className="split-media">
              <ChatWindowMock lang={lang} />
            </div>
          </div>
        </div>
      </section>

      {/* 菜单按钮化 */}
      <section className="section">
        <div className="container">
          <div className="split rev">
            <div className="split-media">
              <MenuButtonsMock lang={lang} />
            </div>
            <div className="split-text">
              <span className="eyebrow">{c.s5.eyebrow}</span>
              <h2 className="h2">{c.s5.title}</h2>
              <p className="lead">{c.s5.body}</p>
              <ul className="checklist">
                {c.s5.checks.map((x) => <CheckItem key={x}>{x}</CheckItem>)}
              </ul>
            </div>
          </div>
        </div>
      </section>

      {/* 账号 / 中转 */}
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
                <AccountCard title={card.title} body={card.body} rows={card.rows} />
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

      {/* 主题互动块 */}
      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{d.theme.eyebrowFeat}</span>
            <h2 className="h1">{d.theme.headingFeat}</h2>
            <p className="lead">{d.theme.subFeat}</p>
          </div>
          <ThemeShowcase lang={lang} />
          <p className="note-center">{d.theme.extra}</p>
        </div>
      </section>

      {/* 平台差异 */}
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
                    {pl.checks.map((x) => <CheckItem key={x}>{x}</CheckItem>)}
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
