import { createContext, memo, useContext, useEffect, useState, type ReactNode } from "react";
import { useT } from "../../i18n";
import type { DiffHunk } from "../../generated/contracts/DiffHunk";
import type { ToolDetail } from "../../generated/contracts/ToolDetail";
import { fmtElapsed } from "./cliStatus";
import { CommandLines, dedent, OutputBlock } from "./terminalText";
import { highlightLines, languageForPath } from "./highlight";
import { stripAnsi, type ToolResultItem, type ToolUseItem } from "./shared";

/// 工具名的展示词。命中不了的原样回落成工具 id——比编一个错译好，但**常见工具**
/// 必须都译到：此前只译了 Bash/Read/Write 三家，一组活动摘要读起来是
/// 「运行终端 ×2 · Grep · WebFetch」这样的中英混排（7C-9）。
/// 判定顺序有讲究：web_search 含 "search"，网络类必须排在搜索类之前。
export function friendlyToolName(name: string, t: ReturnType<typeof useT>): string {
  const normalized = name.toLowerCase();
  if (normalized === "bash" || normalized === "exec" || normalized.includes("shell") || normalized.includes("terminal")) return t.chat.runTerminal;
  if (normalized.includes("web") || normalized.includes("fetch") || normalized.includes("http")) return t.chat.fetchWeb;
  if (normalized.includes("grep") || normalized.includes("glob") || normalized.includes("search")) return t.chat.searchFiles;
  if (normalized === "read" || normalized.includes("view_image")) return t.chat.readFile;
  // Write 与 Edit 分开叫：终端里就是两个词，「写入 225 行」挂在「编辑文件」下面读着别扭。
  if (normalized === "write") return t.chat.writeFile;
  if (normalized === "edit" || normalized.includes("patch") || normalized.includes("notebook")) return t.chat.editFile;
  if (normalized.includes("todo")) return t.chat.updateTodos;
  if (normalized === "task" || normalized.includes("agent")) return t.chat.runAgent;
  if (normalized === "ls" || normalized.includes("list_dir") || normalized.includes("listdir")) return t.chat.listDir;
  return name;
}

export type ToolKind = "search" | "read" | "write" | "edit" | "run" | "web" | "todo" | "agent" | "other";

/// 工具的语义类别，驱动时间线上的类型标签与配色（data-kind）。判定顺序与
/// friendlyToolName 一致：网络类先于搜索类（web_search 含 "search"）。
export function toolKind(name: string): ToolKind {
  const normalized = name.toLowerCase();
  if (normalized === "bash" || normalized === "exec" || normalized.includes("shell") || normalized.includes("terminal")) return "run";
  if (normalized.includes("web") || normalized.includes("fetch") || normalized.includes("http")) return "web";
  if (normalized.includes("grep") || normalized.includes("glob") || normalized.includes("search")) return "search";
  if (normalized === "read" || normalized.includes("view_image")) return "read";
  if (normalized === "write") return "write";
  if (normalized === "edit" || normalized.includes("patch") || normalized.includes("notebook")) return "edit";
  if (normalized.includes("todo")) return "todo";
  if (normalized === "task" || normalized.includes("agent")) return "agent";
  return "other";
}


/// 按工具类型分图标：搜索 / 读文件 / 改文件 / 终端 / 网络 / 通用。
/// 一组操作若全是同一个 `>_`，用户在展开列表里无法一眼区分做了什么。
/// 全部按 1.8 描边画：放大镜曾是填充轮廓（内外两圈），与笔/文件并排时明显发粗。
export function ToolIcon({ name }: { name: string }) {
  const normalized = name.toLowerCase();
  const path = normalized.includes("grep") || normalized.includes("glob") || normalized.includes("search")
    ? "M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13zM15.5 15.5 20 20"
    : normalized === "read" || normalized.includes("view_image") || normalized.includes("notebook")
    ? "M6 3h8l4 4v14H6zM14 3v5h5"
    : normalized === "write" || normalized === "edit" || normalized.includes("patch")
    ? "M4 20h4L19.5 8.5a2.1 2.1 0 0 0-3-3L5 17zM13.5 6.5l3 3"
    : normalized.includes("fetch") || normalized.includes("web") || normalized.includes("http")
    ? "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zm-9 9h18M12 3c2.5 2.4 3.8 5.6 3.8 9s-1.3 6.6-3.8 9c-2.5-2.4-3.8-5.6-3.8-9s1.3-6.6 3.8-9z"
    : "M4 17l6-6-6-6M12 19h8";
  return (
    <span className="chat-tool-icon" data-kind={toolKind(name)}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><path d={path} /></svg>
    </span>
  );
}

/// 预览露出的行数上限。CC 终端里 Write 也只露十来行再写「+262 lines」——整篇摊开会把
/// 对话流顶掉几屏，而用户要的是「看一眼在写什么」，不是在对话页读文件。
const PREVIEW_LINES = 12;

/// 拆行：末尾的换行不算一行（"a\nb\n" 是两行），与后端 `lines()` 的口径一致。
function splitLines(text: string): string[] {
  const lines = text.split("\n");
  if (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
  return lines;
}

/// 带行号（或 diff 的 +/- 记号）的代码行。高亮复用文件面板那套 hljs——highlightLines
/// 逐行对齐，失败整体退纯文本，不会出现错位。
function CodeLines({ lines, lang, start = 1, tone, gutter }: {
  lines: string[];
  lang: string | null;
  start?: number;
  tone?: "add" | "del";
  /// 自定义行首记号（终端命令用 `$`）；缺省是行号，diff 态是 +/-。
  gutter?: (index: number) => string;
}) {
  const html = highlightLines(lines, lang);
  const mark = (index: number) => gutter ? gutter(index) : tone ? (tone === "add" ? "+" : "-") : String(start + index);
  return (
    <div className={"chat-tool-code" + (tone ? ` is-${tone}` : "")}>
      {html.map((line, index) => (
        <div className="chat-tool-line" key={index}>
          <span className="chat-tool-ln" aria-hidden="true">{mark(index)}</span>
          <span className="chat-tool-src" dangerouslySetInnerHTML={{ __html: line || " " }} />
        </div>
      ))}
    </div>
  );
}

/// 长正文只露前 PREVIEW_LINES 行，余下折成「还有 N 行 · 展开全部」；展开后可收回。
/// 后端截断过的正文在展开到底时提示「只保留了开头」——不能让用户以为文件就这么长。
function Preview({ text, lang, tone, truncated }: { text: string; lang: string | null; tone?: "add" | "del"; truncated?: boolean }) {
  const t = useT();
  const [all, setAll] = useState(false);
  const lines = splitLines(text);
  const shown = all ? lines : lines.slice(0, PREVIEW_LINES);
  const rest = lines.length - shown.length;
  const foldable = lines.length > PREVIEW_LINES;
  return (
    <>
      <CodeLines lines={shown} lang={lang} tone={tone} />
      {foldable && (
        <button type="button" className="chat-tool-more" onClick={() => setAll((value) => !value)}>
          {all ? t.chat.toolShowLess : `${t.chat.toolMoreLines(rest)} · ${t.chat.toolShowAll}`}
        </button>
      )}
      {truncated && all && <div className="chat-tool-note">{t.chat.toolTruncated}</div>}
    </>
  );
}

/// 一次写入/编辑增删了多少行。Edit 按 old/new 的行数算（同终端里的「-3 +4」，不做
/// 逐行 diff）；Write 只有新增。其余工具不改文件，返回 null。工具组摘要行据此汇总。
export function lineDelta(detail: ToolDetail | null | undefined): { added: number; removed: number } | null {
  if (detail?.kind === "write") return { added: detail.lines, removed: 0 };
  if (detail?.kind === "edit") return { added: splitLines(detail.new).length, removed: splitLines(detail.old).length };
  return null;
}


/// 展开后的详情正文：Write 是带行号的内容预览，Edit 是删/增两段，Bash 是 `$ 命令`。
/// Read / Search 的信息都已在标题行（路径 + 范围/模式），没有正文。
function DetailBody({ detail }: { detail: ToolDetail }): ReactNode {
  const t = useT();
  switch (detail.kind) {
    case "write":
      return <Preview text={detail.content} lang={languageForPath(detail.path)} truncated={detail.truncated} />;
    case "edit": {
      const lang = languageForPath(detail.path);
      return (
        <div className="chat-tool-diff">
          {detail.replace_all && <div className="chat-tool-note">{t.chat.toolReplaceAll}</div>}
          <Preview text={detail.old} lang={lang} tone="del" />
          <Preview text={detail.new} lang={lang} tone="add" truncated={detail.truncated} />
        </div>
      );
    }
    case "command":
      return <CommandLines command={detail.command} />;
    case "read":
    case "search":
      return null;
  }
}

/// 会话工作目录：工具行里落在它下面的路径显示成相对路径（终端里就是这样），
/// 绝对路径的长前缀会把真正有用的文件名挤出可视区（2026-10 用户实拍）。
export const ChatCwdContext = createContext<string | null>(null);

/// 去掉 cwd 前缀。Windows 路径大小写不敏感、正反斜杠混用，比较前统一归一化。
export function relativize(text: string, cwd: string | null): string {
  if (!cwd) return text;
  const norm = (value: string) => value.replace(/\\/g, "/").toLowerCase();
  const base = norm(cwd).replace(/\/+$/, "");
  if (norm(text.slice(0, base.length + 1)) === `${base}/`) return text.slice(base.length + 1);
  // 命令以「cd <cwd> && 」起头时剥掉这段（与运行条的 trimActivityCdPrefix 同一口径）。
  const cd = /^cd\s+("[^"]*"|'[^']*'|\S+)\s+&&\s*/.exec(text);
  if (cd && norm(cd[1].replace(/^["']|["']$/g, "")).replace(/\/+$/, "") === base) return text.slice(cd[0].length);
  return text;
}

/// 终端里工具调用的动词：Edit 显示成 Update、Grep/Glob 显示成 Search（claude 终端的叫法），
/// 其余原样用工具 id。这是「代码」不是文案，不随界面语言变。
function toolVerb(name: string, kind: ToolKind): string {
  if (kind === "edit") return "Update";
  if (kind === "search") return "Search";
  return name;
}

/// 回执正文的非空行（先剥终端着色码）。
function outputLines(text: string): string[] {
  return stripAnsi(text).split("\n").map((line) => line.trimEnd()).filter((line) => line.trim());
}

/// 运行中的那条每秒重渲染一次，第二行的计时才会走。跑完即停。
function useSecondTick(active: boolean) {
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => setTick((value) => value + 1), 1000);
    return () => window.clearInterval(timer);
  }, [active]);
}

/// 统一 diff（claude 回执的 structuredPatch）：终端同款「行号 + -/+ + 代码」，增删交错、
/// 带上下文。删行标旧文件行号、其余标新文件行号；多段之间放一行省略号。
function UnifiedDiff({ hunks, lang }: { hunks: DiffHunk[]; lang: string | null }) {
  return (
    <div className="chat-tool-code chat-tool-udiff">
      {hunks.map((hunk, hunkIndex) => {
        const html = highlightLines(hunk.lines.map((line) => line.slice(1)), lang);
        let oldNo = hunk.old_start;
        let newNo = hunk.new_start;
        return (
          <div key={hunkIndex}>
            {hunkIndex > 0 && <div className="chat-tool-line is-gap" aria-hidden="true"><span className="chat-tool-ln">⋯</span></div>}
            {hunk.lines.map((line, index) => {
              const mark = line[0] === "-" ? "-" : line[0] === "+" ? "+" : " ";
              let number: number;
              if (mark === "-") number = oldNo++;
              else if (mark === "+") number = newNo++;
              else { number = newNo++; oldNo++; }
              return (
                <div className={"chat-tool-line" + (mark === "-" ? " is-del" : mark === "+" ? " is-add" : "")} key={index}>
                  <span className="chat-tool-ln" aria-hidden="true">{number}</span>
                  <span className="chat-tool-mark" aria-hidden="true">{mark}</span>
                  <span className="chat-tool-src" dangerouslySetInnerHTML={{ __html: html[index] || " " }} />
                </div>
              );
            })}
          </div>
        );
      })}
    </div>
  );
}

const OUTPUT_HEAD_LINES = 3;

/// 第二行「⎿ 结果摘要」：终端在每次调用下面写一句结果（Updated … with 1 addition、
/// Read 160 lines、命令输出的头几行），对话页照搬这个形态。
function ResultLine({ item, result, kind, detail, patch }: {
  item: ToolUseItem;
  result?: ToolResultItem;
  kind: ToolKind;
  detail: ToolDetail | null;
  patch: DiffHunk[] | null;
}) {
  const t = useT();
  const [mountedAt] = useState(() => Date.now());
  useSecondTick(!result);
  if (!result) {
    const startedAt = Date.parse(item.timestamp ?? "");
    const since = Number.isNaN(startedAt) ? mountedAt : startedAt;
    return <span className="chat-tool-out is-running">{t.chat.toolRunningFor(fmtElapsed(Date.now() - since))}</span>;
  }
  const lines = outputLines(result.text);
  if (result.is_error) return <span className="chat-tool-out is-error">{lines[0] ?? t.chat.toolNoOutput}</span>;
  if (kind === "edit" || kind === "write") {
    if (patch) {
      const added = patch.reduce((n, hunk) => n + hunk.lines.filter((line) => line[0] === "+").length, 0);
      const removed = patch.reduce((n, hunk) => n + hunk.lines.filter((line) => line[0] === "-").length, 0);
      return <span className="chat-tool-out">{t.chat.toolEditSummary(added, removed)}</span>;
    }
    if (detail?.kind === "write") return <span className="chat-tool-out">{t.chat.toolWroteLines(detail.lines)}</span>;
    const delta = lineDelta(detail);
    if (delta) return <span className="chat-tool-out">{t.chat.toolEditSummary(delta.added, delta.removed)}</span>;
  }
  if (kind === "read") return <span className="chat-tool-out">{t.chat.toolReadLines(lines.length)}</span>;
  if (kind === "search") return <span className="chat-tool-out">{lines.length ? t.chat.toolFoundLines(lines.length) : t.chat.toolNoOutput}</span>;
  if (lines.length === 0) return <span className="chat-tool-out">{t.chat.toolNoOutput}</span>;
  // 命令等：像终端一样露输出的头 3 行，余下折成「还有 N 行」，全文在展开区。
  const head = dedent(lines.slice(0, OUTPUT_HEAD_LINES));
  return (
    <span className="chat-tool-out is-output">
      {head.join("\n")}
      {lines.length > head.length && <span className="chat-tool-out-more">{`\n… ${t.chat.toolMoreLines(lines.length - head.length)}`}</span>}
    </span>
  );
}

/// memo 理由同 Message：item/result 都来自 reducer，未变化时引用稳定。
/// 形态照搬 claude 终端（2026-10 用户要求「像终端」）：首行「● Update(src/a.ts)」，
/// 次行「⎿ 新增 3 行，删除 1 行」；展开区是 diff / 命令全文与完整输出。
export const ToolActivity = memo(function ToolActivity({ item, result }: { item: ToolUseItem; result?: ToolResultItem }) {
  const t = useT();
  const cwd = useContext(ChatCwdContext);
  const detail = item.detail ?? null;
  const kind = toolKind(item.name);
  const patch = result?.patch?.length ? result.patch : null;
  // 开合只在挂载时定一次，之后归用户（React 只在 prop 变化时碰 open）。跑着的那条展开看
  // 在做什么；编辑类恒展开——终端里 diff 就直接摊在调用下面。
  const [initialOpen] = useState(() => (!result && detail !== null) || kind === "edit");
  const command = detail?.kind === "command" ? detail : null;
  const path = detail && "path" in detail ? detail.path : null;
  return (
    <details
      className={"chat-tool" + (result?.is_error ? " is-error" : "")}
      // 时间线状态点读它：跑着 / 失败 / 完成三态。
      data-state={!result ? "running" : result.is_error ? "error" : "done"}
      open={initialOpen || undefined}
    >
      <summary>
        {/* 命令描述不单独占一行（宽度不够时会被挤成孤零零的一行，用户实拍嫌丑），进悬停提示。 */}
        <span className="chat-tool-call" data-tip={command?.description ?? undefined}>
          <span className="chat-tool-verb" data-kind={kind} aria-hidden="true">{toolVerb(item.name, kind)}</span>
          <span className="chat-tool-name chat-sronly">{friendlyToolName(item.name, t)}</span>
          <span className="chat-tool-paren" aria-hidden="true">(</span>
          <span className="chat-tool-summary">{relativize(item.summary, cwd)}</span>
          <span className="chat-tool-paren" aria-hidden="true">)</span>
        </span>
        <span className="chat-tool-chevron">›</span>
        <span className="chat-tool-result">
          <span className="chat-tool-elbow" aria-hidden="true">⎿</span>
          <ResultLine item={item} result={result} kind={kind} detail={detail} patch={patch} />
        </span>
      </summary>
      <div className="chat-tool-body">
        {patch
          ? <UnifiedDiff hunks={patch} lang={languageForPath(path ?? item.summary)} />
          : detail && <DetailBody detail={detail} />}
        {/* 编辑成功的回执只是一句「已更新」，diff 已经说明了一切，不再重复。 */}
        {!(kind === "edit" && result && !result.is_error) && (
          result ? <OutputBlock text={stripAnsi(result.text) || t.chat.toolNoOutput} /> : <pre>{t.chat.toolRunning}</pre>
        )}
      </div>
    </details>
  );
});

/// 活动组容器：挂载时组里还有没回执的调用（= 正在跑）就默认展开，用户当场看到正在做
/// 什么；跑完后保持用户留下的开合状态。open 只在挂载时定一次（理由见 ToolActivity）。
export function ActivityGroup({ defaultOpen, className, children }: { defaultOpen: boolean; className: string; children: ReactNode }) {
  const [initialOpen] = useState(() => defaultOpen);
  return <details className={className} open={initialOpen || undefined}>{children}</details>;
}
