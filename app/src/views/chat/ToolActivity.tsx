import { createContext, memo, useEffect, useState, type ReactNode } from "react";
import { useT } from "../../i18n";
import type { DiffHunk } from "../../generated/contracts/DiffHunk";
import type { ToolDetail } from "../../generated/contracts/ToolDetail";
import { highlightLines, languageForPath, useHighlighter } from "./highlight";
import { BashTool } from "./tools/BashTool";
import { ReadTool } from "./tools/ReadTool";
import { EditTool } from "./tools/EditTool";
import { GenericTool } from "./tools/GenericTool";

export { ToolIcon, toolKindLabel, toolShortName } from "./ToolIcon";
export type { ToolKind } from "./ToolIcon";

type DiffOperation = { kind: " " | "-" | "+"; text: string };
export type TextDiff = { hunks: DiffHunk[]; added: number; removed: number };

/// 拆行：末尾的换行不算一行；空字符串是 0 行，不是 1 个空行。
function splitLines(text: string): string[] {
  if (!text) return [];
  const lines = text.split("\n");
  if (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
  return lines;
}

const DIFF_CONTEXT_LINES = 2;
const MAX_LCS_CELLS = 1_000_000;

/// 逐行 LCS：只把真正增删的行标成 +/-，公共前后缀保留为上下文。
/// 先剥公共前后缀，典型「大段里改一行」只需比较中间几行；整块替换才走全量兜底。
function diffOperations(oldText: string, newText: string): DiffOperation[] {
  const oldLines = splitLines(oldText);
  const newLines = splitLines(newText);
  let prefix = 0;
  while (prefix < oldLines.length && prefix < newLines.length && oldLines[prefix] === newLines[prefix]) prefix += 1;
  let suffix = 0;
  while (suffix < oldLines.length - prefix && suffix < newLines.length - prefix
    && oldLines[oldLines.length - 1 - suffix] === newLines[newLines.length - 1 - suffix]) suffix += 1;
  const oldMiddle = oldLines.slice(prefix, oldLines.length - suffix);
  const newMiddle = newLines.slice(prefix, newLines.length - suffix);
  const before = oldLines.slice(0, prefix).map((text) => ({ kind: " " as const, text }));
  const after = oldLines.slice(oldLines.length - suffix).map((text) => ({ kind: " " as const, text }));
  if ((oldMiddle.length + 1) * (newMiddle.length + 1) > MAX_LCS_CELLS) {
    return [
      ...before,
      ...oldMiddle.map((text) => ({ kind: "-" as const, text })),
      ...newMiddle.map((text) => ({ kind: "+" as const, text })),
      ...after,
    ];
  }
  const width = newMiddle.length + 1;
  const dp = Array.from({ length: oldMiddle.length + 1 }, () => new Uint32Array(width));
  for (let i = 1; i <= oldMiddle.length; i += 1) {
    for (let j = 1; j <= newMiddle.length; j += 1) {
      dp[i][j] = oldMiddle[i - 1] === newMiddle[j - 1]
        ? dp[i - 1][j - 1] + 1
        : Math.max(dp[i - 1][j], dp[i][j - 1]);
    }
  }
  const middle: DiffOperation[] = [];
  let i = oldMiddle.length;
  let j = newMiddle.length;
  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && oldMiddle[i - 1] === newMiddle[j - 1]) {
      middle.push({ kind: " ", text: oldMiddle[i - 1] });
      i -= 1;
      j -= 1;
    } else if (j > 0 && (i === 0 || dp[i][j - 1] >= dp[i - 1][j])) {
      middle.push({ kind: "+", text: newMiddle[j - 1] });
      j -= 1;
    } else {
      middle.push({ kind: "-", text: oldMiddle[i - 1] });
      i -= 1;
    }
  }
  middle.reverse();
  return [...before, ...middle, ...after];
}

/// 从编辑前后文本生成统一 diff 与真实增删计数；计数和预览共用，杜绝两处算法漂移。
export function diffText(oldText: string, newText: string): TextDiff {
  const operations = diffOperations(oldText, newText);
  let added = 0;
  let removed = 0;
  for (const line of operations) {
    if (line.kind === "+") added += 1;
    else if (line.kind === "-") removed += 1;
  }
  const hunks: DiffHunk[] = [];
  let cursor = 0;
  while (cursor < operations.length) {
    while (cursor < operations.length && operations[cursor].kind === " ") cursor += 1;
    if (cursor >= operations.length) break;
    const firstChange = cursor;
    let lastChange = cursor;
    for (let next = cursor + 1; next < operations.length; next += 1) {
      if (operations[next].kind === " ") continue;
      if (next - lastChange > DIFF_CONTEXT_LINES * 2 + 1) break;
      lastChange = next;
    }
    const start = Math.max(0, firstChange - DIFF_CONTEXT_LINES);
    const end = Math.min(operations.length, lastChange + DIFF_CONTEXT_LINES + 1);
    const oldBefore = operations.slice(0, start).filter((line) => line.kind !== "+").length;
    const newBefore = operations.slice(0, start).filter((line) => line.kind !== "-").length;
    hunks.push({
      old_start: oldBefore + 1,
      new_start: newBefore + 1,
      lines: operations.slice(start, end).map((line) => `${line.kind}${line.text}`),
    });
    cursor = lastChange + 1;
  }
  return { hunks, added, removed };
}

/// 一次写入/编辑真实增删了多少行；其余工具不改文件，返回 null。
export function lineDelta(detail: ToolDetail | null | undefined): { added: number; removed: number } | null {
  if (detail?.kind === "write") return { added: detail.lines, removed: 0 };
  if (detail?.kind === "edit") {
    const { added, removed } = diffText(detail.old, detail.new);
    return { added, removed };
  }
  if (detail?.kind === "patch") {
    return detail.files.reduce(
      (sum, file) => ({ added: sum.added + file.added, removed: sum.removed + file.removed }),
      { added: 0, removed: 0 },
    );
  }
  return null;
}

/** 一次修改里落到单个文件上的那部分：路径、diff、增删行数。 */
export type FileEdit = {
  path: string;
  /** 新建 / 修改 / 删除。删除没有 diff 可看，卡片上只有路径。 */
  op: "add" | "update" | "delete";
  patch: DiffHunk[] | null;
  delta: { added: number; removed: number } | null;
  /** diff 超过预览上限，后面的行没收进来（增删行数仍是完整的）。 */
  truncated: boolean;
};

function patchDelta(patch: DiffHunk[]): { added: number; removed: number } {
  return {
    added: patch.reduce((n, hunk) => n + hunk.lines.filter((line) => line[0] === "+").length, 0),
    removed: patch.reduce((n, hunk) => n + hunk.lines.filter((line) => line[0] === "-").length, 0),
  };
}

/// 一次写入/编辑的落地结果。五家的数据形态不同，都在这里归成「逐文件的修改」：
/// - claude / gemini：调用带旧段新段，回执带真实行号的 diff（优先用回执的）；
/// - kimi / opencode：只有旧段新段，按它们现算 diff；
/// - codex：调用就是补丁，一次可以改几个文件，diff 与增删行数随调用下发。
/// 工具行与回合末尾的「文件已修改」卡片共用这一份口径，两处的增删数才对得上。
export function editOutcome(item: import("./shared").ToolUseItem, result?: import("./shared").ToolResultItem) {
  const raw = item.detail ?? null;
  if (raw?.kind === "patch") {
    const files: FileEdit[] = raw.files.map((file) => ({
      path: file.move_to ?? file.path,
      op: file.op === "add" || file.op === "delete" ? file.op : "update",
      patch: file.hunks.length ? file.hunks : null,
      delta: { added: file.added, removed: file.removed },
      truncated: file.truncated,
    }));
    return { detail: null, path: files[0]?.path ?? item.summary, patch: files[0]?.patch ?? null, delta: lineDelta(raw), files };
  }
  const detail = raw && (raw.kind === "edit" || raw.kind === "write") ? raw : null;
  const generated = !result?.patch?.length && detail?.kind === "edit" ? diffText(detail.old, detail.new) : null;
  const patch = result?.patch?.length ? result.patch : generated?.hunks ?? null;
  const delta = generated ? { added: generated.added, removed: generated.removed } : patch ? patchDelta(patch) : lineDelta(detail);
  const path = detail?.path ?? item.summary;
  const files: FileEdit[] = detail
    ? [{ path, op: detail.kind === "write" ? "add" : "update", patch, delta, truncated: detail.truncated }]
    : [];
  return { detail, path, patch, delta, files };
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

/// 工具行的参数摘要（对齐 Kimi 的 BashTool/ReadTool/EditTool）：命令全文、文件路径、
/// 搜索模式，而不是把 JSON 参数原样摊开。没有结构化 detail 时尝试从 JSON 摘要里提取。
export function toolSummary(item: import("./shared").ToolUseItem, cwd: string | null): string {
  const detail = item.detail;
  if (detail?.kind === "command") return detail.command;
  if (detail?.kind === "read" || detail?.kind === "write" || detail?.kind === "edit") {
    const path = relativize(detail.path, cwd).replace(/\\/g, "/");
    const index = path.lastIndexOf("/");
    // Kimi 的目录与文件名分两段显示，中间留一个空格。
    return index >= 0 ? `${path.slice(0, index + 1)} ${path.slice(index + 1)}` : path;
  }
  if (detail?.kind === "search") return [detail.pattern, detail.path && relativize(detail.path, cwd)].filter(Boolean).join(" ");
  if (detail?.kind === "patch") return detail.files.map((file) => relativize(file.move_to ?? file.path, cwd).replace(/\\/g, "/")).join(", ");
  if (detail?.kind === "web") return detail.url ?? detail.query ?? "";
  const raw = item.summary.trim();
  if (raw.startsWith("{")) {
    try {
      const parsed = JSON.parse(raw) as Record<string, unknown>;
      const command = parsed.command ?? parsed.cmd ?? parsed.script;
      const path = parsed.path ?? parsed.file_path ?? parsed.filePath;
      const pattern = parsed.pattern ?? parsed.query ?? parsed.search;
      if (typeof command === "string") return command;
      if (typeof pattern === "string") return [pattern, typeof path === "string" ? relativize(path, cwd) : ""].filter(Boolean).join(" ");
      if (typeof path === "string") return relativize(path, cwd);
    } catch { /* 不是 JSON 就按原文摘要 */ }
  }
  return raw;
}

/// 统一 diff（claude 回执的 structuredPatch）：终端同款「行号 + -/+ + 代码」，增删交错、
/// 带上下文。删行标旧文件行号、其余标新文件行号；多段之间放一行省略号。
export function UnifiedDiff({ hunks, lang }: { hunks: DiffHunk[]; lang: string | null }) {
  // hljs 懒加载：就绪前 highlightLines 给转义纯文本，到位后重渲染上色（见 highlight.ts）。
  useHighlighter(!!lang);
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
                  {/* 起始行号为 0 = 这段 diff 不带行号（codex 的补丁只有上下文定位），行号列留空。 */}
                  <span className="chat-tool-ln" aria-hidden="true">{(mark === "-" ? hunk.old_start : hunk.new_start) > 0 ? number : ""}</span>
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

/// 工具调用按类型分派到独立渲染器（对照 Kimi 的 ToolCall → BashTool/ReadTool/EditTool/GenericTool）。
/// 类别来自后端（`item.kind`）：五家的读、写、改、命令各落到同一个渲染器。
export const ToolActivity = memo(function ToolActivity({ item, result }: { item: import("./shared").ToolUseItem; result?: import("./shared").ToolResultItem }) {
  switch (item.kind) {
    case "run": return <BashTool item={item} result={result} />;
    case "read": return <ReadTool item={item} result={result} />;
    case "write":
    case "edit": return <EditTool item={item} result={result} />;
    default: return <GenericTool item={item} result={result} />;
  }
});

/// 收起后内容多留一瞬间，与 CSS 里 chat-strip-out 的时长同源（时长略短于动画会砍掉尾帧）。
const COLLAPSE_GRACE_MS = 200;

/// 活动组开合的记忆，放在组件之外：Transcript 随流式到达会重建条目，组件实例的寿命比
/// 用户意图短，记在 state 里一重挂载就丢（Kimi 的 TurnFold 同样是把展开态存在 inject
/// 进来的共享 Map 里）。换会话时由 Transcript 清空——旧会话的组 id 不会再出现。
const groupOpenMemory = new Map<string, boolean>();

export function resetActivityGroupMemory(): void {
  groupOpenMemory.clear();
}

/// 活动组容器：首次挂载时组里还有没回执的调用（= 正在跑）就默认展开，用户当场看到正在
/// 做什么；此后一律以记忆为准，用户收过的组不会被下一条结果弹开。
///
/// shown 只决定内容留在不在 DOM 里（收起后多留 200ms 走完出场动画）；expanded 是意图。
/// 两态拆开是照抄 Kimi 的 TurnFold。进场没有用它那套双 requestAnimationFrame 而是用
/// keyframes：双 rAF 依赖「先画收起态那一帧」，重挂载频繁的流式列表里会看到内容闪一下。
export function ActivityGroup({ id, defaultOpen, className, children }: { id: string; defaultOpen: boolean; className: string; children: ReactNode }) {
  const [expanded, setExpanded] = useState(() => groupOpenMemory.get(id) ?? defaultOpen);
  const [shown, setShown] = useState(expanded);

  useEffect(() => {
    groupOpenMemory.set(id, expanded);
    if (expanded) {
      setShown(true);
      return;
    }
    const timer = setTimeout(() => setShown(false), COLLAPSE_GRACE_MS);
    return () => clearTimeout(timer);
  }, [id, expanded]);

  return (
    <details
      className={className}
      open={shown || undefined}
      data-motion={expanded ? undefined : "out"}
      onToggle={(event) => setExpanded(event.currentTarget.open)}
    >
      {children}
    </details>
  );
}
