import type { ReactNode } from "react";
import Link from "next/link";
import { PlusIcon } from "@/components/icons";
import CtaBand from "@/components/CtaBand";
import { withLang, type Lang } from "@/lib/i18n";
import { REPO } from "@/lib/site";

type QA = { q: string; a: ReactNode };

function build(lang: Lang): { head: { eyebrow: string; title: string; lead: string }; qa: QA[]; ctaTitle: string; ctaSub: string } {
  const docs = withLang(lang, "/docs");
  if (lang === "en") {
    return {
      head: { eyebrow: "FAQ", title: "Frequently asked questions", lead: "About supported tools, phone access, privacy, accounts and proxies, and platform support." },
      ctaTitle: "Still have questions?",
      ctaSub: "Open an Issue or Discussion on GitHub.",
      qa: [
        { q: "Which AI coding CLIs does Meowo support?", a: "Claude Code, Codex, Kimi, Gemini CLI and OpenCode are built in. Each reports events into the same local database (OpenCode through a bridge plugin, the others through their own hooks), so they all show up in one place." },
        { q: "How is this different from just using the terminal?", a: "In the terminal you switch windows yourself and remember each tool's commands. In Meowo, sessions run in one chat window: approvals and questions are buttons, the diff is in the side panel, the island tells you when a session needs you, and your phone takes over when you step away. The terminal is still there; switch to it anytime." },
        { q: "Is my session data uploaded to the cloud?", a: (<>No. Data is only written to a local <code className="inline">~/.meowo/board.db</code> (SQLite). Meowo has no server of its own and sends no session content anywhere. The only exception is optional: if you set up Bark push, each push (session title, the agent&apos;s last message, a link back to the session) goes through Bark&apos;s server.</>) },
        { q: "Do I need to configure hooks manually?", a: (<>Usually not. On startup Meowo wires the reporter into the AI coding CLIs it detects, backing up first so your existing config isn&apos;t broken. For troubleshooting or manual setup, see the <Link href={docs}>docs</Link>.</>) },
        { q: "Where do sessions open?", a: "Choose under Settings → Open session in: the chat window, or an external terminal (the default), and which terminal to use (Windows Terminal, WezTerm, PowerShell or Command Prompt on Windows; Terminal, iTerm2 or Ghostty on macOS). Clicking a session on the island always opens the chat window. Either way the session runs in the same terminal hosted by Meowo, and sessions you start in your own terminal show up too." },
        { q: "What can I do from my phone?", a: "Read conversations, send messages, approve commands, answer the agent's questions, start and fork sessions, see the terminal and the changes, and export a conversation. You can also toggle the computer's keep-awake and push settings from the phone." },
        { q: "Does phone access need a public IP? Is it safe?", a: (<>No public IP needed: it works on the same Wi-Fi or over Tailscale. The connection is plain HTTP and protected by an access token, so on networks you don&apos;t trust, bind to Tailscale only (recommended). Regenerating the token unpairs every phone. See the <Link href={`${docs}#remote`}>phone access guide</Link>.</>) },
        { q: "Can the agent use the browser in the side panel? Can I stop it?", a: "claude, codex and kimi sessions get a set of browser tools on start: they can open pages, click, type, read content and take screenshots in the panel browser. You see the cursor while it works and can click Take over to pause it anytime. Turn it off in Settings and running sessions lose access immediately." },
        { q: "Can I see the agent's task progress?", a: "Yes. The progress panel lists the agent's todo list, the subagents it sends out and commands still running in the background, and it refreshes as work goes on. While a background command runs, the session isn't shown as waiting for you." },
        { q: "What proxy formats are supported, and where do they apply?", a: (<>Set a global default rule, or override per agent. Supports HTTP, URLs with username and password, and <code className="inline">host:port:user:pass</code>; only Kimi and Gemini CLI accept SOCKS. The config applies to Meowo&apos;s usage queries, CLI installs and the sessions it launches; protocol support and scope vary by CLI, and the settings page shows what&apos;s actually in effect.</>) },
        { q: "Can I keep multiple accounts for one tool?", a: "Yes, for Claude Code, Codex, Kimi and OpenCode (Gemini CLI has a single account). Add several official accounts in Settings; each has its own credentials and session history. Switch with one click and quota readouts and login state follow. For Claude Code, Codex and Kimi, Meowo also asks whether running sessions should switch along." },
        { q: "No official account? Can I use an API relay?", a: "Yes. Connect an API relay for an agent in account settings: fill in the relay address, model name and key; pick the model from suggestions or enter any ID your relay provides. Official account and API relay are mutually exclusive, and the official quota doesn't apply while the relay is active." },
        { q: "Windows says “Windows protected your PC”. Is that OK?", a: (<>The Windows installer isn&apos;t code-signed yet, so SmartScreen shows this on first run. Choose “More info” → “Run anyway”. Installers are built by GitHub Actions from the public source, and in-app updates are verified with a minisign signature. The macOS builds are signed and notarized.</>) },
        { q: "Can I change how it looks?", a: "Yes. Dark, light or follow the system; adjust UI scale and chat width; turn on Auto-fold messages to keep just the final answer. Changes apply live. The island itself is always black so it blends into the top edge of the screen." },
        { q: "Does it support Linux?", a: "Only Windows and macOS installers are provided for now. Linux packaging is planned but not done yet. Track progress in GitHub Releases." },
        { q: "Does it use a lot of resources?", a: "No. The reporter only starts when a hook fires, writes the database and exits. The Meowo app itself stays resident and refreshes by watching local file changes, at very low cost." },
        { q: "Is it free? Can I use it commercially?", a: (<>Free, open source, MIT-licensed: use and modify it freely. Source on <a href={REPO} target="_blank" rel="noopener noreferrer">GitHub</a>.</>) },
        { q: "How do I uninstall, and what's left behind?", a: (<>Uninstall the app the usual way for your OS. Session data and settings live in <code className="inline">~/.meowo/</code>. To fully clean up, also remove entries whose command contains <code className="inline">meowo-reporter</code> from each AI CLI&apos;s hook config; don&apos;t delete your other custom hooks.</>) },
      ],
    };
  }
  return {
    head: { eyebrow: "FAQ", title: "常见问题", lead: "关于支持范围、手机远程、隐私、账号与代理，以及平台兼容。" },
    ctaTitle: "还有问题？",
    ctaSub: "可以在 GitHub 上开 Issue 或 Discussion。",
    qa: [
      { q: "Meowo 支持哪些 AI 编程 CLI？", a: "内置支持 Claude Code、Codex、Kimi、Gemini CLI 和 OpenCode。它们把事件写进本地同一份数据库（OpenCode 走桥接插件，其余四家走各自的 hook），所以能在一个地方看到。" },
      { q: "这和直接用终端有什么区别？", a: "用终端要自己切窗口、记各家工具的命令。在 Meowo 里，会话都在一个对话窗里跑：审批和提问是按钮，改动在右侧面板里看，会话要你时灵动岛提醒你，离开电脑还能用手机接着推进。终端也还在，随时能切过去。" },
      { q: "我的会话数据会上传到云端吗？", a: (<>不会。数据只写进本地的 <code className="inline">~/.meowo/board.db</code>（SQLite）。Meowo 没有自己的服务器，不把会话内容发到任何地方。唯一的例外是可选的：如果你设置了 Bark 推送，推送内容（会话标题、Agent 最后一句话、打开会话的链接）会经过 Bark 的服务器。</>) },
      { q: "需要手动配置 hooks 吗？", a: (<>一般不需要。Meowo 启动时会为检测到的 AI 编程 CLI 接入 reporter，写之前先备份，不会破坏已有配置。需要排查或手动接入时见<Link href={docs}>文档</Link>。</>) },
      { q: "会话在哪里打开？", a: "在设置「打开会话」里选：对话窗口，或者外部终端（默认），外部终端用哪个也能指定（Windows 上有 Windows Terminal、WezTerm、PowerShell、命令提示符，macOS 上有 Terminal、iTerm2、Ghostty）。从灵动岛点会话头像，打开的总是对话窗。两种方式看的都是 Meowo 托管的同一个终端；你自己在终端里开的会话，也会出现在 Meowo 里。" },
      { q: "手机上能做哪些事？", a: "看对话、发消息、批审批、回答 Agent 的提问，新建和分叉会话，看终端画面和改动，导出对话。电脑的「运行时防止休眠」和推送开关，在手机上也能改。" },
      { q: "手机远程需要公网 IP 吗？安全吗？", a: (<>不需要公网 IP：同一个 Wi-Fi 下，或者通过 Tailscale 都能连。连接是明文 HTTP，靠访问令牌鉴权，所以在不可信的网络里请只绑定 Tailscale 网卡（推荐）。重新生成令牌后，已配对的手机都要重新扫码。设置步骤见<Link href={`${docs}#remote`}>手机远程说明</Link>。</>) },
      { q: "Agent 能操作右侧面板的浏览器吗？能叫停吗？", a: "claude、codex、kimi 会话启动时会接上一组浏览器工具，能在面板浏览器里打开网页、点击、输入、读内容、截图。操作时你看得到它的鼠标，随时点「接管」就能暂停它。在设置里关掉后，正在跑的会话也立刻用不了。" },
      { q: "能看到 Agent 的任务进度吗？", a: "能。进度面板列出 Agent 的待办清单、派出去的子任务，以及还在后台跑的命令，跟着进度刷新。后台命令还在跑时，会话不会显示成「等你输入」。" },
      { q: "代理支持哪些格式，在哪里生效？", a: (<>可以设置全局默认规则，也可以按 Agent 单独设置。支持 HTTP、带用户名密码的 URL，以及 <code className="inline">host:port:user:pass</code>；SOCKS 只有 Kimi 和 Gemini CLI 认。配置会用于 Meowo 的用量查询、CLI 安装和从 Meowo 启动的会话；不同 CLI 的协议与覆盖范围有差异，设置页会显示实际生效情况。</>) },
      { q: "同一个工具能挂多个账号吗？", a: "Claude Code、Codex、Kimi、OpenCode 可以，Gemini CLI 只有一个账号。在设置里添加多个官方账号，每个账号有独立的登录凭据和会话历史。点一下就能切换，额度读数和登录状态跟着当前账号走；Claude Code、Codex、Kimi 还会问你要不要把正在跑的会话一起切过去。" },
      { q: "没有官方账号，能用 API 中转吗？", a: "可以。在账号设置里给某一家 Agent 接入 API 中转：填入中转地址、模型名和密钥即可启用，模型名可以从推荐项选，也能填中转商提供的任意 ID。官方账号和 API 中转两种方式互斥，用中转时官方额度不适用。" },
      { q: "Windows 提示「Windows 已保护你的电脑」，正常吗？", a: (<>正常。Windows 安装包暂时没有代码签名，首次运行时 SmartScreen 会这样提示，点「更多信息」→「仍要运行」即可。安装包由 GitHub Actions 从公开源码构建，应用内的自动更新另有 minisign 签名校验。macOS 安装包已签名公证。</>) },
      { q: "外观能改吗？", a: "能。深色、浅色或跟随系统；界面缩放和对话内容宽度可调；打开「消息自动折叠」后只留最终回答。改动实时生效。灵动岛本身固定是黑色，好和屏幕顶边融成一体。" },
      { q: "支持 Linux 吗？", a: "目前只提供 Windows 和 macOS 的安装包。Linux 打包在计划里，还没做。进展看 GitHub Releases。" },
      { q: "会一直占用系统资源吗？", a: "不会。reporter 只在触发 hook 时启动，写完数据库就退出。常驻的只有 Meowo 应用本身，它靠监听本地文件变化来刷新，开销很小。" },
      { q: "免费吗？可以商用吗？", a: (<>免费，开源，MIT 许可证，可以自由使用和修改。源码见 <a href={REPO} target="_blank" rel="noopener noreferrer">GitHub</a>。</>) },
      { q: "如何卸载？会残留什么？", a: (<>按系统常规方式卸载应用。会话数据和应用设置位于 <code className="inline">~/.meowo/</code>。如需彻底清理，再从各 AI CLI 的 hook 配置中删除命令包含 <code className="inline">meowo-reporter</code> 的条目；不要删除其他自定义 hooks。</>) },
    ],
  };
}

export default function FaqContent({ lang }: { lang: Lang }) {
  const { head, qa, ctaTitle, ctaSub } = build(lang);
  return (
    <main>
      <section className="pagehead">
        <div className="container">
          <span className="eyebrow">{head.eyebrow}</span>
          <h1 className="h1">{head.title}</h1>
          <p className="lead">{head.lead}</p>
        </div>
      </section>

      <section className="section-sm">
        <div className="container">
          <div className="faq">
            {qa.map((item, i) => (
              <details key={item.q} open={i === 0}>
                <summary>
                  {item.q}
                  <span className="chev">
                    <PlusIcon />
                  </span>
                </summary>
                <div className="faq-a">{item.a}</div>
              </details>
            ))}
          </div>
        </div>
      </section>

      <CtaBand lang={lang} title={ctaTitle} subtitle={ctaSub} />
    </main>
  );
}
