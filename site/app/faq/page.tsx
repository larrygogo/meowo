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
    a: "目前支持 Claude Code、Codex、Kimi。它们各自通过 CLI 的 hook 上报事件，数据都写到本地同一份数据库里，所以能在同一张贴纸上显示。",
  },
  {
    q: "我的会话数据会上传到云端吗？",
    a: (
      <>
        不会。所有数据只写入本地 <code className="inline">~/.meowo/board.db</code>（SQLite），
        reporter 与 app 只通过这块本地库通信，Meowo 不把会话内容传到任何服务器。
      </>
    ),
  },
  {
    q: "需要手动配置 hooks 吗？",
    a: (
      <>
        通常不需要。使用安装包时，app 每次启动会自动把 reporter 接入
        Claude Code 的设置，先备份、再写入、不破坏已有配置。如果想手动配置，详见
        <a href="/docs">文档</a>。
      </>
    ),
  },
  {
    q: "这和直接看终端有什么区别？",
    a: "终端需要你主动切过去、记住哪个窗口对应哪个会话。Meowo 把这些信息常驻在桌面一角，状态变了实时更新，等你回复时还会弹通知。",
  },
  {
    q: "点击卡片为什么能直达终端？",
    a: "连接中的会话会精确切到它所在的终端标签页（Windows 的 Windows Terminal、macOS 的 Terminal / iTerm2）；断开的会话则在原项目目录新开终端并 claude --resume 续上对话。所用终端可在设置里指定。",
  },
  {
    q: "支持 Linux 吗？",
    a: "目前提供 Windows 与 macOS 安装包，Linux 打包在路线图上、尚未提供。可关注 GitHub Releases 获取后续进展。",
  },
  {
    q: "会一直占用系统资源吗？",
    a: "不会。reporter 只在触发 hook 时启动，写完数据库就退出。常驻的只有贴纸窗口本身，它通过监听本地文件变化来刷新，开销很小。",
  },
  {
    q: "免费吗？可以商用吗？",
    a: (
      <>
        完全免费、开源，采用 MIT 许可证，可自由使用与修改。源码见{" "}
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
        按系统常规方式卸载应用即可。个人数据都在{" "}
        <code className="inline">~/.meowo/</code> 目录下，删除该目录即彻底清理；接入 Claude Code
        的 hooks 可在 <code className="inline">~/.claude/settings.json</code> 中移除对应条目。
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
          <p className="lead">关于隐私、接入、平台与使用的一些常见疑问。</p>
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
