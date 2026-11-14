import { memo, useId, useState } from "react";
import { tablistIds, useTablist } from "../useTablist";
import { useT } from "../../i18n";
import { collectSubagentReceipts } from "../../chat/reducer";
import { durationText, isBackgroundShell, isSubagentDelegation, type ToolUseItem } from "./shared";
import type { ChatItem } from "../../api";
import { Icon } from "../../icons/Icon";
import { icCircleCheck, icClose } from "../../icons/glyphs";
import { TodoBadge } from "./TodoBadge";
import { ASSUME_LIVE, liveTurnStart, type SessionLive } from "./toolState";

export type BackgroundTask = {
  id: string;
  /** 任务说明（Bash 的 description）；没有时由命令顶上。 */
  name: string;
  /** 次要信息：后台 Bash 是「$ 命令」，子代理没有。 */
  meta: string | null;
  /** stop = 已中断：没等到结局，也不会再有（所在回合被打断、会话进程没了）。 */
  state: "run" | "done" | "fail" | "stop";
  /** 子代理交了结论、它派出的后台活儿还在跑：仍按完成算，行里提示「后台仍在运行」。 */
  backgroundRunning?: boolean;
  durationMs: number | null;
  /** 排序用：结束时刻优先，其次开始时刻（Kimi 的「最近」按完成先后排）。 */
  sortKey: string;
};

export type TaskFilter = "recent" | "running" | "done" | "all";

/// 后台 Bash 与子代理的任务清单。状态读委派回执上的结局统计（running/failed），
/// 与对话流里子任务块同一份关联规则（collectSubagentReceipts），两处不会各说各话。
///
/// 「运行中」不再只凭「没有结局」：还没回执的委派，要它所在的回合此刻还在进行才算在跑；
/// 回执说在跑的后台任务，要会话进程还活着才算。否则是已中断——此前这两种都记成运行中，
/// 面板头永远挂着「N 运行中」（走查 A1）。
export function collectBackgroundTasks(items: ChatItem[], live: SessionLive = ASSUME_LIVE): { bash: BackgroundTask[]; subagents: BackgroundTask[] } {
  const { outcomes, finishedTs } = collectSubagentReceipts(items);
  const bash: BackgroundTask[] = [];
  const subagents: BackgroundTask[] = [];
  const turnStart = liveTurnStart(items);
  for (const [index, item] of items.entries()) {
    if (item.type !== "tool_use") continue;
    const isShell = isBackgroundShell(item, outcomes);
    if (!isShell && !isSubagentDelegation(item, outcomes)) continue;
    const outcome = outcomes.get(item.id);
    const state: BackgroundTask["state"] = !outcome
      ? (live.activity !== "idle" && index > turnStart ? "run" : "stop")
      : outcome.running > 0 ? (live.alive ? "run" : "stop")
      : outcome.failed > 0 ? "fail" : "done";
    const started = Date.parse(item.timestamp ?? "");
    const ended = Date.parse(finishedTs.get(item.id) ?? "");
    const durationMs = state !== "run" && Number.isFinite(started) && Number.isFinite(ended) && ended >= started ? ended - started : null;
    const sortKey = (state !== "run" ? finishedTs.get(item.id) : null) ?? item.timestamp ?? "";
    if (isShell) {
      const command = item.detail?.kind === "command" ? item.detail.command : item.summary;
      const description = item.detail?.kind === "command" ? item.detail.description?.trim() : null;
      bash.push({ id: item.id, name: description || `$ ${command}`, meta: description ? `$ ${command}` : null, state, durationMs, sortKey });
    } else {
      // 结论已交、后台仍在运行：按完成算（不占「进行中」），行里提示一句（做空第二轮）。
      const backgroundRunning = state === "done" && outcome?.background_running === true;
      subagents.push({ id: item.id, name: subagentName(item), meta: null, state, durationMs, sortKey, backgroundRunning });
    }
  }
  return { bash, subagents };
}

function subagentName(item: ToolUseItem): string {
  if (item.subagent?.description?.trim()) return item.subagent.description.trim();
  const raw = item.summary.trim();
  if (raw.startsWith("{")) {
    try {
      const parsed = JSON.parse(raw) as Record<string, unknown>;
      const text = parsed.description ?? parsed.prompt;
      if (typeof text === "string" && text.trim()) return text.trim();
    } catch { /* 不是 JSON 就用原文 */ }
  }
  return raw;
}

/// 入口与面板共用同一判据：普通工具与待办不属于「后台 Bash / 子代理」。
export function hasTaskItems(items: ChatItem[]): boolean {
  const { bash, subagents } = collectBackgroundTasks(items);
  return bash.length + subagents.length > 0;
}

/// 筛选口径照 Kimi 的 TasksPane：「最近」= 全部运行中 + 最近结束的 5 条；其余按状态过滤。
export function filterTasks(tasks: BackgroundTask[], filter: TaskFilter): BackgroundTask[] {
  if (filter === "all") return tasks;
  if (filter === "running") return tasks.filter((task) => task.state === "run");
  const finished = tasks.filter((task) => task.state !== "run").sort((a, b) => b.sortKey.localeCompare(a.sortKey));
  if (filter === "done") return finished;
  return [...tasks.filter((task) => task.state === "run"), ...finished.slice(0, 5)];
}

const FILTERS: TaskFilter[] = ["recent", "running", "done", "all"];

/// 面板头右侧的分段筛选（最近/进行中/已完成/全部）。
/// 页签在面板头、列表在面板体，分属两处：idBase 由 TasksDockPanel 生成后两边共用。
export function TaskFilterTabs({ value, onChange, idBase }: { value: TaskFilter; onChange: (next: TaskFilter) => void; idBase?: string }) {
  const t = useT();
  const tabs = useTablist({ keys: FILTERS, active: value, onSelect: onChange, idBase });
  return (
    <div className="chat-task-filter" {...tabs.listProps}>
      {FILTERS.map((filter) => (
        <button key={filter} type="button" {...tabs.tabProps(filter)} className={value === filter ? "is-active" : ""} onClick={() => onChange(filter)}>
          {t.chat.taskFilters[filter]}
        </button>
      ))}
    </div>
  );
}

/// 任务列表（对照 Kimi 的 TasksPane）：状态图标 + 说明 + 「$ 命令」 + 耗时，一行一个任务。
export const TasksPanel = memo(function TasksPanel({ tasks, filter }: { tasks: BackgroundTask[]; filter: TaskFilter }) {
  const t = useT();
  const shown = filterTasks(tasks, filter);
  if (shown.length === 0) return <div className="chat-tasks-empty">{t.chat.taskEmpty[filter]}</div>;
  return (
    <ul className="chat-task-list">
      {shown.map((task) => (
        <li key={task.id} className={"chat-task-row is-" + task.state} data-tip={task.meta ?? task.name}>
          <span className="chat-task-glyph" role="img" aria-label={t.chat.taskStates[task.state]}>
            {task.state === "run" ? <span className="chat-task-spin" aria-hidden="true" />
              : task.state === "done"
                ? <Icon glyph={icCircleCheck} size={14} />
                // 已中断用待办同款的空圈：它没有完成，也不是失败。
                : task.state === "stop" ? <TodoBadge status="pending" small />
                : <Icon glyph={icClose} size={14} />}
          </span>
          <span className="chat-task-name">{task.name}</span>
          {task.meta && <span className="chat-task-meta">{task.meta}</span>}
          {task.backgroundRunning && <span className="chat-task-meta">{t.chat.subagentBackgroundRunning}</span>}
          {task.durationMs !== null && <span className="chat-task-time">{durationText(task.durationMs, t)}</span>}
        </li>
      ))}
    </ul>
  );
});

/// 面板头 + 筛选 + 列表的整块（后台 Bash 与子代理共用；筛选状态各自独立）。
export function TasksDockPanel({ title, icon, tasks, onClose }: { title: string; icon: JSX.Element; tasks: BackgroundTask[]; onClose: () => void }) {
  const t = useT();
  const [filter, setFilter] = useState<TaskFilter>("recent");
  const tabsId = useId();
  const ids = tablistIds(tabsId);
  const running = tasks.filter((task) => task.state === "run").length;
  return (
    <div className="dock-work-panel panel-tasks">
      <div className="dock-work-head">
        <span className="dock-work-title">
          {icon}
          {title}
          <span className="dock-work-meta">{t.chat.tasksRunningCount(running)}</span>
        </span>
        <span className="dock-work-actions">
          <TaskFilterTabs value={filter} onChange={setFilter} idBase={tabsId} />
          <button type="button" className="dock-work-close" aria-label={t.chat.close} onClick={onClose}>×</button>
        </span>
      </div>
      <div className="dock-work-body" id={ids.panelId} role="tabpanel" aria-labelledby={ids.tabId(filter)}><TasksPanel tasks={tasks} filter={filter} /></div>
    </div>
  );
}
