import type { Metadata } from "next";
import { PlusIcon } from "@/components/icons";
import CtaBand from "@/components/CtaBand";

export const metadata: Metadata = {
  title: "FAQ · Meowo",
  description:
    "Meowo 常见问题：支持哪些 AI CLI、如何减少命令输入、代理设置、数据是否上传、自动接入、隐私与卸载。",
};

const QA: { q: string; a: React.ReactNode }[] = [
  {
    q: "Meowo 支持哪些 AI 编程 CLI？",
    a: "当前内置支持 Claude Code、Codex 和 Kimi。它们各自通过 CLI 的 hook 上报事件，数据写进本地同一份数据库，所以能显示在同一个窗口里。",
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
        一般不需要。Meowo 启动时会为检测到的 AI 编程 CLI 接入 reporter，
        写之前先备份，不会破坏已有配置。需要排查或手动接入时见<a href="/docs">文档</a>。
      </>
    ),
  },
  {
    q: "这和直接看终端有什么区别？",
    a: "看终端要你主动切换窗口，还得记住会话位置和不同工具的命令。Meowo 把状态与常用操作放在桌面一角：需要回复时提醒你，点击即可回到、新建或续接会话。",
  },
  {
    q: "Meowo 能帮我省掉哪些命令？",
    a: "新建会话时直接选择项目目录和 AI 工具；恢复会话时点击卡片即可，不用查会话 ID 或记各工具的续接参数。CLI 安装、登录、hooks 修复和代理设置也提供了界面入口。",
  },
  {
    q: "代理支持哪些格式，在哪里生效？",
    a: (
      <>
        可以设置全局默认规则，也可以按 AI 工具覆盖。支持 HTTP、SOCKS5、带用户名密码的 URL，以及{" "}
        <code className="inline">host:port:user:pass</code>。配置会用于 Meowo 的用量查询、CLI 安装和从 Meowo
        启动的会话；不同 CLI 的协议与覆盖范围有差异，设置页会显示实际生效情况。
      </>
    ),
  },
  {
    q: "点击卡片怎么切到终端的？",
    a: "连接中的会话会切到它所在的终端标签页（Windows 上是 Windows Terminal，macOS 上是 Terminal 或 iTerm2）。已断开的会话，Meowo 会回到原项目目录，并按对应 AI 工具的方式续接。使用哪个终端可以在设置里指定。",
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
        按系统常规方式卸载应用。会话数据和应用设置位于{" "}
        <code className="inline">~/.meowo/</code>。如需彻底清理，再从各 AI CLI 的 hook 配置中删除命令包含{" "}
        <code className="inline">meowo-reporter</code> 的条目；不要删除其他自定义 hooks。
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
          <p className="lead">关于支持范围、自动接入、代理、隐私与平台兼容。</p>
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
