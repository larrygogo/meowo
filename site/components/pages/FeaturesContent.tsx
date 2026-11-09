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
    head: { eyebrow: "功能", title: "功能", lead: "这一页按用途列出 Meowo 0.8.1 的功能，以及五家 Agent 各自支持哪些。" },
    overview: { eyebrow: "概览", title: "主要功能" },
    matrixHead: { eyebrow: "五家 Agent", title: "各家 Agent 的支持情况", lead: "五家 CLI 能用的功能不完全一样，比如能不能在对话窗里回答提问、能不能读额度、能不能保存多个账号。下表逐项列出。" },
    s5: {
      eyebrow: "终端菜单",
      title: "终端菜单显示成按钮",
      body: "CLI 在终端里弹出的交互菜单，比如信任目录、恢复长会话、选择模型，Meowo 会识别出来，在对话窗里显示成按钮。点按钮等于在终端里按了对应的键。",
      checks: ["启动时的信任目录、恢复长会话等确认，点按钮选择", "按菜单当前的光标位置计算要发的按键，菜单首尾循环时也不会选错", "命令审批卡显示完整命令，可以允许或拒绝", "输入 /model 这类命令后弹出的菜单，也会显示成按钮"],
    },
    moreHead: { eyebrow: "其他", title: "其他功能" },
    more: [
      { title: "运行时防止休眠", body: "有会话在运行时电脑不会睡眠，屏幕照常熄灭。没有会话在运行时，交还系统的设置。" },
      { title: "通知", body: "会话需要回复或出错时弹出系统通知，任务栏图标也会高亮。灵动岛弹出哪几类提醒、停留几秒，都可以设置。" },
      { title: "外观", body: "可以选深色、浅色或跟随系统，界面缩放和对话内容宽度可调。开启「消息自动折叠」后，回合结束只显示最终回答。" },
      { title: "轻量模式", body: "关闭对话窗口功能后，会话都在你自己的终端里运行，Meowo 只显示状态和提醒。" },
      { title: "会话管理", body: "按工作区、状态和更新时间筛选会话，可以批量标记完成或恢复。" },
      { title: "诊断日志", body: "设置里可以导出诊断日志，反馈问题时附上。" },
    ],
    s4: {
      eyebrow: "对话窗口",
      title: "对话窗口",
      body: "在对话窗里选 Agent 和目录，输入第一句话就新建会话。对话按消息、工具调用、子任务分开显示，待批准的命令和 Agent 的提问显示成卡片。",
      checks: ["工具调用折叠成一行，展开可以看完整输入和输出；子任务有自己的时间线", "审批卡上写明这次操作会动到什么，比如工作区外的路径、删除文件、联网", "Agent 的选择题可以在卡片上回答", "可以粘贴图片，也可以截图后标出位置、加上说明再发送", "斜杠命令有补全，模型和权限模式在输入框旁切换", "长对话的右侧有消息导航，可以跳回自己之前发的某条消息", "会话运行在托管终端里，可以切到终端视图自己操作"],
    },
    panel: {
      eyebrow: "右侧面板",
      title: "右侧面板",
      body: "对话窗右侧是一排页签：更改、文件、终端、浏览器。页签可以拖动排序，右键可以批量关闭。每个会话记得自己开过哪些页签，会话结束或标记完成后，它打开的终端和浏览器会一起关闭。",
      checks: ["「更改」页签显示 diff，左侧是新旧两列行号；可以对某一行写评论，再放进输入框发给 Agent", "「文件」页签浏览和预览项目文件；文件在后台被改动时会提示刷新；菜单里可以把路径添加到对话", "可以在会话目录里开多个终端；关闭仍有程序在运行的终端时会先确认", "浏览器有地址栏、前进后退和缩放；对话里的 localhost 链接会在面板里打开", "Claude Code、Codex 和 Kimi 的会话可以操作这个浏览器：打开网页、点击、输入、读取内容、截图。操作时能看到它的鼠标，你可以随时点「接管」，也可以在设置里关闭这项功能", "快捷键：Ctrl+Shift+G 打开更改，Ctrl+Shift+E 打开文件，Ctrl+Shift+B 打开浏览器"],
    },
    mobile: {
      eyebrow: "手机远程",
      title: "手机远程",
      body: "在设置里开启手机远程后，用手机扫码，就能在手机浏览器里打开 Meowo 的手机页，不需要安装 App。",
      checks: ["在手机上处理命令审批和 Agent 的提问", "新建会话、分叉会话，在输入框里用 @ 引用文件", "查看托管终端的画面和这次的改动", "会话需要你时，通过 Bark（iPhone App）推送通知", "在手机上开关电脑的「运行时防止休眠」和推送", "导出对话；可以把页面添加到主屏幕", "支持局域网和 Tailscale，推荐只绑定 Tailscale 网卡"],
    },
    acctHead: { eyebrow: "账号", title: "账号与 API 中转", lead: "Claude Code、Codex、Kimi 和 OpenCode 可以保存多个官方账号，Gemini CLI 只能有一个。没有官方账号的，可以给这家 Agent 配 API 中转。官方账号和中转只能二选一，开启中转后，官方额度不再适用。" },
    acct1: { title: "多个官方账号", body: "每个账号有自己的登录凭据和会话历史。切换后，额度读数和登录状态跟着变。切换时，Claude Code、Codex 和 Kimi 会询问正在运行的会话要不要一起切过去。", rows: [{ av: "工", bg: "#d97757", name: "Claude · 工作", badge: "使用中", cls: "on" }, { av: "个", bg: "#5b8db8", name: "Claude · 个人", badge: "切换到此账号", cls: "off" }] },
    acct2: { title: "API 中转", body: "给一家 Agent 填上中转地址、模型名和密钥，就能启用中转。模型名可以从推荐里选，也可以填中转商提供的其他名称。", rows: [{ av: "↳", bg: "#7a5bb8", name: "Kimi Code · 中转", badge: "中转", cls: "relay" }, { av: "官", bg: "#0f9e78", name: "Claude Code · 官方账号", badge: "官方", cls: "on" }] },
    proxyHead: { eyebrow: "网络与代理", title: "代理设置", lead: "可以设置一个默认代理，也可以给每家 Agent 单独设置。Meowo 查询用量、安装 CLI 和启动会话时，都会使用对应的代理。" },
    proxy: [
      { title: "默认代理", body: "可选直连、跟随系统环境变量或自定义代理。没有单独设置的 Agent 使用默认代理。" },
      { title: "按 Agent 设置", body: "每家 Agent 可以用不同的代理，也可以设为直连。" },
      { title: "支持的格式", body: "支持 HTTP 代理和带认证的代理，包括 host:port:user:pass 写法。只有 Kimi 和 Gemini CLI 支持 SOCKS，给其他几家填 SOCKS 代理时会提示不支持。" },
    ],
    platHead: { eyebrow: "平台", title: "Windows 与 macOS 上的灵动岛" },
    win: { title: "Windows", body: "灵动岛是屏幕顶部的一条胶囊，显示哪些会话在运行、哪些在等你。", checks: ["点击展开面板，查看会话和各家额度", "会话需要你时弹出提醒，点头像打开那个会话", "极简模式下只留一条细线，鼠标移上去恢复成胶囊"] },
    mac: { title: "macOS", body: "有刘海的 Mac 上，灵动岛围在刘海周围；没有刘海时，它在菜单栏里。", checks: ["有刘海时，额度和会话头像分别在刘海两侧", "没有刘海时，点菜单栏图标会在下方弹出面板", "菜单栏图标是一双会动的眼睛，旁边显示最需要处理的会话"] },
  },
  en: {
    head: { eyebrow: "Features", title: "Features", lead: "This page lists the features in Meowo 0.8.1 by area, and which of them each of the five agents supports." },
    overview: { eyebrow: "Overview", title: "Main features" },
    matrixHead: { eyebrow: "Five agents", title: "What each agent supports", lead: "The five CLIs don't all support the same features, for example answering questions in the chat window, reading quota or keeping several accounts. The table lists them one by one." },
    s5: {
      eyebrow: "Terminal menus",
      title: "Terminal menus as buttons",
      body: "When a CLI shows an interactive menu in the terminal, such as trusting a folder, resuming a long session or picking a model, Meowo recognizes it and shows it as buttons in the chat window. Clicking a button sends the same keys you would press in the terminal.",
      checks: ["Startup prompts such as trusting a folder or resuming a long session are answered with a button", "The keys to send are worked out from the menu's current cursor position, so menus that wrap around don't pick the wrong item", "Approval cards show the full command, and you can allow or deny it", "Menus that appear after commands like /model also show up as buttons"],
    },
    moreHead: { eyebrow: "Other", title: "Other features" },
    more: [
      { title: "Keep awake while running", body: "While sessions are running, the computer doesn't sleep, and the screen still turns off. When nothing is running, the system's own settings apply again." },
      { title: "Notifications", body: "A system notification appears when a session needs a reply or hits an error, and the taskbar icon is highlighted. You can choose which alerts the island shows and how long they stay." },
      { title: "Appearance", body: "Choose dark, light or follow the system, and adjust UI scale and chat content width. With Auto-fold messages on, finished turns show only the final answer." },
      { title: "Lite mode", body: "With the chat window turned off, sessions run in your own terminal and Meowo only shows status and alerts." },
      { title: "Session manager", body: "Filter sessions by workspace, status and last update, and mark them done or restore them in bulk." },
      { title: "Diagnostic log", body: "You can export a diagnostic log from Settings and attach it when you report a problem." },
    ],
    s4: {
      eyebrow: "Chat window",
      title: "Chat window",
      body: "Pick an agent and a folder in the chat window and type the first message to start a session. The conversation shows messages, tool calls and subagents separately, and commands pending approval and the agent's questions appear as cards.",
      checks: ["Tool calls fold into one line; expand one to see the full input and output. Subagents have their own timeline", "Approval cards state what the action will touch, such as paths outside the workspace, deleted files or network access", "You can answer the agent's multiple-choice questions on the card", "Paste images, or take a screenshot, mark a spot and add a note before sending", "Slash commands have completion, and the model and permission mode can be switched next to the message box", "Long conversations have a message navigator on the right for jumping back to an earlier message of yours", "Sessions run in a managed terminal, and you can switch to the terminal view to type yourself"],
    },
    panel: {
      eyebrow: "Right panel",
      title: "Right panel",
      body: "The right side of the chat window holds a row of tabs: Changes, Files, Terminal and Browser. Drag tabs to reorder them, or right-click to close several at once. Each session remembers which tabs it had open, and the terminals and browsers it opened close when the session ends or is marked done.",
      checks: ["The Changes tab shows the diff with old and new line numbers. You can comment on a line and add the comment to the message for the agent", "The Files tab browses and previews project files, tells you when a file changed in the background, and can add a path to the chat from its menu", "Open several terminals in the session folder. Closing one that still has a program running asks you first", "The browser has an address bar, back and forward, and zoom. localhost links in the chat open in the panel", "Claude Code, Codex and Kimi sessions can operate this browser: open pages, click, type, read content and take screenshots. You can see its cursor while it works, click Take over at any time, or turn the feature off in Settings", "Shortcuts: Ctrl+Shift+G opens Changes, Ctrl+Shift+E opens Files, Ctrl+Shift+B opens Browser"],
    },
    mobile: {
      eyebrow: "Phone access",
      title: "Phone access",
      body: "After you turn on phone access in Settings and scan the QR code, your phone's browser opens Meowo's phone page. No app needs to be installed.",
      checks: ["Handle command approvals and the agent's questions on your phone", "Start or fork sessions, and use @ in the message box to reference a file", "See the managed terminal and this session's changes", "Get a notification through Bark (an iPhone app) when a session needs you", "Turn the computer's keep-awake and push settings on or off from your phone", "Export a conversation, and add the page to your home screen", "Works over your LAN or Tailscale. Binding to the Tailscale interface only is recommended"],
    },
    acctHead: { eyebrow: "Accounts", title: "Accounts and API relay", lead: "Claude Code, Codex, Kimi and OpenCode can keep several official accounts; Gemini CLI can have only one. Without an official account, you can set up an API relay for that agent. It's either the official account or the relay: once the relay is on, the official quota no longer applies." },
    acct1: { title: "Several official accounts", body: "Each account has its own credentials and session history. After you switch, the quota readout and sign-in status change with it. For Claude Code, Codex and Kimi, Meowo asks whether running sessions should switch too.", rows: [{ av: "W", bg: "#d97757", name: "Claude · Work", badge: "In use", cls: "on" }, { av: "P", bg: "#5b8db8", name: "Claude · Personal", badge: "Switch to this", cls: "off" }] },
    acct2: { title: "API relay", body: "Enter a relay address, model name and key for an agent to turn on the relay. Choose a model name from the suggestions or enter another one your relay provides.", rows: [{ av: "↳", bg: "#7a5bb8", name: "Kimi Code · Relay", badge: "Relay", cls: "relay" }, { av: "O", bg: "#0f9e78", name: "Claude Code · Official", badge: "Official", cls: "on" }] },
    proxyHead: { eyebrow: "Network & proxy", title: "Proxy settings", lead: "Set one default proxy, or set one per agent. Meowo uses the matching proxy when it checks usage, installs a CLI and starts sessions." },
    proxy: [
      { title: "Default proxy", body: "Choose direct, follow the system environment variables, or a custom proxy. Agents without their own setting use the default." },
      { title: "Per agent", body: "Each agent can use a different proxy or connect directly." },
      { title: "Supported formats", body: "HTTP proxies and authenticated proxies are supported, including the host:port:user:pass form. Only Kimi and Gemini CLI support SOCKS; if you enter a SOCKS proxy for the others, Meowo tells you it isn't supported." },
    ],
    platHead: { eyebrow: "Platform", title: "The island on Windows and macOS" },
    win: { title: "Windows", body: "The island is a pill at the top of the screen showing which sessions are running and which are waiting for you.", checks: ["Click it to open a panel with sessions and each agent's quota", "When a session needs you, an alert appears; click the icon to open that session", "Minimal mode leaves a thin strip; move the mouse over it to bring the pill back"] },
    mac: { title: "macOS", body: "On a Mac with a notch, the island sits around the notch. Without a notch, it lives in the menu bar.", checks: ["With a notch, quota and session icons sit on either side of it", "Without a notch, clicking the menu-bar icon opens a panel below it", "The menu-bar icon is a pair of animated eyes, next to the session that most needs attention"] },
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
