// demo 专用:构造与 LiveSession 字段一一对应的假会话(仅 demo.html 引用,不进生产构建)。
import { LiveSession, DEFAULT_PROVIDER, type ProviderKey } from "../api";

export type Item = LiveSession & { connected: boolean };

let nextId = 1;

export function makeSession(p: {
  title: string;
  project: string;
  status?: "running" | "waiting" | "ended" | "stale";
  activity?: string | null;
  ctx?: number | null;
  agoMin?: number;
  connected?: boolean;
  archived?: boolean;
  todoDone?: number;
  todoTotal?: number;
  preview?: string | null;
  note?: string | null;
  lastAi?: string | null;
  provider?: ProviderKey;
}): Item {
  const id = nextId++;
  // 在调用时读取(而非模块加载时):main.tsx 已冻结 Date.now,保证录制全程时间戳稳定。
  const NOW = Date.now();
  return {
    session: {
      id,
      project_id: id,
      cc_session_id: `demo-${id}`,
      status: p.status ?? "running",
      started_at: NOW - 3_600_000,
      last_event_at: NOW - (p.agoMin ?? 0) * 60_000,
      ended_at: null,
    },
    project_name: p.project,
    task_title: p.title,
    current_activity: p.activity ?? null,
    column: "doing",
    todo_done: p.todoDone ?? 0,
    todo_total: p.todoTotal ?? 0,
    todos: [],
    pid: 1000 + id,
    connected: p.connected ?? true,
    archived: p.archived ?? false,
    archived_at: p.archived ? NOW : null,
    cwd: `C:/dev/${p.project.split("/").pop()}`,
    errored: false,
    error_label: null,
    error_raw: null,
    preview: p.preview ?? null,
    note: p.note ?? null,
    context_pct: p.ctx ?? null,
    context_window: p.ctx != null ? 200_000 : null,
    pending_review: null,
    last_ai_text: p.lastAi ?? null,
    last_user_text: null,
    model: null,
    provider: p.provider ?? DEFAULT_PROVIDER,
  };
}
