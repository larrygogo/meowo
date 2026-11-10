import { memo, useContext, useEffect, useRef, type ReactNode } from "react";
import { useT } from "../../i18n";
import type { Dict } from "../../i18n/zh";
import { fmtSpan, type ToolResultItem, type ToolUseItem } from "./shared";
import { toolKind, type ToolKind } from "./ToolIcon";
import { ChatCwdContext, toolSummary } from "./ToolActivity";
import type { ChatItem } from "../../api";
import { useRememberedOpen } from "./foldMemory";

/// 连续活动的汇总块（对照 Kimi 的 ActivityRun）：两段正文之间 ≥2 条思考/工具收成一行摘要。
/// 运行中展开、摘要写「正在做什么 · 已完成的 · 计时」；跑完自动收起，摘要改成完成态。
/// 开合记忆按块 id 存进当前会话的那份（foldMemory），流式重渲染、切会话再回来都不丢用户意图。

type ProcessItem = Extract<ChatItem, { type: "reasoning" | "reasoning_delta" | "tool_use" | "tool_result" }>;

/// Kimi 的 subject 上限：参数再长就截断，摘要行只要认得出是哪个文件/哪个模式。
const SUBJECT_MAX = 80;

function clip(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > SUBJECT_MAX ? `${flat.slice(0, SUBJECT_MAX)}…` : flat;
}

/// 「正在做什么」：正在流的思考 → 思考中；否则最后一个没回执的工具按类型写动作 + 对象。
/// 工具都回来了（在等模型下一步）返回 null，摘要只剩已完成部分与计时。
function currentClause(items: ProcessItem[], results: Map<string, ToolResultItem>, cwd: string | null, t: Dict): { text: string; toolId: string | null } | null {
  const last = items[items.length - 1];
  if (last?.type === "reasoning_delta") return { text: t.chat.reasoningLive, toolId: null };
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (item.type !== "tool_use" || results.has(item.id)) continue;
    const kind = toolKind(item.name);
    const doing = t.chat.turnToolDoing;
    if (kind === "run") return { text: doing.run, toolId: item.id };
    const subject = clip(toolSummary(item, cwd).replace(/\/ /g, "/"));
    const verb = kind === "read" || kind === "edit" || kind === "write" || kind === "search" || kind === "web" ? doing[kind] : null;
    return { text: verb && subject ? verb(subject) : doing.other, toolId: item.id };
  }
  return null;
}

/// 已完成部分：按类型首次出现的顺序每类一条，有失败的附「（N 失败）」。思考不计数（Kimi 同）。
/// countUnresolved：段已结束时，没等到回执的调用也照常计数（有的工具 CLI 不落回执，
/// 不计的话摘要只剩一个时长，像是什么都没做）。运行中则只数已回来的。
function doneClauses(tools: ToolUseItem[], results: Map<string, ToolResultItem>, skip: string | null, t: Dict, countUnresolved = false): string[] {
  const counts = new Map<ToolKind, { done: number; failed: number }>();
  for (const tool of tools) {
    if (tool.id === skip) continue;
    const result = results.get(tool.id);
    if (!result && !countUnresolved) continue;
    const kind = toolKind(tool.name);
    const entry = counts.get(kind) ?? { done: 0, failed: 0 };
    entry.done += 1;
    if (result?.is_error) entry.failed += 1;
    counts.set(kind, entry);
  }
  return [...counts.entries()].map(([kind, { done, failed }]) =>
    t.chat.turnToolSummaries[kind](done) + (failed > 0 ? t.chat.turnToolFailed(failed) : ""));
}

/// 回合级折叠（设置「消息自动折叠」，对照 Kimi 的 TurnFold）：一整段回复跑完后，最终回答
/// 之前的全部工作过程（思考、工具、活动汇总、中途的话）收成「工作了 2m13s」一行。
/// 只在回合结束后由 Transcript 套上；运行中不套，过程照常平铺。默认收起，开合按回复记忆。
export function ReplyFold({ id, durationMs, children }: { id: string; durationMs: number | null; children: ReactNode }) {
  const t = useT();
  const [open, setOpen] = useRememberedOpen(`reply:${id}`, false);
  const summary = durationMs !== null && durationMs >= 1000 ? t.chat.replyWorked(fmtSpan(durationMs)) : t.chat.turnWorked;
  return (
    <div className={"turn-fold is-reply" + (open ? " is-open" : "")}>
      <button type="button" className="turn-fold-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <span className="turn-fold-summary">{summary}</span>
        <span className="chat-tool-chevron">›</span>
      </button>
      {open && <div className="turn-fold-body">{children}</div>}
    </div>
  );
}

export const TurnFold = memo(function TurnFold({ id, items, durationMs, running = false, children }: {
  id: string;
  items: ProcessItem[];
  durationMs: number | null;
  /** 这一段还在跑（会话在工作、且后面还没有正文）。 */
  running?: boolean;
  children: ReactNode;
}) {
  const t = useT();
  const cwd = useContext(ChatCwdContext);
  const [open, setOpen] = useRememberedOpen(`fold:${id}`, running);
  const wasRunning = useRef(running);
  // Kimi 的开合规则：进入运行展开，跑完自动收起（用户中途手动收/开不影响下一次转换）。
  useEffect(() => {
    if (running !== wasRunning.current) setOpen(running);
    wasRunning.current = running;
  // eslint-disable-next-line react-hooks/exhaustive-deps -- 只在运行态翻转时同步开合；setOpen 来自 useRememberedOpen，变化不该触发
  }, [running]);

  const tools = items.filter((item): item is ToolUseItem => item.type === "tool_use");
  const results = new Map<string, ToolResultItem>();
  for (const item of items) {
    if (item.type === "tool_result" && item.tool_use_id) results.set(item.tool_use_id, item);
  }
  let lead: string | null = null;
  let rest: string[];
  let time: string | null = null;
  if (running) {
    const current = currentClause(items, results, cwd, t);
    lead = current?.text ?? null;
    // 运行中不计时：常驻运行条已经在走表，同一件事只在一处跳（也省掉每秒重渲染整段对话）。
    rest = doneClauses(tools, results, current?.toolId ?? null, t);
  } else {
    rest = doneClauses(tools, results, null, t, true);
    if (durationMs !== null && durationMs >= 1000) time = fmtSpan(durationMs);
  }
  // 整段只有思考没有工具时，完成态没有可数的动作，用「思考过程」起头，免得摘要只剩一个时长。
  if (!running && rest.length === 0 && tools.length === 0) rest = [t.chat.reasoning];
  const clauses = [...(lead ? [lead] : []), ...rest];

  return (
    <div className={"turn-fold" + (open ? " is-open" : "") + (running ? " is-running" : "")}>
      <button type="button" className="turn-fold-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <span className="turn-fold-summary">
          {clauses.length === 0 && !time && t.chat.turnWorked}
          {/* 运行中：当前动作是正文色，已完成部分与计时弱一档（Kimi 的 faint 子句）。 */}
          {lead && <span className="turn-fold-lead">{lead}</span>}
          {(lead ? rest : clauses).map((clause, index) => (
            <span key={index} className={running ? "turn-fold-faint" : undefined}>{(index > 0 || lead) ? " · " : ""}{clause}</span>
          ))}
          {time && <span className="turn-fold-faint">{clauses.length > 0 ? " · " : ""}{time}</span>}
        </span>
        <span className="chat-tool-chevron">›</span>
      </button>
      {open && <div className="turn-fold-body">{children}</div>}
    </div>
  );
});
