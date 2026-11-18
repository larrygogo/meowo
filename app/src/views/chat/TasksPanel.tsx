import { memo, useEffect, useId, useMemo, useState } from "react";
import { tablistIds, useTablist } from "../useTablist";
import { useT } from "../../i18n";
import { collectSubagentReceipts } from "../../chat/reducer";
import { durationText, isBackgroundShell, isSubagentDelegation, type ToolUseItem } from "./shared";
import type { ChatItem } from "../../api";
import { Icon } from "../../icons/Icon";
import { icCircleCheck, icClose } from "../../icons/glyphs";
import { TodoBadge } from "./TodoBadge";
import { ASSUME_LIVE, liveTurnStart, type SessionLive } from "./toolState";
import { fmtTokens } from "./cliStatus";

export type BackgroundTask = {
  id: string;
  /** 任务说明（Bash 的 description）；没有时由命令顶上。 */
  name: string;
  /** 次要信息：后台 Bash 是「$ 命令」，子代理没有。 */
  meta: string | null;
  /** stop = 已中断：没等到结局，也不会再有（所在回合被打断、会话进程没了）。 */
  state: "run" | "done" | "fail" | "stop";
  /** 这一轮开跑的时刻（毫秒）：运行中的行据此实时走表。续上的子代理从续上那一刻算。 */
  startedAt?: number | null;
  /** 向侧车查用量时用的 id：一般是委派那条调用的 id；最初的委派不在已载入窗口里、由
   *  SendMessage 代表的子代理，用它的 agentId（侧车文件按它命名）。 */
  probeId?: string;
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
  const { outcomes, finishedTs, runningSince } = collectSubagentReceipts(items);
  const bash: BackgroundTask[] = [];
  const subagents: BackgroundTask[] = [];
  const turnStart = liveTurnStart(items);
  for (const [index, item] of items.entries()) {
    if (item.type !== "tool_use") continue;
    const isShell = isBackgroundShell(item, outcomes);
    // 被 SendMessage 续上的子代理，最初那次委派不在已载入的窗口里（首屏只有末尾一屏）时，续上
    // 回执的 task_id 找不到归属，结局就挂在 SendMessage 自己身上——由它代表这个子代理，否则
    // 终端里在跑的子代理面板上看不见（实拍 2026-11-18）。最初的委派在窗口里时，结局归它，不重复。
    const resumedHere = !isShell && item.name === "SendMessage" && !!outcomes.get(item.id)?.task_id;
    if (!isShell && !resumedHere && !isSubagentDelegation(item, outcomes)) continue;
    const outcome = outcomes.get(item.id);
    const state: BackgroundTask["state"] = !outcome
      ? (live.activity !== "idle" && index > turnStart ? "run" : "stop")
      : outcome.running > 0 ? (live.alive ? "run" : "stop")
      // 被叫停的子任务（codex interrupt_agent）：没跑完，也不是失败。
      : outcome.interrupted ? "stop"
      : outcome.failed > 0 ? "fail" : "done";
    // 本轮开跑时刻：最近一次「开跑」回执（启动或续上）的时间，没有就退回调用时刻。
    const started = Date.parse(runningSince.get(item.id) ?? item.timestamp ?? "");
    const ended = Date.parse(finishedTs.get(item.id) ?? "");
    const durationMs = state !== "run" && Number.isFinite(started) && Number.isFinite(ended) && ended >= started ? ended - started : null;
    const sortKey = (state !== "run" ? finishedTs.get(item.id) : null) ?? item.timestamp ?? "";
    if (isShell) {
      const command = item.detail?.kind === "command" ? item.detail.command : item.summary;
      const description = item.detail?.kind === "command" ? item.detail.description?.trim() : null;
      bash.push({ id: item.id, name: description || `$ ${command}`, meta: description ? `$ ${command}` : null, state, durationMs, sortKey, startedAt: Number.isFinite(started) ? started : null });
    } else {
      // 结论已交、后台仍在运行：按完成算（不占「进行中」），行里提示一句（做空第二轮）。
      const backgroundRunning = state === "done" && outcome?.background_running === true;
      const probeId = resumedHere ? outcome?.task_id ?? item.id : item.id;
      subagents.push({ id: item.id, name: subagentName(item), meta: null, state, durationMs, sortKey, backgroundRunning, startedAt: Number.isFinite(started) ? started : null, probeId });
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
      // SendMessage 的参数没有 description，取给子代理的话（首行，长的交给 CSS 截断）。
      const text = parsed.description ?? parsed.prompt ?? parsed.summary ?? (typeof parsed.message === "string" ? parsed.message.split("\n")[0] : undefined);
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

/// 一行的耗时与用量（对照 Claude Code 终端子代理行「2m 3s · ↓ 387.8k tokens」）：运行中的从本轮
/// 开跑起实时走表，结束的显示这一轮的总时长；token 只在侧车给得出时显示（不估算）。
function taskStats(task: BackgroundTask, now: number, tokens: number | undefined, t: ReturnType<typeof useT>): string | null {
  const ms = task.state === "run" ? (task.startedAt != null ? now - task.startedAt : null) : task.durationMs;
  const parts = [
    ms !== null ? durationText(ms, t) : null,
    tokens !== undefined && tokens > 0 ? t.chat.taskTokens(fmtTokens(tokens)) : null,
  ].filter((part): part is string => !!part);
  return parts.length > 0 ? parts.join(" · ") : null;
}

/// 任务列表（对照 Kimi 的 TasksPane）：状态图标 + 说明 + 「$ 命令」 + 耗时，一行一个任务。
export const TasksPanel = memo(function TasksPanel({ tasks, filter, now = Date.now(), tokens }: {
  tasks: BackgroundTask[];
  filter: TaskFilter;
  /** 运行中的行走表用的「此刻」（面板每秒推一次）。 */
  now?: number;
  /** 各任务已用 token，键是 probeId（缺省为 id）。 */
  tokens?: Record<string, number>;
}) {
  const t = useT();
  const shown = filterTasks(tasks, filter);
  if (shown.length === 0) return <div className="chat-tasks-empty">{t.chat.taskEmpty[filter]}</div>;
  return (
    <ul className="chat-task-list">
      {shown.map((task) => {
        const stats = taskStats(task, now, tokens?.[task.probeId ?? task.id], t);
        return (
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
          {stats && <span className="chat-task-time">{stats}</span>}
        </li>
        );
      })}
    </ul>
  );
});

/// 用量多久问一次侧车：token 不必逐秒刷新，侧车按文件长度与修改时间缓存，没变化是纯内存读。
const TOKEN_POLL_MS = 5000;

/// 面板头 + 筛选 + 列表的整块（后台 Bash 与子代理共用；筛选状态各自独立）。
/// `probeTokens` 只给子代理面板：向侧车查各子代理用掉的 token（拿不到的不出现在结果里）。
export function TasksDockPanel({ title, icon, tasks, onClose, probeTokens }: {
  title: string;
  icon: JSX.Element;
  tasks: BackgroundTask[];
  onClose: () => void;
  probeTokens?: (ids: string[]) => Promise<Record<string, number>>;
}) {
  const t = useT();
  const [filter, setFilter] = useState<TaskFilter>("recent");
  const tabsId = useId();
  const ids = tablistIds(tabsId);
  const running = tasks.filter((task) => task.state === "run").length;
  // 走表：面板开着（本组件挂着）且有运行中的任务才起定时器，否则一个都不起。
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (running === 0) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [running]);
  // 用量：打开时查一次；有运行中的任务时每 5 秒再查（结束的数不会再变）。
  const probeKey = useMemo(() => tasks.map((task) => `${task.probeId ?? task.id}:${task.state}`).join("|"), [tasks]);
  const [tokens, setTokens] = useState<Record<string, number>>({});
  useEffect(() => {
    if (!probeTokens) return;
    const probeIds = probeKey.split("|").filter(Boolean).map((key) => key.slice(0, key.lastIndexOf(":")));
    if (probeIds.length === 0) return;
    let alive = true;
    const load = () => {
      probeTokens(probeIds).then((next) => { if (alive) setTokens(next); }).catch(() => { /* 查不到就不显示数字 */ });
    };
    load();
    const timer = running > 0 ? window.setInterval(load, TOKEN_POLL_MS) : undefined;
    return () => { alive = false; if (timer !== undefined) window.clearInterval(timer); };
  }, [probeTokens, probeKey, running]);
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
      <div className="dock-work-body" id={ids.panelId} role="tabpanel" aria-labelledby={ids.tabId(filter)}><TasksPanel tasks={tasks} filter={filter} now={now} tokens={tokens} /></div>
    </div>
  );
}
