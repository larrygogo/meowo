import type { Metadata } from "next";
import { PlusIcon } from "@/components/icons";
import CtaBand from "@/components/CtaBand";

export const metadata: Metadata = {
  title: "FAQ · Meowo",
  description:
    "关于 Meowo 的常见问题：支持哪些 AI CLI、数据是否上传、是否需要手动配置、如何跳转终端、隐私与卸载等。",
};

const QA: { q: string; a: React.ReactNode }[] = [
  {
    q: "Meowo 支持哪些 AI 编程 CLI？",
    a: "Claude Code、Codex、Kimi。三者各自通过自己 CLI 的 hook 上报事件，最后都写进同一份本地数据库，所以能摆在同一张贴纸上。",
  },
  {
    q: "我的会话内容会被传到云上吗？",
    a: (
      <>
        不会。数据只写进本地的 <code className="inline">~/.meowo/board.db</code>（一个 SQLite
        文件），reporter 和 app 之间也只靠这个文件通信。没有服务端，会话内容不往任何地方发。
      </>
    ),
  },
  {
    q: "需要自己配 hooks 吗？",
    a: (
      <>
        一般不用。app 每次启动都会检查一遍，把 reporter 接进 Claude Code 的设置——先备份、再原子写入，已有的配置不会被弄坏。想自己动手的话看
        <a href="/docs">文档</a>。
      </>
    ),
  },
  {
    q: "这跟我自己看终端有什么区别？",
    a: "终端得你主动切过去，还得记住哪个窗口是哪个会话。Meowo 就待在桌面一角，状态一变就更新；它开始等你回复的时候，会主动叫你一声。",
  },
  {
    q: "点卡片是怎么跳回终端的？",
    a: "还连着的会话，会精确切到它所在的那个标签页——Windows 上是 Windows Terminal，macOS 上是 Terminal 或 iTerm2。已经断开的，就在原项目目录新开一个终端跑 claude --resume。用哪个终端可以在设置里指定。",
  },
  {
    q: "支持 Linux 吗？",
    a: "还没有。现在只有 Windows 和 macOS 的安装包，Linux 在计划里但没做。可以盯着 GitHub Releases。",
  },
  {
    q: "会一直吃资源吗？",
    a: "reporter 只有 hook 触发时才跑起来，写完库立刻退出。常驻的只有贴纸窗口，它靠监听本地文件变化来刷新，开销很小。",
  },
  {
    q: "免费吗？能商用吗？",
    a: (
      <>
        免费，开源，MIT 协议，随便用随便改。源码在{" "}
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
    q: "卸载干净吗？",
    a: (
      <>
        照系统常规方式卸载就行。数据全在{" "}
        <code className="inline">~/.meowo/</code> 这个目录里，删掉它就没了。挂在{" "}
        <code className="inline">~/.claude/settings.json</code> 里的 hooks 条目要手动删一下。
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
          <p className="lead">隐私、接入、平台，被问得比较多的几个。</p>
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
        title="没找到答案？"
        subtitle="去 GitHub 开个 Issue 或者 Discussion 问一声。"
      />
    </main>
  );
}
