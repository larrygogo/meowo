import type { Metadata } from "next";
import { DOCS_CLAUDE_CODE } from "@/lib/site";
import { InfoIcon } from "@/components/icons";

export const metadata: Metadata = {
  title: "文档 · Meowo",
  description:
    "Meowo 文档：工作原理、接入 Claude Code、手动挂 hooks、数据与配置文件的位置。",
};

const TOC = [
  { id: "how", label: "工作原理" },
  { id: "connect", label: "接入 Claude Code" },
  { id: "manual", label: "手动挂 hooks" },
  { id: "data", label: "数据与配置" },
];

export default function DocsPage() {
  return (
    <main>
      <section className="pagehead">
        <div className="container">
          <span className="eyebrow">文档</span>
          <h1 className="h1">文档</h1>
          <p className="lead">工作原理、接入方式、数据和配置文件的位置。</p>
        </div>
      </section>

      <section className="section-sm">
        <div className="container">
          <div className="doc-layout">
            <aside className="doc-toc">
              <div className="toc-title">目录</div>
              {TOC.map((t) => (
                <a key={t.id} href={`#${t.id}`}>
                  {t.label}
                </a>
              ))}
            </aside>

            <article className="prose">
              <h2 id="how">工作原理</h2>
              <p>
                以 Claude Code 为例；Codex / Kimi 走各自 CLI 的 hook
                机制，数据最终都落到同一份本地数据库。
              </p>
              <div className="codeblock">
                <pre>
                  {` Claude Code 会话
   │  触发 hooks（SessionStart / UserPromptSubmit / PostToolUse / Stop …）
   │  渲染 statusline（取得准确的 Context 百分比）
   ▼
 meowo-reporter（命令行，读 stdin 的事件 JSON）
   │  解析事件、标题、项目、todo、Context 用量
   ▼
 ~/.meowo/board.db（SQLite，WAL）
   ▲
   │  文件监听 + 去抖刷新
 meowo-app（Tauri 贴纸，React 前端）`}
                </pre>
              </div>
              <ul>
                <li>
                  <strong>meowo-reporter</strong>{" "}
                  是一次性进程，不保存状态。每次触发 hook 时启动，读取事件、写库，然后退出，不会阻塞会话。
                </li>
                <li>
                  <strong>meowo-app</strong> 启动时监听{" "}
                  <code className="inline">~/.meowo/</code>{" "}
                  目录的变化，库一变就刷新界面。它还跑两个后台任务：标记空闲会话，以及首次启动时导入历史会话。
                </li>
                <li>两端只通过这个 SQLite 文件通信，运行时不互相依赖。</li>
              </ul>

              <h2 id="connect">接入 Claude Code</h2>
              <p>
                窗口里要有数据，得把 <code className="inline">meowo-reporter</code>{" "}
                挂到 Claude Code 的 hooks 上。
              </p>
              <div className="callout">
                <span className="ci">
                  <InfoIcon />
                </span>
                <span>
                  <strong>用安装包时一般不需要手动操作。</strong> meowo-app
                  每次启动会把 reporter 写进{" "}
                  <code className="inline">~/.claude/settings.json</code>：补齐所需的
                  hook 事件，并把 statusLine 包装成{" "}
                  <code className="inline">~/.meowo/statusline.sh</code>，以便拿到准确的
                  Context 百分比。写之前先备份，写入是原子的；如果配置已经正确就不动它。前提是{" "}
                  <code className="inline">~/.claude/settings.json</code>{" "}
                  已经存在（运行过一次 Claude Code 就会生成）。
                </span>
              </div>
              <p>
                挂好之后，新开的 Claude Code 会话就会出现在窗口里。Claude Code 本身见{" "}
                <a href={DOCS_CLAUDE_CODE} target="_blank" rel="noopener noreferrer">
                  官方文档
                </a>
                。
              </p>

              <h2 id="manual">手动挂 hooks（可选）</h2>
              <p>不想先启动 app，或者要写入自定义的 settings 路径时，可以手动挂：</p>
              <div className="codeblock">
                <pre>
                  {`# 1. 编译 meowo-reporter
cargo build --release -p meowo-reporter
# 产物：target/release/meowo-reporter(.exe)

# 2. 接入 ~/.claude/settings.json 的 hooks（用绝对路径）
bun scripts/install-hooks.mjs "<仓库绝对路径>/target/release/meowo-reporter.exe"`}
                </pre>
              </div>
              <p>
                脚本会把 reporter 挂到需要的 hook 事件上（SessionStart / UserPromptSubmit /
                PostToolUse / Stop / SessionEnd / PermissionRequest，以及 PreToolUse
                的 AskUserQuestion / ExitPlanMode，都带 5s 超时）。用同一个路径重复运行不会重复追加，也不会破坏已有的 hooks。
              </p>
              <div className="callout">
                <span className="ci">
                  <InfoIcon />
                </span>
                <span>
                  这个脚本只管 Claude Code。codex 和 kimi 的接入走各自 CLI 的原生 hook
                  配置（hook 命令带 <code className="inline">--provider codex|kimi</code>），不经过它。
                </span>
              </div>

              <h2 id="data">数据与配置</h2>
              <ul>
                <li>
                  <strong>数据库</strong>：<code className="inline">~/.meowo/board.db</code>（SQLite，WAL）。可用{" "}
                  <code className="inline">MEOWO_DB</code> 覆盖路径。
                </li>
                <li>
                  <strong>应用设置</strong>：<code className="inline">~/.meowo/settings.json</code>。通知开关、主题、不透明度、界面密度、归档自动隐藏天数、用哪个终端恢复会话，都在这里。
                </li>
                <li>
                  <strong>用量缓存</strong>：<code className="inline">~/.meowo/usage-cache.json</code>。
                </li>
                <li>
                  <strong>statusLine 包装脚本</strong>：<code className="inline">~/.meowo/statusline.sh</code>。由 app 生成和维护，不用手改。
                </li>
                <li>
                  <strong>首次导入标记</strong>：<code className="inline">~/.meowo/imported.json</code>。删掉它，下次启动会重新导入最近的历史会话。
                </li>
              </ul>
            </article>
          </div>
        </div>
      </section>
    </main>
  );
}
