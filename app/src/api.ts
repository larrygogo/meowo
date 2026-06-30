import { invoke } from "@tauri-apps/api/core";

/**
 * agent 提供方 key——必须与 Rust 侧 cc_store::ProviderKey 保持一致。
 * 新增 CLI 的同步点共 4 处：本联合类型、providers.tsx 的 PROVIDERS、
 * providers.test.tsx 的 EXPECTED_KEYS、Rust cc_store::ProviderKey::ALL。
 */
export type ProviderKey = "claude" | "kimi" | "codex";
/** 缺省 provider，无法识别时回退；与 Rust 侧 DEFAULT_PROVIDER 一致。 */
export const DEFAULT_PROVIDER: ProviderKey = "claude";

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
  /** 模型展示名（Claude Code statusline 的 model.display_name，如 "Opus"）；无则 null。 */
  model: string | null;
  /** 待审批子态:回合中途等用户介入(批准工具/回答提问/批准计划);无则 null。 */
  pending_review: "approval" | "question" | "plan" | null;
  /** 最近一条 AI 正文(锚 Stop hook);无则 null,卡片回退 preview。 */
  last_ai_text: string | null;
  /** 最近一条用户消息(锚 UserPromptSubmit);独立字段,不被工具活动覆盖。 */
  last_user_text: string | null;
  /** agent 提供方：claude（默认）/ kimi / codex，决定卡片图标与标签。 */
  provider: ProviderKey;
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
  /** 是否在卡片显示对话预览（你的提问 + AI 回复两行）。缺省开启。 */
  preview_enabled: boolean;
  /** 贴纸风格：elevated = 立体感（默认）/ flat = 扁平。 */
  sticker_style: StickerStyle;
  /** 贴纸底色预设 key（classic/slate/moss/plum/rose/amber）。 */
  sticker_color: string;
};

export type ResumeTerminal = "terminal" | "iterm" | "wt" | "powershell" | "cmd";
export type LangSetting = "auto" | "zh" | "en";
export type TerminalOpenMode = "card" | "button";
export type StickerStyle = "elevated" | "flat";

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

export type UsageKind = "five_hour" | "seven_day" | "opus" | "weekly" | "balance" | "other";
export type UsageLane = {
  kind: UsageKind;
  used_pct: number | null;
  used: number | null;
  limit: number | null;
  unit: string | null;
  resets_at: string | null;
};
export type ProviderUsage = { lanes: UsageLane[]; note: string | null };
export type Account = {
  email: string | null;
  display_name: string | null;
  organization: string | null;
  plan: string | null;
  login_label: string | null;
};
export type ProviderAccountPayload = {
  provider: string;
  account: Account | null;
  usage: ProviderUsage | null;
  usage_supported: boolean;
};

export function getAccounts(): Promise<ProviderAccountPayload[]> {
  return invoke("get_accounts");
}
export function refreshUsage(provider: string): Promise<ProviderUsage> {
  return invoke("refresh_usage", { provider });
}
