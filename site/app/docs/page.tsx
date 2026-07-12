import type { Metadata } from "next";
import { DOCS_CLAUDE_CODE } from "@/lib/site";
import { InfoIcon } from "@/components/icons";

export const metadata: Metadata = {
  title: "文档 · Meowo",
  description:
    "Meowo 使用文档：工作原理、接入 Claude Code、手动挂 hooks、数据与配置文件位置。",
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
          <h1 className="h1">它是怎么跑起来的</h1>
          <p className="lead">数据从哪来、怎么接进 Claude Code、文件都放在哪。</p>
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
                  是无状态的一次性进程：每次触发 hook 都会启动它，读取事件、写库后立即退出，不会阻塞会话。
                </li>
                <li>
                  <strong>meowo-app</strong>{" "}
                  启动时监听 <code className="inline">~/.meowo/</code>{" "}
                  目录变化，库一变就刷新 UI；同时跑后台任务标记空闲会话、首次导入历史会话。
                </li>
                <li>两端只通过这块 SQLite 通信，运行时不直接依赖。</li>
              </ul>

              <h2 id="connect">接入 Claude Code</h2>
              <p>
                贴纸里要有东西可看，就得先把 <code className="inline">meowo-reporter</code>{" "}
                挂到 Claude Code 的 hooks 上。
              </p>
              <div className="callout">
                <span className="ci">
                  <InfoIcon />
                </span>
                <span>
                  <strong>使用安装包时通常无需手动操作。</strong> meowo-app
                  每次启动会自动把 reporter 接入{" "}
                  <code className="inline">~/.claude/settings.json</code>——补齐所需
                  hook 事件，并把 statusLine 包装成{" "}
                  <code className="inline">~/.meowo/statusline.sh</code>{" "}
                  以获取准确的 Context 百分比。全程先备份、原子写、已正确则不改。前提是{" "}
                  <code className="inline">~/.claude/settings.json</code>{" "}
                  已存在（运行过一次 Claude Code 即会生成）。
                </span>
              </div>
              <p>
                挂好之后，新开的 Claude Code 会话就会自己冒到贴纸里。Claude Code 本身的用法见{" "}
                <a href={DOCS_CLAUDE_CODE} target="_blank" rel="noopener noreferrer">
                  官方文档
                </a>
                。
              </p>

              <h2 id="manual">手动挂 hooks（可选）</h2>
              <p>如果你不想先启动 app，或者要写到别的 settings 路径，可以自己来：</p>
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
                脚本会把 reporter 挂到所需 hook 事件（SessionStart / UserPromptSubmit /
                PostToolUse / Stop / SessionEnd / PermissionRequest，以及 PreToolUse
                的 AskUserQuestion / ExitPlanMode，均带 5s 超时）。用同一路径重复运行不会重复追加，也不会破坏已有 hooks。
              </p>
              <div className="callout">
                <span className="ci">
                  <InfoIcon />
                </span>
                <span>
                  此脚本仅用于 Claude Code。codex / kimi 的接入走各自 CLI 的原生 hook
                  配置（其 hook 命令带 <code className="inline">--provider codex|kimi</code>），不经此脚本。
                </span>
              </div>

              <h2 id="data">数据与配置</h2>
              <ul>
                <li>
                  <strong>数据库</strong>：<code className="inline">~/.meowo/board.db</code>（SQLite，WAL）。可用{" "}
                  <code className="inline">MEOWO_DB</code> 覆盖路径。
                </li>
                <li>
                  <strong>应用设置</strong>：<code className="inline">~/.meowo/settings.json</code>（通知开关、主题、不透明度、界面密度、归档自动隐藏天数、恢复终端等）。
                </li>
                <li>
                  <strong>用量缓存</strong>：<code className="inline">~/.meowo/usage-cache.json</code>。
                </li>
                <li>
                  <strong>statusLine 包装脚本</strong>：<code className="inline">~/.meowo/statusline.sh</code>（app 自动生成维护，无需手改）。
                </li>
                <li>
                  <strong>首次导入标记</strong>：<code className="inline">~/.meowo/imported.json</code>。删掉它可让下次启动重新导入近期历史会话。
                </li>
              </ul>
            </article>
          </div>
        </div>
      </section>
    </main>
  );
}
