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
      head: { eyebrow: "FAQ", title: "Frequently asked questions", lead: "About supported tools, auto-connect, proxies, privacy, and platform support." },
      ctaTitle: "Still have questions?",
      ctaSub: "Open an Issue or Discussion on GitHub.",
      qa: [
        { q: "Which AI coding CLIs does Meowo support?", a: "Currently Claude Code, Codex, Kimi, Gemini CLI, and OpenCode are built in. Each reports events through its CLI hooks into the same local database, so they all show up in one window." },
        { q: "Is my session data uploaded to the cloud?", a: (<>No. Data is only written to a local <code className="inline">~/.meowo/board.db</code> (SQLite). The reporter and the app talk only through this local file; Meowo sends no session content to any server.</>) },
        { q: "Do I need to configure hooks manually?", a: (<>Usually not. On startup Meowo wires the reporter into the AI coding CLIs it detects, backing up first so your existing config isn't broken. For troubleshooting or manual setup, see the <Link href={docs}>docs</Link>.</>) },
        { q: "How is this different from just watching the terminal?", a: "Watching the terminal means switching windows yourself and remembering each session's spot and each tool's commands. Meowo puts status and common actions in a corner of your desktop: it nudges you when a reply is needed, and one click gets you back, starts, or resumes a session." },
        { q: "Which commands does Meowo save me?", a: "New sessions: just pick a project directory and AI tool. Resuming: click the card — no session IDs or per-tool resume flags to remember. CLI install, sign-in, hook repair, and proxy setup all have a UI too." },
        { q: "What proxy formats are supported, and where do they apply?", a: (<>Set a global default rule, or override per AI tool. Supports HTTP, SOCKS5, URLs with username/password, and <code className="inline">host:port:user:pass</code>. The config applies to Meowo's usage queries, CLI installs, and sessions launched from Meowo; protocol support and scope vary by CLI, and the settings page shows what's actually in effect.</>) },
        { q: "How does clicking a card switch to the terminal?", a: "A connected session switches to the terminal tab it lives in (Windows Terminal on Windows, Terminal or iTerm2 on macOS). A disconnected one has Meowo return to its project directory and resume it the tool's way. You choose which terminal in settings." },
        { q: "Can I keep multiple accounts for one tool?", a: "Yes. Add several official accounts for the same tool in settings; each has its own login credentials and session history, kept separate. Switch with one click and quota readouts and login state follow the current account." },
        { q: "No official account — can I use an API relay?", a: "Yes. Connect an API relay per model in account settings: fill in the relay address, model name, and key to enable it; pick the model from suggestions or enter any ID your relay provides. Official account and API relay are mutually exclusive; you keep using the official account while configuring the relay, and the official quota doesn't apply when the relay is active." },
        { q: "Can I change the sticker's look?", a: "Yes — 7 sticker colors, flat or dimensional styles, and dark / light / follow-system themes, plus opacity (to let the frosted desktop show through) and UI density. Changes apply live, across both windows." },
        { q: "Does it support Linux?", a: "Only Windows and macOS installers are provided for now. Linux packaging is planned but not done yet. Track progress in GitHub Releases." },
        { q: "Does it keep using system resources?", a: "No. The reporter only starts when a hook fires, writes the database, and exits. Only the window stays resident; it refreshes by watching local file changes, at very low cost." },
        { q: "Is it free? Can I use it commercially?", a: (<>Free, open source, MIT-licensed — use and modify freely. Source on <a href={REPO} target="_blank" rel="noopener noreferrer">GitHub</a>.</>) },
        { q: "How do I uninstall, and what's left behind?", a: (<>Uninstall the app the usual way for your OS. Session data and settings live in <code className="inline">~/.meowo/</code>. To fully clean up, also remove entries whose command contains <code className="inline">meowo-reporter</code> from each AI CLI's hook config; don't delete your other custom hooks.</>) },
      ],
    };
  }
  return {
    head: { eyebrow: "FAQ", title: "常见问题", lead: "关于支持范围、自动接入、代理、隐私与平台兼容。" },
    ctaTitle: "还有问题？",
    ctaSub: "可以在 GitHub 上开 Issue 或 Discussion。",
    qa: [
      { q: "Meowo 支持哪些 AI 编程 CLI？", a: "当前内置支持 Claude Code、Codex、Kimi、Gemini CLI 和 OpenCode。它们各自通过 CLI 的 hook 上报事件，数据写进本地同一份数据库，所以能显示在同一个窗口里。" },
      { q: "我的会话数据会上传到云端吗？", a: (<>不会。数据只写进本地的 <code className="inline">~/.meowo/board.db</code>（SQLite）。reporter 和 app 只通过这个本地文件通信，Meowo 不把会话内容发到任何服务器。</>) },
      { q: "需要手动配置 hooks 吗？", a: (<>一般不需要。Meowo 启动时会为检测到的 AI 编程 CLI 接入 reporter，写之前先备份，不会破坏已有配置。需要排查或手动接入时见<Link href={docs}>文档</Link>。</>) },
      { q: "这和直接看终端有什么区别？", a: "看终端要你主动切换窗口，还得记住会话位置和不同工具的命令。Meowo 把状态与常用操作放在桌面一角：需要回复时提醒你，点击即可回到、新建或续接会话。" },
      { q: "Meowo 能帮我省掉哪些命令？", a: "新建会话时直接选择项目目录和 AI 工具；恢复会话时点击卡片即可，不用查会话 ID 或记各工具的续接参数。CLI 安装、登录、hooks 修复和代理设置也提供了界面入口。" },
      { q: "代理支持哪些格式，在哪里生效？", a: (<>可以设置全局默认规则，也可以按 AI 工具覆盖。支持 HTTP、SOCKS5、带用户名密码的 URL，以及 <code className="inline">host:port:user:pass</code>。配置会用于 Meowo 的用量查询、CLI 安装和从 Meowo 启动的会话；不同 CLI 的协议与覆盖范围有差异，设置页会显示实际生效情况。</>) },
      { q: "点击卡片怎么切到终端的？", a: "连接中的会话会切到它所在的终端标签页（Windows 上是 Windows Terminal，macOS 上是 Terminal 或 iTerm2）。已断开的会话，Meowo 会回到原项目目录，并按对应 AI 工具的方式续接。使用哪个终端可以在设置里指定。" },
      { q: "同一个工具能挂多个账号吗？", a: "可以。在设置里为同一个工具添加多个官方账号，每个账号有独立的登录凭据与会话历史，互不影响。点一下即可切换，配额读数和登录状态会跟着当前账号走。" },
      { q: "没有官方账号，能用 API 中转吗？", a: "可以。在账号设置里按模型接入 API 中转：填入中转地址、模型名和密钥即可启用，模型名可从推荐项选择或直接填中转商提供的任意 ID。官方账号与 API 中转两种接入方式互斥；配置中转期间仍使用官方账号，用中转时官方配额不适用。" },
      { q: "贴纸的外观能改吗？", a: "能。提供 7 种贴纸配色、扁平与立体两种风格、深 / 浅 / 跟随系统三种主题，还能调整不透明度（配合系统毛玻璃透出桌面）和界面密度。改动实时生效，两个窗口一起套用。" },
      { q: "支持 Linux 吗？", a: "目前只提供 Windows 和 macOS 的安装包。Linux 打包在计划里，还没做。进展看 GitHub Releases。" },
      { q: "会一直占用系统资源吗？", a: "不会。reporter 只在触发 hook 时启动，写完数据库就退出。常驻的只有窗口本身，它靠监听本地文件变化来刷新，开销很小。" },
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
