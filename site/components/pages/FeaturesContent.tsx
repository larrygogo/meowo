import FeatureGrid from "@/components/FeatureGrid";
import Reveal from "@/components/Reveal";
import CtaBand from "@/components/CtaBand";
import AccountCard, { type AcctRow } from "@/components/AccountCard";
import CheckItem from "@/components/CheckItem";
import { MenuButtonsMock } from "@/components/screenshots";
import DemoFrame from "@/components/DemoFrame";
import AgentMatrix from "@/components/AgentMatrix";
import { type Lang } from "@/lib/i18n";

type Sec = { eyebrow: string; title: string; body: string; checks: string[] };

type Content = {
  head: { eyebrow: string; title: string; lead: string };
  overview: { eyebrow: string; title: string };
  matrixHead: { eyebrow: string; title: string; lead: string };
  s4: Sec;
  panel: Sec;
  mobile: Sec;
  s5: Sec;
  moreHead: { eyebrow: string; title: string };
  more: { title: string; body: string }[];
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
    head: { eyebrow: "功能", title: "一个工作台，管理完整流程", lead: "从安装、登录 AI 编程代理，到开会话、批审批、看改动、切账号，再到离开电脑用手机接着推进，整个流程都在 Meowo 里完成。" },
    overview: { eyebrow: "概览", title: "功能一览" },
    matrixHead: { eyebrow: "五家 Agent", title: "各家能做什么，摆在一张表里", lead: "Meowo 支持的五家 CLI 能力不完全一样：有的能在对话窗里直接答题，有的读得到额度，有的能存多个账号。下面逐项对照。" },
    s5: {
      eyebrow: "菜单按钮化",
      title: "终端弹的菜单，在这里变成按钮",
      body: "CLI 在终端里弹出的交互菜单，比如信任确认、长会话恢复、模型选择，Meowo 实时识别出来，渲染成可以直接点的按钮。不用再记「上一项、下一项、回车」那套键盘操作。",
      checks: ["信任文件夹、恢复长会话等启动确认，一点即选", "按菜单光标位置换算按键序列发回，菜单首尾循环也不会选错", "命令审批卡附命令全文，允许 / 拒绝一键即发", "发出 /model 之类命令后，弹出的菜单同样自动转成按钮"],
    },
    moreHead: { eyebrow: "更多细节", title: "顺手的地方" },
    more: [
      { title: "运行时防止休眠", body: "有会话在跑时电脑不睡，屏幕照常熄灭；没会话在跑就交还系统。" },
      { title: "提醒", body: "会话需要你回复或出错时弹系统通知，任务栏图标也会高亮。灵动岛弹哪几种提醒、停留几秒都能设。" },
      { title: "外观", body: "深色、浅色或跟随系统；界面缩放、对话内容宽度可调；消息自动折叠只留最终回答。" },
      { title: "轻量模式", body: "关掉对话窗口功能，会话都在你自己的终端里跑，Meowo 只负责看状态和提醒。" },
      { title: "会话管理", body: "按工作区、状态、更新时间筛选会话，批量标记完成或恢复。" },
      { title: "诊断日志", body: "出问题时在设置里导出日志，附在反馈里，方便排查。" },
    ],
    s4: {
      eyebrow: "对话窗口",
      title: "会话在这里跑，不用翻终端",
      body: "选好 Agent 和目录，写下第一句话就开聊。消息、工具调用、子任务各就各位，等批准的命令、Agent 的提问、终端弹的菜单，都变成可以直接点的按钮。",
      checks: ["工具调用折叠成一行，展开看完整输入输出；子任务有独立时间线", "审批卡标出这次操作碰到哪里：工作区外的路径、删文件、联网", "Agent 的选择题直接在卡片上作答", "可以贴图片，也能截图后框出位置、写好说明再发", "斜杠命令带补全，模型与权限模式在输入区直接切换", "长对话右侧有消息导航，一键跳回自己问过的某句话", "会话跑在托管终端里，随时切到终端视图亲手操作"],
    },
    panel: {
      eyebrow: "右侧面板",
      title: "更改、文件、终端、浏览器，就在对话旁边",
      body: "对话窗右侧是一排页签，可以拖动排序、右键批量关闭。每个会话记住自己开了哪些页签；会话结束或归档后，它开的终端和浏览器一起关掉。",
      checks: ["「更改」页签看 diff，左侧有新旧两列行号；对某一行写评论，一键放进输入框发给 Agent", "文件页浏览项目、预览文件；文件在后台被改了会提示刷新，菜单里能把路径添加到对话", "在会话目录里开终端，可以开多个；关掉还有程序在跑的终端时先问一句", "浏览器有地址栏、前进后退、缩放；对话里的 localhost 链接直接在面板里打开", "claude、codex、kimi 会话能自己操作这个浏览器：打开网页、点击、输入、读内容、截图。你看得到它的鼠标，随时点「接管」，设置里也能关掉", "快捷键：Ctrl+Shift+G 改动，Ctrl+Shift+E 文件，Ctrl+Shift+B 浏览器"],
    },
    mobile: {
      eyebrow: "手机远程",
      title: "离开电脑，会话照样往前走",
      body: "在设置里开启手机远程，手机扫码就能打开 Meowo 的手机页，不用装 App。看对话、发消息、批审批，和在电脑前一样。",
      checks: ["命令审批和 Agent 的提问，在手机上直接处理", "新建、分叉会话，输入框打 @ 引用文件", "看托管终端的画面和这次的改动", "会话需要你时，经 Bark 推送到手机", "手机上能开关电脑的「运行时防止休眠」和推送", "能导出对话，能添加到主屏幕当 App 用", "局域网或 Tailscale 下可用；推荐只绑定 Tailscale 网卡"],
    },
    acctHead: { eyebrow: "账号", title: "官方多账号一键切，也支持 API 中转", lead: "同一个 Agent 可以存多个官方账号，随时切换（Gemini CLI 只有一个账号）；没有官方账号时，按模型接入 API 中转。两种方式二选一：启用中转后走中转，官方额度不再适用。" },
    acct1: { title: "官方多账号", body: "每个账号有独立的登录凭据与会话历史，互不影响。切换后额度读数、登录状态立刻跟着走；Claude Code、Codex、Kimi 正在跑的会话，会问你要不要一起切过去。", rows: [{ av: "工", bg: "#d97757", name: "Claude · 工作", badge: "使用中", cls: "on" }, { av: "个", bg: "#5b8db8", name: "Claude · 个人", badge: "切换到此账号", cls: "off" }] },
    acct2: { title: "API 中转", body: "为模型填入中转地址、模型名与密钥即可启用；可从推荐项选择，也能填中转商提供的任意模型 ID。", rows: [{ av: "↳", bg: "#7a5bb8", name: "Opus · 7 天", badge: "中转", cls: "relay" }, { av: "官", bg: "#0f9e78", name: "Sonnet · 官方账号", badge: "官方", cls: "on" }] },
    proxyHead: { eyebrow: "网络与代理", title: "每个 AI 工具，走适合自己的网络路径", lead: "一份默认规则覆盖日常使用，需要时再按 AI 工具单独设置。Meowo 发起的用量查询、CLI 安装和新会话会复用对应配置。" },
    proxy: [
      { title: "全局默认", body: "选择直连、跟随系统环境变量或自定义代理，未单独设置的 AI 工具自动跟随。" },
      { title: "按工具覆盖", body: "不同 AI 工具可以使用不同代理，也可以让其中一部分保持直连，互不影响。" },
      { title: "常见格式直接填写", body: "支持 HTTP 和带认证的代理，包括 host:port:user:pass。SOCKS 只有 Kimi 和 Gemini CLI 认，给其他几家填了会当场提示。" },
    ],
    platHead: { eyebrow: "平台", title: "两个平台都是灵动岛，落点不同" },
    win: { title: "Windows · 灵动岛", body: "停在屏幕顶部的一条胶囊，谁在跑、谁在等你一眼可见。", checks: ["点一下展开面板，看会话和各家额度", "会话需要你时弹出提醒，点头像直接打开会话", "极简模式只留一条细线，鼠标移上去变回胶囊"] },
    mac: { title: "macOS · 灵动岛", body: "有刘海的 Mac 上包住刘海，没刘海时收进菜单栏。", checks: ["有刘海时，额度和会话头像分在刘海两侧", "没刘海时，点菜单栏图标在下方弹出面板", "菜单栏上一双会动的眼睛，旁边显示最要紧的会话"] },
  },
  en: {
    head: { eyebrow: "Features", title: "One workbench, the whole flow", lead: "From installing and signing into AI coding agents to starting sessions, approving commands, reviewing changes and switching accounts, then keeping things moving from your phone, the whole flow lives in Meowo." },
    overview: { eyebrow: "Overview", title: "Feature list" },
    matrixHead: { eyebrow: "Five agents", title: "What each agent can do, in one table", lead: "The five CLIs Meowo supports don't all do the same things: some can answer questions right in the chat window, some report quota, some keep several accounts. Here they are side by side." },
    s5: {
      eyebrow: "Menus as buttons",
      title: "Terminal menus, turned into buttons",
      body: "Interactive menus the CLI pops up in the terminal, such as trust prompts, long-session resume and model pickers, are recognized by Meowo and rendered as buttons you can click. No more “up, down, Enter” keyboard choreography.",
      checks: ["Startup prompts like trusting a folder or resuming a long session are one click away", "Clicks are translated into key sequences from the menu cursor, so wrap-around menus can't misfire", "Approval cards show the full command; allow or deny in one click", "Menus that pop after commands like /model turn into buttons just the same"],
    },
    moreHead: { eyebrow: "More", title: "Small things that help" },
    more: [
      { title: "Keep awake while running", body: "The computer won't sleep while sessions run, though the screen still turns off. With nothing running, the system takes over again." },
      { title: "Alerts", body: "System notifications when a session needs a reply or errors out, and the taskbar icon lights up. Choose which alerts the island shows and how long they stay." },
      { title: "Appearance", body: "Dark, light or follow the system; adjust UI scale and chat content width; Auto-fold messages keeps just the final answer." },
      { title: "Lite mode", body: "Turn off the chat window and run every session in your own terminal; Meowo just tracks status and alerts you." },
      { title: "Session manager", body: "Filter sessions by workspace, status and last update, and mark them done or restore them in bulk." },
      { title: "Diagnostic log", body: "Export the log from Settings when something goes wrong and attach it to your report." },
    ],
    s4: {
      eyebrow: "Chat window",
      title: "Sessions run here, no terminal scrolling",
      body: "Pick an agent and a folder, then type your first message. Messages, tool calls and subagents each have their place, and pending commands, the agent's questions and terminal menus all become buttons you can click.",
      checks: ["Tool calls fold into one line; expand for full input and output. Subagents get their own timeline", "Approval cards show what the action touches: paths outside the workspace, file deletion, network", "Answer the agent's multiple-choice questions right on the card", "Paste images, or take a screenshot, mark it up and add a note before sending", "Slash commands come with completion; switch model and permission mode in the composer", "Long conversations get a message navigator on the right to jump back to anything you asked", "Sessions run in a managed terminal; switch to the terminal view and type yourself anytime"],
    },
    panel: {
      eyebrow: "Right panel",
      title: "Changes, files, terminal and browser, right next to the chat",
      body: "A row of tabs sits on the right of the chat window; drag to reorder, right-click to close in bulk. Each session remembers its tabs, and the terminals and browsers it opened close when the session ends or is archived.",
      checks: ["The Changes tab shows the diff with old and new line numbers. Comment on a line and drop it into the composer for the agent", "The Files tab browses and previews the project, flags files changed in the background, and adds a path to the chat from the menu", "Open terminals in the session folder, as many as you like; closing one with a program still running asks first", "The browser has an address bar, back and forward, and zoom; localhost links in the chat open right in the panel", "claude, codex and kimi sessions can drive this browser: open pages, click, type, read content, take screenshots. You see the cursor, can take over anytime, and can turn it off in Settings", "Shortcuts: Ctrl+Shift+G Changes, Ctrl+Shift+E Files, Ctrl+Shift+B Browser"],
    },
    mobile: {
      eyebrow: "Phone access",
      title: "Step away, and your sessions keep moving",
      body: "Turn on phone access in Settings and scan the QR code to open Meowo's phone page. No app to install. Read the conversation, send messages and approve commands just like at your desk.",
      checks: ["Handle command approvals and the agent's questions right on your phone", "Start and fork sessions; type @ in the composer to reference a file", "See the managed terminal and this session's changes", "Get a push on your phone through Bark when a session needs you", "Toggle the computer's keep-awake and push settings from your phone", "Export a conversation, or add the page to your home screen as an app", "Works over your LAN or Tailscale; binding to the Tailscale interface only is recommended"],
    },
    acctHead: { eyebrow: "Accounts", title: "Switch official accounts in a click, or use an API relay", lead: "Keep several official accounts per agent and switch anytime (Gemini CLI has just one); without one, connect an API relay per model. It's one or the other: once the relay is on, requests go through it and the official quota no longer applies." },
    acct1: { title: "Official accounts", body: "Each account has its own login credentials and session history. After switching, quota readouts and login state follow immediately; for Claude Code, Codex and Kimi, Meowo asks whether running sessions should switch too.", rows: [{ av: "W", bg: "#d97757", name: "Claude · Work", badge: "In use", cls: "on" }, { av: "P", bg: "#5b8db8", name: "Claude · Personal", badge: "Switch to this", cls: "off" }] },
    acct2: { title: "API relay", body: "Fill in the relay address, model name, and key to enable it; pick from suggestions or enter any model ID your relay provides.", rows: [{ av: "↳", bg: "#7a5bb8", name: "Opus · 7-day", badge: "Relay", cls: "relay" }, { av: "O", bg: "#0f9e78", name: "Sonnet · Official", badge: "Official", cls: "on" }] },
    proxyHead: { eyebrow: "Network & proxy", title: "Each AI tool takes the network path that suits it", lead: "One default rule covers everyday use; set per-tool when needed. Meowo's usage queries, CLI installs, and new sessions reuse the matching config." },
    proxy: [
      { title: "Global default", body: "Choose direct, follow system env vars, or a custom proxy; tools without their own setting follow it." },
      { title: "Per-tool override", body: "Different AI tools can use different proxies, or some can stay direct, each set independently." },
      { title: "Common formats accepted", body: "Supports HTTP and authenticated proxies, including host:port:user:pass. Only Kimi and Gemini CLI accept SOCKS; enter one for the others and Meowo tells you right away." },
    ],
    platHead: { eyebrow: "Platform", title: "An island on both platforms, placed to fit each" },
    win: { title: "Windows · Island", body: "A slim pill at the top of the screen that shows who's running and who's waiting on you.", checks: ["Click it to expand a panel with sessions and quota", "Pops an alert when a session needs you; click an icon to open it", "Minimal mode leaves just a thin strip; hover to bring the pill back"] },
    mac: { title: "macOS · Island", body: "Wraps around the notch on Macs that have one; tucks into the menu bar on those that don't.", checks: ["With a notch, quota and session icons sit on either side of it", "Without one, click the menu-bar icon for a panel right below it", "A pair of animated eyes in the menu bar, next to your most urgent session"] },
  },
};

export default function FeaturesContent({ lang }: { lang: Lang }) {
  const c = CONTENT[lang];
  return (
    <main>
      <section className="pagehead">
        <div className="container">
          <span className="eyebrow">{c.head.eyebrow}</span>
          <h1 className="h1">{c.head.title}</h1>
          <p className="lead">{c.head.lead}</p>
        </div>
      </section>

      {/* 五家 Agent 能力对照（首页「看能力对照」跳到这里） */}
      <section className="section-sm" id="agents">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.matrixHead.eyebrow}</span>
            <h2 className="h1">{c.matrixHead.title}</h2>
            <p className="lead">{c.matrixHead.lead}</p>
          </div>
          <AgentMatrix lang={lang} />
        </div>
      </section>

      {/* 灵动岛演示：iframe 里是应用真实的灵动岛组件 + 假数据（app/src/demo/IslandStage.tsx） */}
      <section className="section-sm">
        <div className="container">
          <Reveal>
            <div className="stage">
              <DemoFrame lang={lang} />
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
              {/* 对话窗口演示：iframe 里是应用真实的 ChatWindow + 假会话（app/src/demo/chatEmbed.tsx） */}
              <DemoFrame lang={lang} page="chat" />
            </div>
          </div>
        </div>
      </section>

      {/* 右侧面板：演示 1180 宽，整宽大图 + 要点排在图下 */}
      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.panel.eyebrow}</span>
            <h2 className="h1">{c.panel.title}</h2>
            <p className="lead">{c.panel.body}</p>
          </div>
          <Reveal>
            <div className="stage wide-demo">
              {/* iframe 里是真实 ChatWindow，右侧面板开着更改页签（app/src/demo/panelEmbed.tsx） */}
              <DemoFrame lang={lang} page="panel" />
            </div>
          </Reveal>
          <ul className="checklist wide-checks">
            {c.panel.checks.map((x) => <CheckItem key={x}>{x}</CheckItem>)}
          </ul>
        </div>
      </section>

      {/* 手机远程 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="split rev">
            <div className="split-media">
              {/* iframe 里是手机远程页的真实布局（app/src/demo/mobileEmbed.tsx） */}
              <div className="phone-shell">
                <DemoFrame lang={lang} page="mobile" />
              </div>
            </div>
            <div className="split-text">
              <span className="eyebrow">{c.mobile.eyebrow}</span>
              <h2 className="h2">{c.mobile.title}</h2>
              <p className="lead">{c.mobile.body}</p>
              <ul className="checklist">
                {c.mobile.checks.map((x) => <CheckItem key={x}>{x}</CheckItem>)}
              </ul>
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
      <section className="section section-sunken">
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
      <section className="section">
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

      {/* 更多细节 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">{c.moreHead.eyebrow}</span>
            <h2 className="h1">{c.moreHead.title}</h2>
          </div>
          <div className="grid grid-3">
            {c.more.map((m) => (
              <Reveal key={m.title}>
                <div className="fcard">
                  <h3>{m.title}</h3>
                  <p>{m.body}</p>
                </div>
              </Reveal>
            ))}
          </div>
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
