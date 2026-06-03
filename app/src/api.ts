import { invoke } from "@tauri-apps/api/core";

export type Todo = {
  id: number;
  task_id: number;
  content: string;
  status: "pending" | "in_progress" | "completed";
  order_idx: number;
};

export type Task = {
  id: number;
  project_id: number;
  session_id: number | null;
  title: string;
  column: "todo" | "doing" | "done";
  column_locked: boolean;
  current_activity: string | null;
  created_at: number;
  updated_at: number;
};

export type Project = {
  id: number;
  root_path: string;
  name: string;
  created_at: number;
  updated_at: number;
};

export type ProjectOverview = {
  project: Project;
  active_sessions: number;
  todo_count: number;
  doing_count: number;
  done_count: number;
  last_activity_at: number;
};

export type TaskCard = {
  task: Task;
  todos: Todo[];
  session_status: string | null;
};

export function getOverview(): Promise<ProjectOverview[]> {
  return invoke("get_overview");
}

export function getProjectTasks(projectId: number): Promise<TaskCard[]> {
  // JS 传 projectId，Tauri 自动转成 Rust 命令的 project_id 参数。
  return invoke("get_project_tasks", { projectId });
}

export type Session = {
  id: number;
  project_id: number;
  cc_session_id: string;
  status: "running" | "waiting" | "ended" | "stale";
  started_at: number;
  last_event_at: number;
  ended_at: number | null;
};

export type LiveSession = {
  session: Session;
  project_name: string;
  task_title: string;
  current_activity: string | null;
  column: "todo" | "doing" | "done";
  todo_done: number;
  todo_total: number;
  todos: Todo[];
};

export function getLiveSessions(): Promise<LiveSession[]> {
  return invoke("get_live_sessions");
}

// 纯函数：根据 todo 列表算完成度。
export function todoProgress(todos: Todo[]): { done: number; total: number; percent: number } {
  const total = todos.length;
  const done = todos.filter((t) => t.status === "completed").length;
  const percent = total === 0 ? 0 : Math.round((done / total) * 100);
  return { done, total, percent };
}
