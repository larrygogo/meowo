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
  /** 由续上它的调用代表的子代理（最初的委派不在已载入窗口里）：名称优先换成侧车元数据里最初委派的说明。 */
  resumed?: boolean;
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
  const { outcomes, finishedTs, runningSince, results } = collectSubagentReceipts(items);
  const bash: BackgroundTask[] = [];
  const subagents: BackgroundTask[] = [];
  const turnStart = liveTurnStart(items);
  const rowIds = new Set<string>();
  // 一行的状态与时长：结局归属 owner（委派那条调用，或续上时代表它的那条），按本轮计时。
  const stateOf = (owner: string, index: number): Pick<BackgroundTask, "state" | "durationMs" | "sortKey" | "startedAt"> & { outcome: ReturnType<typeof outcomes.get> } => {
    const outcome = outcomes.get(owner);
    const state: BackgroundTask["state"] = !outcome
      ? (live.activity !== "idle" && index > turnStart ? "run" : "stop")
      : outcome.running > 0 ? (live.alive ? "run" : "stop")
      // 被叫停的子任务（codex interrupt_agent）：没跑完，也不是失败。
      : outcome.interrupted ? "stop"
      : outcome.failed > 0 ? "fail" : "done";
    const item = items[index];
    // 本轮开跑时刻：最近一次「开跑」回执（启动或续上）的时间，没有就退回调用时刻。
    const started = Date.parse(runningSince.get(owner) ?? item.timestamp ?? "");
    const ended = Date.parse(finishedTs.get(owner) ?? "");
    const durationMs = state !== "run" && Number.isFinite(started) && Number.isFinite(ended) && ended >= started ? ended - started : null;
    const sortKey = (state !== "run" ? finishedTs.get(owner) : null) ?? item.timestamp ?? "";
    return { state, durationMs, sortKey, startedAt: Number.isFinite(started) ? started : null, outcome };
  };
  for (const [index, item] of items.entries()) {
    if (item.type !== "tool_use") continue;
    const isShell = isBackgroundShell(item, outcomes);
    if (!isShell && !isSubagentDelegation(item, outcomes)) continue;
    const { outcome, ...row } = stateOf(item.id, index);
    rowIds.add(item.id);
    if (isShell) {
      const command = item.detail?.kind === "command" ? item.detail.command : item.summary;
      const description = item.detail?.kind === "command" ? item.detail.description?.trim() : null;
      bash.push({ id: item.id, name: description || `$ ${command}`, meta: description ? `$ ${command}` : null, ...row });
    } else {
      // 结论已交、后台仍在运行：按完成算（不占「进行中」），行里提示一句（做空第二轮）。
      const backgroundRunning = row.state === "done" && outcome?.background_running === true;
      subagents.push({ id: item.id, name: subagentName(item), meta: null, backgroundRunning, probeId: item.id, ...row });
    }
  }
  // 被 SendMessage 续上的子代理，最初那次委派不在已载入的窗口里（首屏只有末尾 200 条）：结局归到了
  // 窗口里第一条带这个 task_id 的回执上——可能是续上的那条调用，也可能是一条完成通知（它挂在窗口外
  // 某次续上的调用 id 上，那条调用本身不在窗口里）。实拍 2026-11-18：两种形状都有，后一种让第二次
  // 续上的子代理在面板上整个消失。按 task_id 归并成一行（多次续上也只一行，状态取最近一次），由窗口里
  // **最近一次**续上它的调用代表。判据不看工具名（类别只认后端给的 kind），只看回执形状：这条调用的
  // 回执让该 task_id 开跑。子代理再派出的子代理只在它自己的侧车里，主链看不到，不单列。
  const seenTasks = new Set<string>();
  for (const [owner, outcome] of outcomes) {
    const taskId = outcome.task_id;
    if (!taskId || rowIds.has(owner) || seenTasks.has(taskId)) continue;
    let rep = -1;
    for (let index = items.length - 1; index >= 0; index -= 1) {
      const item = items[index];
      if (item.type !== "tool_use" || rowIds.has(item.id)) continue;
      const receipt = results.get(item.id)?.subagent;
      if (receipt?.task_id === taskId && receipt.running > 0) { rep = index; break; }
    }
    if (rep < 0) continue;
    seenTasks.add(taskId);
    const item = items[rep] as ToolUseItem;
    const { outcome: _owned, ...row } = stateOf(owner, rep);
    subagents.push({
      id: item.id, name: resumedName(item), meta: null, probeId: taskId, resumed: true,
      backgroundRunning: row.state === "done" && outcome.background_running === true, ...row,
    });
  }
  return { bash, subagents };
}

function subagentName(item: ToolUseItem): string {
  if (item.subagent?.description?.trim()) return item.subagent.description.trim();
  return argText(item.summary, ["description", "prompt"]) ?? item.summary.trim();
}

/// 续上子代理那条调用的名称：参数里的 summary，其次给它的话的首行（不带 JSON 外壳）。面板拿到侧车
/// 元数据里最初委派的说明后会换成那个（见 TasksPanel 的 descriptions）。
function resumedName(item: ToolUseItem): string {
  return argText(item.summary, ["summary", "description", "message", "content"]) ?? item.summary.trim();
}

/// 从工具调用的参数摘要里取某个字段的文本。摘要是**截断过**的参数 JSON（实拍：
/// `{"content":"CI（workflow_dispatch…","message":"CI（…`），整段 JSON.parse 会失败——逐个字段用正则取
/// 字符串值，取到的转义照 JSON 规则还原，只留首行。都取不到返回 null。
function argText(summary: string, keys: string[]): string | null {
  const raw = summary.trim();
  if (!raw.startsWith("{")) return null;
  for (const key of keys) {
    const match = new RegExp(String.raw`"${key}"\s*:\s*"((?:[^"\\]|\\.)*)`).exec(raw);
    if (!match) continue;
    let text = match[1];
    // 截断可能正好切在一个反斜杠后面：去掉落单的那个再按 JSON 字符串还原。
    try { text = JSON.parse(`"${text.replace(/\\$/, "")}"`) as string; } catch { /* 半截转义：照原文 */ }
    const line = text.split("\n").map((part) => part.trim()).find(Boolean);
    if (line) return line;
  }
  return null;
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
/** 侧车实测：已用 token 与最初委派的说明（见 probe_subagent_states），拿不到的字段缺省。 */
export type TaskProbe = { tokens?: number; description?: string | null };

export const TasksPanel = memo(function TasksPanel({ tasks, filter, now = Date.now(), probes }: {
  tasks: BackgroundTask[];
  filter: TaskFilter;
  /** 运行中的行走表用的「此刻」（面板每秒推一次）。 */
  now?: number;
  /** 各任务的侧车实测，键是 probeId（缺省为 id）。 */
  probes?: Record<string, TaskProbe>;
}) {
  const t = useT();
  const shown = filterTasks(tasks, filter);
  if (shown.length === 0) return <div className="chat-tasks-empty">{t.chat.taskEmpty[filter]}</div>;
  return (
    <ul className="chat-task-list">
      {shown.map((task) => {
        const probe = probes?.[task.probeId ?? task.id];
        const stats = taskStats(task, now, probe?.tokens, t);
        // 由续上它的调用代表的子代理：有最初委派的说明就用它（与终端子代理行同名），否则用续上时那句话。
        const name = (task.resumed && probe?.description?.trim()) || task.name;
        return (
        <li key={task.id} className={"chat-task-row is-" + task.state} data-tip={task.meta ?? name}>
          <span className="chat-task-glyph" role="img" aria-label={t.chat.taskStates[task.state]}>
            {task.state === "run" ? <span className="chat-task-spin" aria-hidden="true" />
              : task.state === "done"
                ? <Icon glyph={icCircleCheck} size={14} />
                // 已中断用待办同款的空圈：它没有完成，也不是失败。
                : task.state === "stop" ? <TodoBadge status="pending" small />
                : <Icon glyph={icClose} size={14} />}
          </span>
          <span className="chat-task-name">{name}</span>
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
/// `probe` 只给子代理面板：向侧车查各子代理用掉的 token 与最初委派的说明（拿不到的不出现在结果里）。
export function TasksDockPanel({ title, icon, tasks, onClose, probe }: {
  title: string;
  icon: JSX.Element;
  tasks: BackgroundTask[];
  onClose: () => void;
  probe?: (ids: string[]) => Promise<Record<string, TaskProbe>>;
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
  const [probes, setProbes] = useState<Record<string, TaskProbe>>({});
  useEffect(() => {
    if (!probe) return;
    const probeIds = probeKey.split("|").filter(Boolean).map((key) => key.slice(0, key.lastIndexOf(":")));
    if (probeIds.length === 0) return;
    let alive = true;
    const load = () => {
      probe(probeIds).then((next) => { if (alive) setProbes(next); }).catch(() => { /* 查不到就不显示数字 */ });
    };
    load();
    const timer = running > 0 ? window.setInterval(load, TOKEN_POLL_MS) : undefined;
    return () => { alive = false; if (timer !== undefined) window.clearInterval(timer); };
  }, [probe, probeKey, running]);
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
      <div className="dock-work-body" id={ids.panelId} role="tabpanel" aria-labelledby={ids.tabId(filter)}><TasksPanel tasks={tasks} filter={filter} now={now} probes={probes} /></div>
    </div>
  );
}
