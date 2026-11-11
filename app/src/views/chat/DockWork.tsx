/// 停靠栏（对照 Kimi ChatDock 的 workbar + 向上弹出的面板）：计划、当前进度、后台 Bash、子代理。
///
/// 为什么单独成模块：这一块只读 transcript 与几项会话元信息，却把开合状态、关闭规则、入口
/// 显隐判定、四块面板、连同只有它用的任务列表重建，全摊在 ChatWindow 里。收拢后 ChatWindow
/// 只交数据，入口何时出现（dockEntries）是可单测的纯函数。

import { useEffect, useMemo, useRef, useState } from "react";
import type { ChatItem, ChatHistory } from "../../api";
import { buildTranscriptTodos, type TaskUpdateCache } from "../../chat/transcriptTodos";
import { pushEscLayer } from "../../escLayers";
import { useT } from "../../i18n";
import { latestPlan, PlanPanel } from "./PlanPanel";
import { collectBackgroundTasks, TasksDockPanel } from "./TasksPanel";
import { TodoListItems, type TodoRow } from "./TodoPanel";
import { LeadGateCard, LeadPanel, LeadPill, useLeadTask } from "./LeadPanel";
import { Icon } from "../../icons/Icon";
import { icList, icSparkles, icTerminalBox } from "../../icons/glyphs.chat";
import { icPencil } from "../../icons/glyphs";

type Panel = "plan" | "todos" | "tasks" | "subagents" | "lead";

/// 入口按需出现（对照 Kimi 的 ChatDock）：计划只在有计划、开着计划模式或正等计划审批时出；
/// 后台 Bash/子代理只在确实派过时出——常驻的空入口点开只有「暂无」，是噪音。
export function dockEntries({ items, plan, planMode, planPending }: {
  items: ChatItem[];
  plan: string | null | undefined;
  planMode: boolean;
  planPending: boolean;
}) {
  const backgroundTasks = collectBackgroundTasks(items);
  const currentPlan = latestPlan(items, plan ?? undefined);
  return {
    backgroundTasks,
    currentPlan,
    showPlan: currentPlan !== null || planMode || planPending,
    bashRunning: backgroundTasks.bash.filter((task) => task.state === "run").length,
    subagentsRunning: backgroundTasks.subagents.filter((task) => task.state === "run").length,
  };
}

function TerminalPillIcon({ className }: { className: string }) {
  return (
    <Icon glyph={icTerminalBox} size={16} className={className} />
  );
}

function SparklesPillIcon({ className }: { className: string }) {
  return (
    <Icon glyph={icSparkles} size={16} className={className} />
  );
}

function TodosIcon({ className }: { className: string }) {
  return <Icon glyph={icList} size={16} className={className} />;
}

export function DockWork({ sessionId, cwd, ccSessionId, items, history, loading }: {
  sessionId: number;
  /// 会话工作目录：Lead 入口据此读 `.lead/active`（为空时不显示 Lead）。
  cwd?: string | null;
  /// 会话的 cc_session_id：Lead 任务只给认领它的会话看。
  ccSessionId?: string | null;
  items: ChatItem[];
  history: Pick<ChatHistory, "plan" | "agentModes" | "pendingReview" | "todos"> | null;
  loading: boolean;
}) {
  const t = useT();
  const [panel, setPanel] = useState<Panel | null>(null);
  const toggle = (next: Panel) => setPanel((current) => (current === next ? null : next));
  const [leadTask, refreshLead] = useLeadTask(cwd, ccSessionId);

  // claude 新版任务列表(TaskCreate/TaskUpdate)从时间线累积重建:CC 对这类 harness
  // 内部工具**不触发 PostToolUse hook**(实测:reporter 手喂事件能落库,真调用不触发),
  // hook 快照/增量两条路都收不到,transcript 是唯一来源。编号在 TaskCreate 回执文本
  // `Task #N created successfully` 里;TaskUpdate 的摘要是 Rust 侧精简出的 JSON。
  // 「当前回合」分界:最后一条用户消息的位置。任务/子任务列表跨回合只增不减,长会话
  // 会把历史旧账全堆进面板(用户反馈)——已完成项只显示**这条分界之后**完成的;
  // 未完成的恒显示,无论多老,它们都是还没做完的活计划。
  const lastUserIdx = useMemo(
    () => items.reduce((acc, item, index) => (item.type === "user_text" ? index : acc), -1),
    [items],
  );
  // TaskUpdate 摘要的解析缓存。这条扫描是本页唯一昂贵的一处:实测 30000 条(三成是顶到
  // 800 字上限的 TaskUpdate)时,带 parse 的一遍要 5.5~6.4 ms,而同批数据上其余派生
  // 全在 1 ms 以下;流式 200ms 一批,6 ms 一次就吃掉小半帧。条目不可变(reducer 写时复制)、
  // 同 id 的 summary 永不变,按 id 缓存后降到 0.7~1.6 ms。换会话整只丢弃:id 不跨会话复用,
  // 也顺带封住无界增长。
  const taskUpdateCacheRef = useRef<TaskUpdateCache>(new Map());
  useEffect(() => { taskUpdateCacheRef.current = new Map(); }, [sessionId]);
  const transcriptTodos = useMemo(
    () => buildTranscriptTodos(items, lastUserIdx, taskUpdateCacheRef.current),
    [items, lastUserIdx],
  );
  // DB 快照(旧版 TodoWrite/kimi 的 TodoList,hook 路线仍通)优先;空则用 transcript 重建的
  // (claude 新版唯一来源)。
  const todos: TodoRow[] = history?.todos?.length ? history.todos : transcriptTodos;

  const planMode = (history?.agentModes ?? []).some((mode) => mode.value === "plan");
  const planPending = history?.pendingReview === "plan";
  const { backgroundTasks, currentPlan, showPlan, bashRunning, subagentsRunning } = useMemo(
    () => dockEntries({ items, plan: history?.plan, planMode, planPending }),
    [items, history?.plan, planMode, planPending],
  );

  // 开着的面板：Esc 关（走层栈，审批卡的 Esc 不抢）；点面板外关（对照 Kimi 的 ChatDock）。
  // 入口胶囊本身除外：它的 click 自己负责开合，若这里先在 pointerdown 关掉，随后的 click
  // 读到「已关」又会把面板重新打开。
  useEffect(() => {
    if (!panel) return;
    const popLayer = pushEscLayer();
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      setPanel(null);
    };
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Element && target.closest(".dock-work-panel, .dock-workbar .ui-pill")) return;
      setPanel(null);
    };
    window.addEventListener("keydown", onKey, true);
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      popLayer();
      window.removeEventListener("keydown", onKey, true);
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [panel]);

  const done = todos.filter((todo) => todo.status === "completed").length;
  const close = () => setPanel(null);
  return (
    <>
      {panel === "lead" && leadTask && <LeadPanel task={leadTask} onClose={close} />}
      {/* 关卡在等 leader 时常驻显示（不必先点开 Lead 面板）：这是 leader 要做的决定。 */}
      {cwd && leadTask?.gate?.status === "pending" && (
        <LeadGateCard key={leadTask.gate.id} cwd={cwd} gate={leadTask.gate} onAnswered={refreshLead} />
      )}
      {panel === "plan" && showPlan && <PlanPanel plan={currentPlan} pending={planPending} onClose={close} />}
      {panel === "todos" && todos.length > 0 && (
        <div className="dock-work-panel panel-todos">
          <div className="dock-work-head">
            <span className="dock-work-title"><TodosIcon className="dock-work-title-icon" />{t.chat.todoStripProgress(done, todos.length)}</span>
          </div>
          <div className="dock-work-body"><TodoListItems todos={todos} /></div>
        </div>
      )}
      {panel === "tasks" && backgroundTasks.bash.length > 0 && (
        <TasksDockPanel key="bash" title={t.chat.tasksBackground} icon={<TerminalPillIcon className="dock-work-title-icon" />} tasks={backgroundTasks.bash} onClose={close} />
      )}
      {panel === "subagents" && backgroundTasks.subagents.length > 0 && (
        <TasksDockPanel key="subagents" title={t.chat.tasksSubagents} icon={<SparklesPillIcon className="dock-work-title-icon" />} tasks={backgroundTasks.subagents} onClose={close} />
      )}
      {!loading && (
        <div className="dock-workbar">
          {leadTask && <LeadPill task={leadTask} active={panel === "lead"} onClick={() => toggle("lead")} />}
          {showPlan && (
            <button type="button" className={"ui-pill" + (panel === "plan" ? " is-active" : "")} onClick={() => toggle("plan")}>
              <Icon glyph={icPencil} size={16} className="ui-pill-icon" />
              {t.chat.plan}
            </button>
          )}
          {todos.length > 0 && (
            <button type="button" className={"ui-pill" + (panel === "todos" ? " is-active" : "")} onClick={() => toggle("todos")}>
              <TodosIcon className="ui-pill-icon" />
              {t.chat.todoStripProgress(done, todos.length)}
            </button>
          )}
          {backgroundTasks.bash.length > 0 && (
            <button type="button" className={"ui-pill" + (panel === "tasks" ? " is-active" : "")} onClick={() => toggle("tasks")}>
              <TerminalPillIcon className="ui-pill-icon" />
              {t.chat.tasksBackground}
              {bashRunning > 0 && <span className="ui-pill-meta"><span className="chat-task-spin" aria-hidden="true" />{bashRunning}</span>}
            </button>
          )}
          {backgroundTasks.subagents.length > 0 && (
            <button type="button" className={"ui-pill" + (panel === "subagents" ? " is-active" : "")} onClick={() => toggle("subagents")}>
              <SparklesPillIcon className="ui-pill-icon" />
              {t.chat.tasksSubagents}
              {subagentsRunning > 0 && <span className="ui-pill-meta"><span className="chat-task-spin" aria-hidden="true" />{subagentsRunning}</span>}
            </button>
          )}
        </div>
      )}
    </>
  );
}
