import type { Metadata } from "next";
import { PlusIcon } from "@/components/icons";
import CtaBand from "@/components/CtaBand";

export const metadata: Metadata = {
  title: "FAQ · Meowo",
  description:
    "Meowo 常见问题：支持哪些 AI CLI、数据是否上传、是否需要手动配置、怎么跳转终端、隐私与卸载。",
};

const QA: { q: string; a: React.ReactNode }[] = [
  {
    q: "Meowo 支持哪些 AI 编程 CLI？",
    a: "Claude Code、Codex、Kimi。它们各自通过 CLI 的 hook 上报事件，数据写进本地同一份数据库，所以能显示在同一个窗口里。",
  },
  {
    q: "我的会话数据会上传到云端吗？",
    a: (
      <>
        不会。数据只写进本地的 <code className="inline">~/.meowo/board.db</code>（SQLite）。
        reporter 和 app 只通过这个本地文件通信，Meowo 不把会话内容发到任何服务器。
      </>
    ),
  },
  {
    q: "需要手动配置 hooks 吗？",
    a: (
      <>
        一般不需要。用安装包时，app 每次启动会把 reporter 写进 Claude Code 的设置，
        写之前先备份，不会破坏已有配置。想手动配置的话见<a href="/docs">文档</a>。
      </>
    ),
  },
  {
    q: "这和直接看终端有什么区别？",
    a: "看终端要你主动切过去，还得记住哪个窗口对应哪个会话。Meowo 把这些信息放在桌面一角，状态变化时自动更新；会话等你回复时可以弹通知。",
  },
  {
    q: "点击卡片怎么切到终端的？",
    a: "连接中的会话会切到它所在的终端标签页（Windows 上是 Windows Terminal，macOS 上是 Terminal 或 iTerm2）。已断开的会话，Meowo 在原项目目录新开终端并执行 claude --resume 续接对话。用哪个终端可以在设置里指定。",
  },
  {
    q: "支持 Linux 吗？",
    a: "目前只提供 Windows 和 macOS 的安装包。Linux 打包在计划里，还没做。进展看 GitHub Releases。",
  },
  {
    q: "会一直占用系统资源吗？",
    a: "不会。reporter 只在触发 hook 时启动，写完数据库就退出。常驻的只有窗口本身，它靠监听本地文件变化来刷新，开销很小。",
  },
  {
    q: "免费吗？可以商用吗？",
    a: (
      <>
        免费，开源，MIT 许可证，可以自由使用和修改。源码见{" "}
        <a
          href="https://github.com/larrygogo/meowo"
          target="_blank"
          rel="noopener noreferrer"
        >
          GitHub
        </a>
        。
      </>
    ),
  },
  {
    q: "如何卸载？会残留什么？",
    a: (
      <>
        按系统常规方式卸载应用。数据都在{" "}
        <code className="inline">~/.meowo/</code> 目录下，删掉这个目录就清干净了。之前写进 Claude Code
        的 hooks，在 <code className="inline">~/.claude/settings.json</code> 里删掉对应条目即可。
      </>
    ),
  },
];

export default function FaqPage() {
  return (
    <main>
      <section className="pagehead">
        <div className="container">
          <span className="eyebrow">FAQ</span>
          <h1 className="h1">常见问题</h1>
          <p className="lead">隐私、接入、平台支持、卸载。</p>
        </div>
      </section>

      <section className="section-sm">
        <div className="container">
          <div className="faq">
            {QA.map((item, i) => (
              <details key={i} open={i === 0}>
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

      <CtaBand
        title="还有问题？"
        subtitle="可以在 GitHub 上开 Issue 或 Discussion。"
      />
    </main>
  );
}
