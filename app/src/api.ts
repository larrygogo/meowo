import { invoke } from "@tauri-apps/api/core";

/**
 * agent 身份串（`"claude"` / `"kimi"` / …），与 Rust 侧 `meowo_agent::AgentId` 同值。
 *
 * 刻意**不是**联合类型：agent 名单由后端的 `list_agents()` 下发，前端不再维护自己的一份
 * （此前加一个 CLI 要同步 4 处：这个联合、PROVIDERS 表、测试里的 EXPECTED_KEYS、Rust 的枚举）。
 * DB 里还可能存着本版本尚不认识的 id，联合类型会对它撒谎。
 */
export type AgentId = string;

/** 后端下发的一个 agent。前端认识 agent 的唯一途径。 */
export type AgentDescriptor = {
  id: AgentId;
  /** 产品名，不翻译。 */
  display_name: string;
  /** 可执行是否装在本机。 */
  installed: boolean;
};

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
  /** agent 身份（如 "claude"）。DB 里可能存着本版本不认识的 id，故不是联合类型。 */
  provider: AgentId;
};

export type LiveSessionCounts = {
  total: number;
  running: number;
  waiting: number;
  archived: number;
};

export function getLiveSessionsCounts(): Promise<LiveSessionCounts> {
  return invoke("get_live_sessions_counts");
}

export type StickerFilter = "all" | "running" | "waiting" | "archived";

export function getLiveSessionsPage(
  filter: StickerFilter,
  search: string | null,
  cursor: { last_event_at: number; id: number } | null,
  limit: number
): Promise<LiveSession[]> {
  return invoke("get_live_sessions_page", {
    filter,
    search: search && search.trim() ? search : null,
    before_last_event_at: cursor?.last_event_at ?? null,
    before_id: cursor?.id ?? null,
    limit,
  });
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
  /** 卡片菜单触发方式：context = 右键菜单（默认）/ button = 卡片菜单按钮（触屏友好），二选一。 */
  card_menu_mode: CardMenuMode;
  /** 是否在卡片显示对话预览（你的提问 + AI 回复两行）。缺省开启。 */
  preview_enabled: boolean;
  /** 贴纸风格：elevated = 立体感（默认）/ flat = 扁平。 */
  sticker_style: StickerStyle;
  /** 贴纸底色预设 key（neutral/classic/slate/moss/plum/rose/amber）。 */
  sticker_color: string;
  /** 在贴纸底栏显示配额的 agent id 列表（后端给默认值）。 */
  sticker_quota_providers: AgentId[];
  /** 「新建会话」面板默认选中的 agent（后端给默认值）。 */
  default_agent: AgentId;
};

export type ResumeTerminal = "terminal" | "iterm" | "wt" | "wezterm" | "powershell" | "cmd";
export type LangSetting = "auto" | "zh" | "en";
export type TerminalOpenMode = "card" | "button";
export type CardMenuMode = "context" | "button";
export type StickerStyle = "elevated" | "flat";

/** 本机实际可用的「打开未连接会话」终端 key（供设置页过滤下拉项）。 */
export function availableTerminals(): Promise<ResumeTerminal[]> {
  return invoke("available_terminals");
}

/** 全部已注册 agent 及其本机安装状态。展示名、安装态都来自这里，前端不再硬编码 agent 名单。 */
export function listAgents(): Promise<AgentDescriptor[]> {
  return invoke("list_agents");
}

/**
 * 按 id 取展示名。未知 id（DB 里存着本版本不认识的 agent，或 list_agents 尚未 resolve）
 * 回退为 id 本身——显示 `"gemini"` 好过显示 `"Claude Code"`。
 */
export function agentName(agents: AgentDescriptor[], id: AgentId): string {
  return agents.find((a) => a.id === id)?.display_name ?? id;
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

/** 某 provider 的 meowo-reporter hooks 接入状态。unknown = 无法确认（读取失败/位置未知）。 */
export type HooksStatus = "installed" | "missing" | "unknown";

/** 新建一个全新会话：在 cwd 打开终端裸启动该 provider。terminal 省略则用设置里的默认终端。 */
export function newSession(cwd: string, provider: AgentId, terminal?: string): Promise<void> {
  return invoke("new_session", { cwd, provider, terminal });
}

/** 最近使用过的工作目录（新建面板快捷选择）。 */
export function recentCwds(limit: number): Promise<string[]> {
  return invoke("recent_cwds", { limit });
}

/** 检测某 provider 的 meowo-reporter hooks 是否已接入（决定新建后会不会入库）。 */
export function checkProviderHooks(provider: AgentId): Promise<HooksStatus> {
  return invoke("check_provider_hooks", { provider });
}

/** 修复接线失败的原因（后端 setup::RepairReason），null = 成功/已是目标状态。 */
export type RepairReason =
  | "not-detected"
  | "need-login"
  | "reporter-not-found"
  | "config-unreadable"
  | "write-failed";

export type RepairResult = { status: HooksStatus; reason: RepairReason | null };

/** 手动修复某 provider 的 hooks：立即执行一次 setup::apply_provider，返回最新状态与失败原因。 */
export function repairProviderHooks(provider: AgentId): Promise<RepairResult> {
  return invoke("repair_provider_hooks", { provider });
}

/** 一键安装某 agent（在终端跑官方安装脚本）。装完在窗口重新聚焦/手动刷新时重检安装状态。 */
export function installAgent(provider: AgentId): Promise<void> {
  return invoke("install_agent", { provider });
}

/** 后台安装结束事件 payload（对应后端 install-done）。logPath 为安装脚本输出的落盘处（可能为 null）。 */
export type InstallDone = { provider: AgentId; ok: boolean; code: number | null; logPath: string | null };

/**
 * 该 agent 装好了、但它的 bin 目录不在持久 PATH 上 → 返回该目录；无需处理时 null。
 *
 * 存在的理由：官方安装器不保证写 PATH（claude 在 Windows 上只打印一行提示就 exit 0），
 * 而 meowo 启动 agent 走绝对路径、察觉不到，用户要到手敲 `claude` 才发现打不开。
 * 非 Windows 恒为 null——unix 的 PATH 由 shell profile 决定，不代用户改。
 */
export function agentPathGap(provider: AgentId): Promise<string | null> {
  return invoke("agent_path_gap", { provider });
}

/** 把该 agent 的 bin 目录写进用户级 PATH（幂等）。已开的终端需重开才能看到。 */
export function addAgentToUserPath(provider: AgentId): Promise<void> {
  return invoke("add_agent_to_user_path", { provider });
}

/**
 * 在终端里拉起该 agent 的交互式登录（claude 是 `auth login`，codex/kimi 是 `login`）。
 * 登录走浏览器 OAuth、终端是 detach 的，拿不到退出码——后端改为轮询账号解析结果，
 * 完成或超时（5 分钟）后 emit `login-done`。terminal 省略则用设置里的默认终端。
 */
export function loginAgent(provider: AgentId, terminal?: string): Promise<void> {
  return invoke("login_agent", { provider, terminal });
}

/** 登录结束事件 payload（对应后端 login-done）。ok=false 表示等待超时，非登录失败。 */
export type LoginDone = { provider: AgentId; ok: boolean };

/** 该 provider 是否已登录：账号能解析出来就算登录（三家判据各异，已在后端 account() 内收敛）。 */
export function isLoggedIn(payload: ProviderAccountPayload | undefined): boolean {
  return !!payload?.account;
}
