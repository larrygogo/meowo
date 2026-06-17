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
  pid: number | null;
  connected: boolean;
  archived: boolean;
  archived_at: number | null;
  cwd: string | null;
  errored: boolean;
  error_label: string | null;
  error_raw: string | null;
  /** 最近一条 AI 正文的轻推预览（清洗+截断），hover 卡片时速览；无正文回合为 null。 */
  preview: string | null;
  /** 用户给会话挂的便签（手写备忘，存本地 DB）；无便签为 null。 */
  note: string | null;
  /** 上下文已用百分比（来自 Claude Code statusline，准确）；无 statusline 数据为 null。 */
  context_pct: number | null;
  /** 上下文窗口大小（200000 或 1000000）；无 statusline 数据为 null。 */
  context_window: number | null;
};

export function getLiveSessions(): Promise<LiveSession[]> {
  return invoke("get_live_sessions");
}

export type ThemeMode = "dark" | "light" | "system";

export type Settings = {
  /** 归档条目自动隐藏的天数；0 = 永不隐藏。 */
  archive_hide_days: number;
  /** 桌面通知总开关（待交互 + 错误）。 */
  notifications_enabled: boolean;
  /** 外观模式：深色 / 浅色 / 跟随系统。 */
  theme: ThemeMode;
  /** 贴纸背景不透明度（百分比 25–100）。 */
  opacity: number;
  /** 界面密度/字号缩放（百分比，紧凑 90 / 标准 100 / 宽松 112）。 */
  ui_scale: number;
  /** 打开未连接会话用的终端。macOS：terminal/iterm；Windows：wt/powershell/cmd。 */
  resume_terminal: ResumeTerminal;
  /** 界面/通知语言：auto（跟随系统）/ zh / en。 */
  language: LangSetting;
  /** 打开终端方式：card = 点击卡片（默认）/ button = 卡片上单独的打开按钮。 */
  terminal_open_mode: TerminalOpenMode;
  /** 是否显示卡片 hover「轻推」预览（最近一条 AI 正文）。缺省开启。 */
  preview_enabled: boolean;
};

export type ResumeTerminal = "terminal" | "iterm" | "wt" | "powershell" | "cmd";
export type LangSetting = "auto" | "zh" | "en";
export type TerminalOpenMode = "card" | "button";

/** 本机实际可用的「打开未连接会话」终端 key（供设置页过滤下拉项）。 */
export function availableTerminals(): Promise<ResumeTerminal[]> {
  return invoke("available_terminals");
}

export function getSettings(): Promise<Settings> {
  return invoke("get_settings");
}

export function setSettings(settings: Settings): Promise<void> {
  return invoke("set_settings", { settings });
}

// 纯函数：根据 todo 列表算完成度。
export function todoProgress(todos: Todo[]): { done: number; total: number; percent: number } {
  const total = todos.length;
  const done = todos.filter((t) => t.status === "completed").length;
  const percent = total === 0 ? 0 : Math.round((done / total) * 100);
  return { done, total, percent };
}

export type UsageWindow = { utilization: number; resets_at: string };
export type Usage = {
  five_hour: UsageWindow | null;
  seven_day: UsageWindow | null;
  seven_day_opus: UsageWindow | null;
  seven_day_sonnet: UsageWindow | null;
  extra_usage_enabled: boolean;
};
export type Account = {
  email: string;
  display_name: string;
  organization: string | null;
  plan: string | null;
};
export type DailyEntry = { date: string; message_count: number; session_count: number; tokens: number };
export type DailyStats = { days: DailyEntry[]; last_computed_date: string };
export type AccountPayload = { account: Account | null; daily: DailyStats | null; usage: Usage | null };

export function getAccount(): Promise<AccountPayload> {
  return invoke("get_account");
}
export function refreshUsage(): Promise<Usage> {
  return invoke("refresh_usage");
}
