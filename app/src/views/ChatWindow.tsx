import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { agentChatUi, getChatHistory, getPendingApproval, isExternallyHeld, managedTerminalBinding, managedTerminalSnapshot, registerApprovalConsumer, resolvePendingApproval, startManagedTerminal, unregisterApprovalConsumer, writeManagedTerminal, type ChatHistory, type ChatItem, type ChatUi, type PendingApproval } from "../api";
import { useT } from "../i18n";
import { agentAssets, tintStyle } from "../providers";
import { reduceChatEvents } from "../chat/reducer";
import { ChatMarkdown } from "./ChatMarkdown";
import { ChatSidebar } from "./ChatSidebar";
import { ManagedTerminal } from "./ManagedTerminal";
import { appendTerminalText, terminalAttention as detectTerminalAttention, visibleTerminalText, type TerminalAttention, type TerminalAttentionOption } from "../terminalAttention";

function initialSessionId(): number {
  const value = new URLSearchParams(window.location.search).get("sessionId");
  const id = Number(value);
  return Number.isSafeInteger(id) && id !== 0 ? id : 0;
}

const SIDEBAR_COLLAPSED_KEY = "meowo-chat-sidebar-collapsed";

function approvalSuggestionLabel(suggestion: unknown, index: number, t: ReturnType<typeof useT>): string {
  if (!suggestion || typeof suggestion !== "object" || Array.isArray(suggestion)) {
    return t.chat.allowSuggested(index + 1);
  }
  const entry = suggestion as Record<string, unknown>;
  const destination = entry.destination;
  const base = (() => { switch (destination) {
    case "session": return t.chat.allowSession;
    case "localSettings": return t.chat.allowLocalProject;
    case "projectSettings": return t.chat.allowProject;
    case "userSettings": return t.chat.allowUser;
    default: return t.chat.allowSuggested(index + 1);
  } })();
  const firstRule = Array.isArray(entry.rules) ? entry.rules[0] : null;
  if (!firstRule || typeof firstRule !== "object" || Array.isArray(firstRule)) return base;
  const rule = firstRule as Record<string, unknown>;
  const tool = typeof rule.toolName === "string" ? rule.toolName : "";
  const content = typeof rule.ruleContent === "string" ? rule.ruleContent : "";
  const detail = content || tool;
  if (!detail) return base;
  const short = detail.length > 42 ? detail.slice(0, 41) + "…" : detail;
  return `${base} · ${short}`;
}

function claudeCommandApprovalDetails(text: string) {
  const lines = text.split("\n").map((line) => line.trim()).filter(Boolean);
  const marker = lines.findIndex((line) => /this command requires approval/i.test(line));
  let before = marker >= 0 ? lines.slice(0, marker) : lines;
  // 审批框以工具头（"Bash command"）开头；只取最后一个工具头之后的内容，
  // 避免把上一屏残留的输出并进命令文本。
  const header = before.reduce((found, line, index) => (/^bash command$/i.test(line) ? index : found), -1);
  if (header >= 0) before = before.slice(header + 1);
  return {
    // 长命令会按终端宽度硬换行成多行；除末行（用途说明）外全部并入命令整段显示，
    // 不能按「倒数第二行是命令」取——那只会摘到换行后的最后一个片段。
    command: before.length >= 2 ? before.slice(0, -1).join("\n") : before[0] ?? "",
    description: before.length >= 2 ? before[before.length - 1] : "",
    question: lines.find((line) => /do you want to proceed\?/i.test(line)) ?? "",
  };
}

/** token 数缩写：128000 → "128K"，1000000 → "1M"。 */
function shortTokens(n: number): string {
  if (n >= 1_000_000) {
    const m = n / 1_000_000;
    return (m >= 10 || Number.isInteger(m) ? Math.round(m) : m.toFixed(1)) + "M";
  }
  return Math.round(n / 1000) + "K";
}

/** 上下文用量环形进度条：环内百分比，环右侧「已用/总量」。60%↑黄、85%↑红。 */
function ContextMeter({ pct, window, t }: { pct: number; window: number | null; t: ReturnType<typeof useT> }) {
  const clamped = Math.min(100, Math.max(0, pct));
  const R = 8;
  const C = 2 * Math.PI * R;
  const tone = pct >= 85 ? "is-full" : pct >= 60 ? "is-warn" : "";
  const usage = window ? `${shortTokens(window * pct / 100)}/${shortTokens(window)}` : null;
  return (
    <span className={"chat-context " + tone} title={window ? t.chat.contextTip(pct, Math.round(window / 1000)) : t.chat.contextShort(pct)}>
      <span className="chat-context-ring">
        <svg width="20" height="20" viewBox="0 0 20 20">
          <circle className="chat-context-ring-bg" cx="10" cy="10" r={R} fill="none" strokeWidth="2.5" />
          <circle
            className="chat-context-ring-fg" cx="10" cy="10" r={R} fill="none" strokeWidth="2.5"
            strokeLinecap="round" strokeDasharray={C}
            strokeDashoffset={C * (1 - clamped / 100)} transform="rotate(-90 10 10)"
          />
        </svg>
        <span className="chat-context-pct">{pct}</span>
      </span>
      {usage && <span className="chat-context-usage">{usage}</span>}
    </span>
  );
}

function Message({ item }: { item: ChatItem }) {
  const t = useT();
  if (item.type === "user_text" || item.type === "assistant_text" || item.type === "assistant_delta") {
    const user = item.type === "user_text";
    return (
      <article className={"chat-message " + (user ? "is-user" : "is-assistant")}>
        <div className="chat-role">{user ? t.chat.you : t.chat.assistant}</div>
        {/* 用户消息保持原文（用户不是在写 markdown，行首 # 变大标题只会失真）；
            模型输出按 markdown 渲染。 */}
        {user
          ? <div className="chat-text">{item.text}</div>
          : <div className="chat-text chat-md"><ChatMarkdown text={item.text} /></div>}
      </article>
    );
  }
  if (item.type === "reasoning" || item.type === "reasoning_delta") {
    return (
      <details className="chat-reasoning" open>
        <summary><span className="chat-timeline-dot" />{t.chat.reasoning}</summary>
        <div className="chat-md"><ChatMarkdown text={item.text} /></div>
      </details>
    );
  }
  if (item.type === "tool_use") {
    return (
      <details className="chat-tool">
        <summary>
          <span className="chat-tool-icon"><svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><path d="M4 17l6-6-6-6M12 19h8" /></svg></span>
          <span className="chat-tool-name">{item.name}</span><span className="chat-tool-summary">{item.summary}</span><span className="chat-tool-chevron">›</span>
        </summary>
        <pre>{item.summary}</pre>
      </details>
    );
  }
  if (item.type === "tool_result") {
    return (
      <details className={"chat-tool chat-result" + (item.is_error ? " is-error" : "")}>
        <summary>
          <span className="chat-tool-icon is-file"><svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><path d="M6 3h8l4 4v14H6zM14 3v5h5" /></svg></span>
          <span className="chat-tool-name">{t.chat.toolResult}</span><span className="chat-tool-summary">{item.text}</span><span className="chat-tool-chevron">›</span>
        </summary>
        <pre>{item.text}</pre>
      </details>
    );
  }
  return <div className="chat-meta"><span />{t.chat.compact}<span /></div>;
}

type ToolUseItem = Extract<ChatItem, { type: "tool_use" }>;
type ToolResultItem = Extract<ChatItem, { type: "tool_result" }>;

function friendlyToolName(name: string, t: ReturnType<typeof useT>): string {
  const normalized = name.toLowerCase();
  if (normalized === "bash" || normalized.includes("shell") || normalized.includes("terminal")) return t.chat.runTerminal;
  if (normalized === "read" || normalized.includes("view_image")) return t.chat.readFile;
  if (normalized === "write" || normalized === "edit" || normalized.includes("patch")) return t.chat.editFile;
  return name;
}

/// 按工具类型分图标：搜索 / 读文件 / 改文件 / 终端 / 网络 / 通用。
/// 一组操作若全是同一个 `>_`，用户在展开列表里无法一眼区分做了什么。
function ToolIcon({ name }: { name: string }) {
  const normalized = name.toLowerCase();
  const path = normalized.includes("grep") || normalized.includes("glob") || normalized.includes("search")
    ? "M10.5 3a7.5 7.5 0 1 0 4.55 13.46L20 21.4 21.4 20l-4.94-4.95A7.5 7.5 0 0 0 10.5 3zm0 2a5.5 5.5 0 1 1 0 11 5.5 5.5 0 0 1 0-11z"
    : normalized === "read" || normalized.includes("view_image") || normalized.includes("notebook")
    ? "M6 3h8l4 4v14H6zM14 3v5h5"
    : normalized === "write" || normalized === "edit" || normalized.includes("patch")
    ? "M4 20h4L19.5 8.5a2.1 2.1 0 0 0-3-3L5 17zM13.5 6.5l3 3"
    : normalized.includes("fetch") || normalized.includes("web") || normalized.includes("http")
    ? "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zm-9 9h18M12 3c2.5 2.4 3.8 5.6 3.8 9s-1.3 6.6-3.8 9c-2.5-2.4-3.8-5.6-3.8-9s1.3-6.6 3.8-9z"
    : "M4 17l6-6-6-6M12 19h8";
  return (
    <span className="chat-tool-icon">
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><path d={path} /></svg>
    </span>
  );
}

function ToolActivity({ item, result }: { item: ToolUseItem; result?: ToolResultItem }) {
  const t = useT();
  return (
    <details className={"chat-tool" + (result?.is_error ? " is-error" : "")}>
      <summary>
        <ToolIcon name={item.name} />
        <span className="chat-tool-name">{friendlyToolName(item.name, t)}</span>
        <span className="chat-tool-summary">{item.summary}</span>
        {/* 结果未到 = 工具还在跑：给行尾一个跳动指示，否则组头明明说「运行中」，
            展开后却看不出是哪条没跑完。 */}
        {!result && <span className="chat-tool-pending" aria-label={t.chat.running}><i /><i /><i /></span>}
        <span className="chat-tool-chevron">›</span>
      </summary>
      {/* summary 已经在标题行展示过，pre 里不再重复念一遍；展开只看结果本身。 */}
      <pre>{result ? (result.text || t.chat.toolNoOutput) : t.chat.toolRunning}</pre>
    </details>
  );
}

/// items 引用不变就不重算：稳态下 650ms 一轮的 history 刷新会让父组件重渲染，
/// 但 items 往往原样不动（reduceChatEvents 无新消息时返回同一引用）。没有这层
/// memo 时，每一轮都要重跑下面的分组循环、重建全部 JSX——长会话上千条时很贵。
const Transcript = memo(function Transcript({ items }: { items: ChatItem[] }) {
  const t = useT();
  const blocks: JSX.Element[] = [];
  for (let index = 0; index < items.length;) {
    const item = items[index];
    if (item.type === "tool_use" || item.type === "tool_result") {
      const tools: Array<ToolUseItem | ToolResultItem> = [];
      while (index < items.length) {
        const candidate = items[index];
        if (candidate.type !== "tool_use" && candidate.type !== "tool_result") break;
        tools.push(candidate);
        index += 1;
      }
      const results = new Map<string, ToolResultItem>();
      for (const tool of tools) {
        if (tool.type === "tool_result" && tool.tool_use_id) results.set(tool.tool_use_id, tool);
      }
      const consumed = new Set<string>();
      const callCount = tools.filter((tool) => tool.type === "tool_use").length;
      const failureCount = tools.filter((tool) => tool.type === "tool_result" && tool.is_error).length;
      const names = [...new Set(tools
        .filter((tool): tool is ToolUseItem => tool.type === "tool_use")
        .map((tool) => friendlyToolName(tool.name, t)))].slice(0, 3);
      blocks.push(<details className={"chat-activity-group" + (failureCount ? " is-error" : "")} key={`tools-${tools[0].id}`}>
        <summary className="chat-activity-summary">
          <span className="chat-tool-icon"><svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><path d="M4 17l6-6-6-6M12 19h8" /></svg></span>
          <span className="chat-activity-count">{t.chat.toolActivities(callCount || tools.length)}</span>
          <span className="chat-activity-kinds">{names.join(" · ")}</span>
          {failureCount > 0 && <span className="chat-activity-errors">{t.chat.toolFailures(failureCount)}</span>}
          <span className="chat-tool-chevron">›</span>
        </summary>
        <div className="chat-activity-items">{tools.map((tool) => {
          if (tool.type === "tool_use") {
            const result = results.get(tool.id);
            if (result) consumed.add(result.id);
            return <ToolActivity key={tool.id} item={tool} result={result} />;
          }
          return consumed.has(tool.id) ? null : <Message key={tool.id} item={tool} />;
        })}</div>
      </details>);
      continue;
    }
    blocks.push(<Message key={item.id} item={item} />);
    index += 1;
  }
  return <>{blocks}</>;
});

/// 两次 history 的**渲染相关**字段是否完全一致。稳态轮询里这些值一轮都不变，
/// 据此保留旧引用即可跳过整窗重渲染。
///
/// 只比 UI 真正读的字段——items/offset/reset 不在其中，它们由 setItems 单独短路。
/// 新增会被渲染的字段时必须同步加进来，否则会出现「数据变了但界面不动」。
function sameHistoryMeta(prev: ChatHistory | null, next: ChatHistory): boolean {
  return prev !== null
    && prev.sessionId === next.sessionId
    && prev.title === next.title
    && prev.status === next.status
    && prev.provider === next.provider
    && prev.cwd === next.cwd
    && prev.supported === next.supported
    && prev.pendingReview === next.pendingReview
    && prev.model === next.model
    && prev.agentModes.length === next.agentModes.length
    && prev.agentModes.every((mode, index) => mode.dimension === next.agentModes[index]?.dimension && mode.value === next.agentModes[index]?.value)
    && prev.contextPct === next.contextPct
    && prev.contextWindow === next.contextWindow
    && prev.currentActivity === next.currentActivity
    && prev.hasMore === next.hasMore
    // 兜底时间线读它们（transcript 空窗期渲染 hook 落库的最近往来）。
    && prev.lastUserText === next.lastUserText
    && prev.lastAiText === next.lastAiText;
}

type Attachment = { path: string; name: string; image: boolean };
const IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"]);

function attachmentOf(path: string): Attachment {
  const name = path.split(/[\\/]/).pop() || path;
  const extension = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
  return { path, name, image: IMAGE_EXTENSIONS.has(extension) };
}

function promptWithAttachments(prompt: string, attachments: Attachment[]): string {
  if (!attachments.length) return prompt;
  const files = attachments.map((file) => `- ${file.path}`).join("\n");
  const instruction = `请读取并结合以下本地附件完成任务（图片请使用图像读取能力）：\n${files}`;
  return prompt.trim() ? `${prompt.trim()}\n\n${instruction}` : instruction;
}

function terminalInput(content: string): string {
  // 多行内容必须作为一次 bracketed paste 交给 TUI composer，否则附件列表中的换行可能被当成
  // 多次 Enter，导致第一行提前提交。单行保持原协议，兼容不启用 bracketed paste 的旧 CLI。
  return content.includes("\n") ? `\x1b[200~${content}\x1b[201~` : content;
}

type TerminalStartupResult = "ready" | TerminalAttention;

type TerminalReadyMessages = {
  exited: (code: number | null) => string;
  failed: string;
  timeout: string;
};

async function waitForTerminalReady(sessionId: number, attentionMarkers: string[], messages: TerminalReadyMessages): Promise<TerminalStartupResult> {
  const startedAt = Date.now();
  const decoder = new TextDecoder();
  let outputTail = "";
  let lastOutputAt = 0;
  let hasVisible = false;
  // 带 since 只拉增量；保留一小段解码后的尾部，让跨 IPC 分片的提示仍可识别。
  let since = 0;
  while (Date.now() - startedAt < 45_000) {
    const snapshot = await managedTerminalSnapshot(sessionId, since);
    if (!snapshot.active) {
      if (snapshot.exited) {
        throw new Error(messages.exited(snapshot.exitCode ?? null));
      }
      throw new Error(messages.failed);
    }
    // 判「有新输出」看 endOffset 而不是 data：data 现在是增量，首帧之后的轮次
    // 常常为空，用它判断会把已经就绪的终端误判成还没输出。
    const grew = snapshot.endOffset > since;
    since = snapshot.endOffset;
    outputTail = appendTerminalText(outputTail, snapshot.data, decoder);
    const attention = detectTerminalAttention(outputTail, attentionMarkers);
    if (attention) return attention;
    if (grew) lastOutputAt = Date.now();
    if (!hasVisible && visibleTerminalText(outputTail)) hasVisible = true;
    // 就绪 = 已画出可见内容（--resume 启动阶段只有清屏/光标序列，不算）且输出安静了
    // 700ms。回放长 transcript 时输出持续、计时随之顺延，不会把消息写进还在初始化的
    // composer；固定「首字节后 700ms」正是之前吞消息的根因。
    if (hasVisible && lastOutputAt && Date.now() - lastOutputAt >= 700) return "ready";
    // 极端情况：TUI 常驻动画让输出永不安静。已有可见画面且没识别到阻塞提示时，
    // 20 秒后按就绪处理——比直接超时报错对用户更有用。
    if (hasVisible && Date.now() - startedAt >= 20_000) return "ready";
    await new Promise((resolve) => window.setTimeout(resolve, 80));
  }
  throw new Error(messages.timeout);
}

export function ChatWindow() {
  const t = useT();
  const [sessionId, setSessionId] = useState(initialSessionId);
  const [history, setHistory] = useState<ChatHistory | null>(null);
  const [items, setItems] = useState<ChatItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  // 首读裁剪掉的更早消息：hasMore 只在首读那一发为 true，轮询会把它带回 false，
  // 所以单独存一份状态，别直接读 history.hasMore（提示会闪一下就没）。
  const [hasEarlier, setHasEarlier] = useState(false);
  const [loadingEarlier, setLoadingEarlier] = useState(false);
  const [view, setView] = useState<"chat" | "terminal">(sessionId < 0 ? "terminal" : "chat");
  const [prompt, setPrompt] = useState("");
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState("");
  const [terminalAttention, setTerminalAttention] = useState<TerminalAttention | null>(null);
  const [questionCustomText, setQuestionCustomText] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const promptRef = useRef(prompt);
  const attachmentsRef = useRef(attachments);
  const viewRef = useRef(view);
  // 终端视图一旦显示过就常驻树上（隐藏而非卸载），避免来回切 tab 反复 dispose/new Terminal。
  const terminalEverShownRef = useRef(view === "terminal");
  // 活跃托管 PTY 需要一个隐藏的 xterm 来执行 ANSI 光标/清行序列并得到真实屏幕；这不改变
  // 当前 tab，只把 xterm 从“可见终端 UI”降为后台屏幕状态机。
  const [terminalMonitorNeeded, setTerminalMonitorNeeded] = useState(view === "terminal");
  const [managedPtyActive, setManagedPtyActive] = useState(false);
  // 已挂载的 ManagedTerminal 暴露的重启复位钩子：对话页重启 PTY（sendText/changeMode）后
  // 调它把输出偏移归零，否则新进程的输出全被旧偏移判成重复而丢弃。
  const terminalRearmRef = useRef<(() => void) | null>(null);
  const terminalReadyMessages: TerminalReadyMessages = {
    exited: t.chat.terminalStartExited,
    failed: t.chat.terminalStartFailed,
    timeout: t.chat.terminalReadyTimeout,
  };
  const revealTerminalAttention = useCallback((attention: TerminalAttention | null) => {
    if (!attention) { setTerminalAttention(null); return; }
    terminalEverShownRef.current = true;
    setTerminalAttention((current) => current?.id === attention.id
      && current.text === attention.text
      && JSON.stringify(current.options) === JSON.stringify(attention.options)
      ? current : attention);
  }, []);
  const draftsRef = useRef(new Map<number, { prompt: string; attachments: Attachment[] }>());
  promptRef.current = prompt;
  attachmentsRef.current = attachments;
  viewRef.current = view;
  if (view === "terminal") terminalEverShownRef.current = true;
  const terminalMounted = terminalEverShownRef.current || terminalMonitorNeeded;
  const [approval, setApproval] = useState<PendingApproval | null>(null);
  const [brokerOwnsReview, setBrokerOwnsReview] = useState(false);
  const externalRunning = isExternallyHeld(history?.status);
  const [resolvingApproval, setResolvingApproval] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "1");
  const toggleSidebar = () => setSidebarCollapsed((prev) => {
    const next = !prev;
    localStorage.setItem(SIDEBAR_COLLAPSED_KEY, next ? "1" : "0");
    return next;
  });
  const [modelMenu, setModelMenu] = useState(false);
  const [modeMenu, setModeMenu] = useState<string | null>(null);
  // 对话页能力由安装实况组装（基础命令 ∪ 用户/项目命令 ∪ 当前会话 runtime skill 清单），
  // 按 provider+cwd 查询——换会话、换项目都重取，装了新命令下次打开就见。
  // 未知 provider / 查询未回时为空：不补全、不给菜单，宁缺毋滥。
  const [chatUi, setChatUi] = useState<ChatUi | null>(null);
  const [capabilityOffset, setCapabilityOffset] = useState(0);
  const provider = history?.provider;
  const cwd = history?.cwd ?? null;
  const transcriptCapabilitiesReady = capabilityOffset > 0;
  const runtimeCapabilityProbe = chatUi?.runtime_commands_pending
    ? capabilityOffset
    : transcriptCapabilitiesReady ? 1 : 0;
  const startupAttentionMarkerKey = (chatUi?.startup_attention_markers ?? []).join("\0");
  const terminalInteractivePrompt = history?.pendingReview === "question" || history?.pendingReview === "plan";
  // runtime 清单未就绪时，探测键随每次 650ms 轮询的 offset 变化而变化——不能每变一次就
  // 打一发 agent_chat_ui（后端要重扫命令目录、探 transcript）。同一会话内限频到 2s 一查；
  // 换会话/换 provider/换 cwd 仍立即查。
  const chatUiProbeRef = useRef({ key: "", at: 0 });
  useEffect(() => {
    if (!provider) return;
    let stale = false;
    let timer = 0;
    const key = `${provider}\0${cwd ?? ""}\0${sessionId}`;
    const fetchUi = () => {
      chatUiProbeRef.current = { key, at: Date.now() };
      agentChatUi(provider, cwd, sessionId).then((ui) => { if (!stale) setChatUi(ui); }).catch(() => {});
    };
    const last = chatUiProbeRef.current;
    const wait = last.key === key ? 2_000 - (Date.now() - last.at) : 0;
    if (wait > 0) timer = window.setTimeout(fetchUi, wait);
    else fetchUi();
    return () => { stale = true; window.clearTimeout(timer); };
  }, [provider, cwd, sessionId, runtimeCapabilityProbe]);

  // 启动阻塞属于会话状态，不属于终端视图。即使用户从未打开终端 tab，也先用轻量增量
  // snapshot 探测 PTY；发现活跃 PTY 后在屏幕外挂载 xterm，还原 ANSI 当前屏并持续识别选择器。
  useEffect(() => {
    if (sessionId <= 0) return;
    let cancelled = false;
    let timer = 0;
    let since = 0;
    let outputTail = "";
    let reportedId: string | null = null;
    const decoder = new TextDecoder();
    const markers = chatUi?.startup_attention_markers ?? [];
    const poll = async () => {
      try {
        const snapshot = await managedTerminalSnapshot(sessionId, since);
        if (cancelled) return;
        setManagedPtyActive(snapshot.active);
        if (snapshot.endOffset < since) {
          since = 0;
          outputTail = "";
          reportedId = null;
        }
        outputTail = appendTerminalText(outputTail, snapshot.data, decoder);
        since = snapshot.endOffset;
        if (snapshot.data) {
          const attention = detectTerminalAttention(outputTail, markers, terminalInteractivePrompt);
          if (attention) {
            if (attention.id !== reportedId) revealTerminalAttention(attention);
            reportedId = attention.id;
          } else {
            reportedId = null;
          }
        }
        if (snapshot.active) {
          // 后续输出和 ANSI 屏幕识别交给 ManagedTerminal 的事件监听，不再重复轮询 IPC。
          setTerminalMonitorNeeded(true);
          return;
        }
        timer = window.setTimeout(() => void poll(), snapshot.active ? 350 : 1_200);
      } catch {
        if (!cancelled) timer = window.setTimeout(() => void poll(), 1_200);
      }
    };
    void poll();
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [sessionId, startupAttentionMarkerKey, terminalInteractivePrompt, revealTerminalAttention]);
  // "/xx" 且尚未输入空格时给补全候选；一旦带参数或是普通句子就收起。
  // transcript 之外的兜底时间线：hook 落库的最近一问一答（UserPromptSubmit / Stop）。
  // transcript 尚未落盘/尚未定位到、或该 agent 不提供结构化 transcript 时用它渲染，
  // 让「会话已在工作」有真实内容可看。transcript 一旦就位（items 非空）即被完整记录取代。
  const provisional: ChatItem[] = [];
  if (history?.lastUserText) provisional.push({ type: "user_text", id: "hook:last-user", timestamp: null, text: history.lastUserText });
  if (history?.lastAiText) provisional.push({ type: "assistant_text", id: "hook:last-ai", timestamp: null, text: history.lastAiText });
  const slashMatches = prompt.startsWith("/") && !prompt.includes(" ")
    ? (chatUi?.slash_commands ?? []).filter((c) => c.name.startsWith(prompt) && c.name !== prompt)
    : [];
  const modelPresets = chatUi?.model_presets ?? [];
  const modeControls = chatUi?.mode_controls ?? [];
  const offsetRef = useRef(0);
  const activeSessionRef = useRef(sessionId);
  const busyRef = useRef(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const followRef = useRef(true);
  const positionedRef = useRef(false);

  const resetTo = useCallback((id: number) => {
    if (!Number.isSafeInteger(id) || id === 0) return;
    if (id === activeSessionRef.current) return;
    draftsRef.current.set(activeSessionRef.current, {
      prompt: promptRef.current,
      attachments: attachmentsRef.current,
    });
    const draft = draftsRef.current.get(id);
    // 常驻只为省下同一会话内来回切 tab 的 dispose/new Terminal。换会话时 ManagedTerminal
    // 本来就随 key 重挂，继续记着「显示过」只会让每次切换都在后台白挂一个终端
    // （xterm 创建 + 两个 listen + 一次全量 backlog 拉取）。终端模式下 view 仍是 terminal，
    // 常驻照旧生效，不影响「切会话保持终端模式」。
    terminalEverShownRef.current = viewRef.current === "terminal";
    offsetRef.current = 0;
    activeSessionRef.current = id;
    setItems([]);
    setHistory(null);
    setQuestionCustomText("");
    setLoading(true);
    setFailed(false);
    setPrompt(draft?.prompt ?? "");
    setSendError("");
    setTerminalAttention(null);
    setTerminalMonitorNeeded(false);
    setManagedPtyActive(false);
    setAttachments(draft?.attachments ?? []);
    setApproval(null);
    setModeMenu(null);
    setChatUi(null);
    setCapabilityOffset(0);
    setBrokerOwnsReview(false);
    setHasEarlier(false);
    setLoadingEarlier(false);
    positionedRef.current = false;
    followRef.current = true;
    // 切会话保持当前视图（终端模式下切会话仍在终端）。负 id 是尚未认领的临时会话，
    // 还没有 transcript 可看，只能进终端——它 claim 成真 id 时 activeSessionRef 已是负数，
    // 此时 view 已是 terminal，保持即可，故无需为 pending 单列分支。
    setView(id < 0 ? "terminal" : viewRef.current);
    setSessionId(id);
  }, []);

  const refresh = useCallback(async () => {
    if (sessionId <= 0 || busyRef.current) {
      if (sessionId < 0) setLoading(false);
      return;
    }
    busyRef.current = true;
    try {
      const next = await getChatHistory(sessionId, offsetRef.current);
      if (activeSessionRef.current !== sessionId) return;
      setCapabilityOffset(next.offset);
      // hasMore 只有首读那一发才可能为 true，后续增量恒为 false——单独记下来。
      if (next.hasMore) setHasEarlier(true);
      offsetRef.current = next.offset;
      // 保留旧引用（而非无条件 setHistory）——稳态下这些字段一轮都不变，但 next 每次
      // 都是新对象，无脑 set 会让整个窗口每 650ms 重渲染一次。items 已在下面单独短路。
      setHistory((prev) => {
        // mode 只在 transcript 出现新模式记录时随增量返回。普通增量为 null 时保留上次观测；
        // 文件 reset 则必须采信全量重读结果，避免沿用旧 transcript 的状态。
        const updates = next.agentModes ?? [];
        const agentModes = next.reset || prev?.sessionId !== next.sessionId
          ? updates
          : [...(prev?.agentModes ?? [])];
        if (!next.reset && prev?.sessionId === next.sessionId) {
          for (const update of updates) {
            const index = agentModes.findIndex((mode) => mode.dimension === update.dimension);
            if (index >= 0) agentModes[index] = update;
            else agentModes.push(update);
          }
        }
        const merged = { ...next, agentModes };
        return sameHistoryMeta(prev, merged) ? prev : merged;
      });
      setItems((prev) => next.items.length || next.reset ? reduceChatEvents(prev, next.items, next.reset) : prev);
      setLoading(false);
      setFailed(false);
    } catch {
      setLoading(false);
      setFailed(true);
    } finally {
      busyRef.current = false;
    }
  }, [sessionId]);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 650);
    return () => window.clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    if (sessionId >= 0) return;
    let cancelled = false;
    const resolve = () => managedTerminalBinding(sessionId).then((id) => {
      if (!cancelled && id) resetTo(id);
    }).catch(() => {});
    void resolve();
    const timer = window.setInterval(() => void resolve(), 250);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [resetTo, sessionId]);

  useEffect(() => {
    if (sessionId <= 0) return;
    const consumerId = `chat-${sessionId}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
    let disposed = false;
    let retryTimer = 0;
    // 注册失败不能就此放弃：没有租约，后端会把所有审批直接交还终端 TUI，
    // 而这扇窗看起来一切正常（轮询永远拿不到 pending）——用户以为在 GUI 等审批，
    // 实际审批卡在终端里没人看。effect 不会重跑，这里自己做有限退避重试。
    const register = (attempt: number) => {
      void registerApprovalConsumer(sessionId, consumerId).then(() => {
        // effect 可能在 IPC 返回前已经因切会话/关窗清理；再次注销闭合这个竞态窗口。
        if (disposed) void unregisterApprovalConsumer(consumerId).catch(() => {});
      }).catch((error) => {
        console.error("注册审批消费者失败", error);
        if (disposed || attempt >= 5) return;
        retryTimer = window.setTimeout(() => register(attempt + 1), 500 * 2 ** attempt);
      });
    };
    register(0);
    return () => {
      disposed = true;
      window.clearTimeout(retryTimer);
      void unregisterApprovalConsumer(consumerId).catch(() => {});
    };
  }, [sessionId]);

  useEffect(() => {
    if (sessionId <= 0) return;
    let cancelled = false;
    const poll = () => getPendingApproval(sessionId).then((next) => {
      if (!cancelled) {
        if (next) setBrokerOwnsReview(true);
        setApproval(next);
      }
    }).catch(() => {});
    void poll();
    const timer = window.setInterval(() => void poll(), 400);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [sessionId]);

  useEffect(() => {
    let un: (() => void) | undefined;
    let cancelled = false;
    listen<number>("chat-session-changed", (event) => resetTo(event.payload)).then((fn) => {
      if (cancelled) fn(); else un = fn;
    }).catch(() => {});
    return () => { cancelled = true; un?.(); };
  }, [resetTo]);

  useEffect(() => {
    let unApproval: (() => void) | undefined;
    let unCleared: (() => void) | undefined;
    let cancelled = false;
    listen<PendingApproval>("pending-approval", (event) => {
      if (event.payload.sessionId === activeSessionRef.current) {
        setBrokerOwnsReview(true);
        setApproval(event.payload);
        setView("chat");
      }
    }).then((fn) => { if (cancelled) fn(); else unApproval = fn; }).catch(() => {});
    listen<PendingApproval>("pending-approval-cleared", (event) => {
      if (event.payload.sessionId === activeSessionRef.current) {
        setApproval((current) => current?.requestId === event.payload.requestId ? null : current);
      }
    }).then((fn) => { if (cancelled) fn(); else unCleared = fn; }).catch(() => {});
    return () => { cancelled = true; unApproval?.(); unCleared?.(); };
  }, []);

  useEffect(() => {
    // transcript 的 pendingReview 比 broker 的实时状态慢一拍。只有历史状态确实清空后，
    // 才重新启用“去终端处理”的兼容提示，避免 GUI 审批完成时闪一下旧提示。
    if (!history?.pendingReview) setBrokerOwnsReview(false);
  }, [history?.pendingReview]);

  useLayoutEffect(() => {
    if (view !== "chat") {
      // 终端页会卸载 chat-scroll；切回来得到的是全新的滚动容器，必须重新做一次首帧定位。
      positionedRef.current = false;
      followRef.current = true;
      return;
    }
    if (!followRef.current) return;
    const el = scrollRef.current;
    if (!el || items.length === 0) return;
    if (!positionedRef.current) {
      // 历史消息首次出现时必须在绘制前瞬移到底部；否则 `.chat-scroll` 的 smooth 行为会让
      // 用户每次打开窗口都看到整段对话从顶部滚下来。
      const behavior = el.style.scrollBehavior;
      el.style.scrollBehavior = "auto";
      el.scrollTop = el.scrollHeight;
      el.style.scrollBehavior = behavior;
      positionedRef.current = true;
      return;
    }
    el.scrollTop = el.scrollHeight;
  }, [items, view]);

  useLayoutEffect(() => {
    if (view !== "chat" || terminalAttention?.id !== "interactive:numbered-selector" || !followRef.current) return;
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [terminalAttention, view]);

  /// 拉取被首读裁掉的更早消息，并保持用户当前看的那一行不动。
  /// 直接替换 items 会让滚动位置塌到顶部——记下加载前的 scrollHeight，补回增量即可。
  /// 与轮询共用 busyRef：两者都会写 offsetRef/items，交叉执行时先返回的一方会被后返回的
  /// 覆盖——轮询刚追加的新消息会被这里的全量替换抹掉（下一轮才补回来，表现为闪烁）。
  const loadEarlier = async () => {
    if (loadingEarlier || busyRef.current) return;
    busyRef.current = true;
    setLoadingEarlier(true);
    const el = scrollRef.current;
    const prevHeight = el?.scrollHeight ?? 0;
    const prevTop = el?.scrollTop ?? 0;
    try {
      const full = await getChatHistory(sessionId, 0, true);
      if (activeSessionRef.current !== sessionId) return;
      offsetRef.current = full.offset;
      // 全量重建：这批数据已包含现有消息，直接替换而不是 append。
      setItems(reduceChatEvents([], full.items, true));
      setHasEarlier(false);
      // 跳过一次自动吸底，否则用户会被弹回最新消息。
      followRef.current = false;
      requestAnimationFrame(() => {
        const node = scrollRef.current;
        if (!node) return;
        node.scrollTop = prevTop + (node.scrollHeight - prevHeight);
        // 按落位重算吸底状态。内容不足一屏时 scrollTop 仍是 0，不会有 scroll 事件来
        // 恢复 followRef，漏掉这一步会让之后的新消息再也不自动滚到底。
        followRef.current = node.scrollHeight - node.scrollTop - node.clientHeight < 80;
      });
    } catch {
      // 失败不清空已有消息：保留提示让用户可以再点一次。
    } finally {
      busyRef.current = false;
      setLoadingEarlier(false);
    }
  };

  const onScroll = () => {
    const el = scrollRef.current;
    if (el) followRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
  };
  const close = () => getCurrentWindow().close().catch(() => {});
  /// 发送一段文本到会话（消息正文与斜杠命令共用）。返回是否真的送达。
  const sendText = async (content: string): Promise<boolean> => {
    setSending(true);
    setSendError("");
    try {
      if (terminalAttention) {
        terminalEverShownRef.current = true;
        setSendError(t.chat.terminalNeedsAttention);
        return false;
      }
      const snapshot = await managedTerminalSnapshot(sessionId);
      if (!snapshot.active) {
        // 历史/断开会话无需先切换终端页手动接管；发送本身就是明确的恢复意图。
        // 但会话仍在外部终端里跑着时不能这么做：后端守卫会拒绝对同一 session 起第二个
        // agent，用户只会看到一句「不能重复接管」。接管要杀掉外部进程，必须由用户显式
        // 确认（终端页的接管按钮），不能由一次发送隐式代劳。
        if (externalRunning) {
          setSendError(t.chat.sendNeedsTakeover);
          return false;
        }
        // capability 查询通常已随 history 完成；用户极快发送时就在这里补等一次，不能因为
        // React 状态尚未落下而漏掉 provider 声明的信任/登录提示。
        const ui = chatUi ?? (provider ? await agentChatUi(provider, cwd, sessionId).catch(() => null) : null);
        await startManagedTerminal(sessionId, 100, 30);
        // 已挂载的后台终端还停在旧进程的输出偏移上，必须归零重拉，否则新 PTY 的输出
        // 会被它当成「已写过」整段丢弃，画面定格、屏幕识别全部失效。
        terminalRearmRef.current?.();
        const startup = await waitForTerminalReady(sessionId, ui?.startup_attention_markers ?? [], terminalReadyMessages);
        if (startup !== "ready") {
          terminalEverShownRef.current = true;
          setTerminalAttention(startup);
          setSendError(t.chat.terminalNeedsAttention);
          return false;
        }
      }
      // 正文与 Enter 分成两次 IPC/PTY flush。部分 TUI 在同一个输入 chunk 中收到 paste-end + Enter
      // 时只更新 composer 而不提交；分开发送与真实键盘输入语义一致。
      await writeManagedTerminal(sessionId, terminalInput(content));
      await new Promise((resolve) => window.setTimeout(resolve, 20));
      await writeManagedTerminal(sessionId, "\r");
      return true;
    } catch (error) {
      setSendError(String(error));
      return false;
    } finally {
      setSending(false);
    }
  };
  const sendPrompt = async () => {
    if ((!prompt.trim() && attachments.length === 0) || sending) return;
    if (await sendText(promptWithAttachments(prompt, attachments))) {
      setPrompt("");
      setAttachments([]);
    }
  };
  /// 斜杠命令直通 PTY——CLI 的 composer 收到 "/xxx" + 回车会当命令执行，无需特殊协议。
  const sendSlash = (command: string) => {
    if (!sending) void sendText(command);
  };
  /// 模式动作完全由插件描述：快捷键原样发送，命令则用和人工输入一致的 paste + Enter 序列。
  const changeMode = async (dimension: string, inputs: { data: string; submit: boolean }[], optimisticValue?: string) => {
    if (inputs.length === 0 || sending) return;
    setSending(true);
    setSendError("");
    try {
      if (terminalAttention) {
        terminalEverShownRef.current = true;
        setSendError(t.chat.terminalNeedsAttention);
        return;
      }
      const snapshot = await managedTerminalSnapshot(sessionId);
      if (!snapshot.active) {
        if (externalRunning) {
          setSendError(t.chat.sendNeedsTakeover);
          return;
        }
        const ui = chatUi ?? (provider ? await agentChatUi(provider, cwd, sessionId).catch(() => null) : null);
        await startManagedTerminal(sessionId, 100, 30);
        // 同 sendText：重启后必须让后台终端的输出偏移归零，见 terminalRearmRef 注释。
        terminalRearmRef.current?.();
        const startup = await waitForTerminalReady(sessionId, ui?.startup_attention_markers ?? [], terminalReadyMessages);
        if (startup !== "ready") {
          terminalEverShownRef.current = true;
          setTerminalAttention(startup);
          setSendError(t.chat.terminalNeedsAttention);
          return;
        }
      }
      for (const input of inputs) {
        await writeManagedTerminal(sessionId, input.submit ? terminalInput(input.data) : input.data);
        if (input.submit) {
          await new Promise((resolve) => window.setTimeout(resolve, 20));
          await writeManagedTerminal(sessionId, "\r");
        }
      }
      if (optimisticValue) {
        setHistory((current) => {
          if (!current) return current;
          const agentModes = [...current.agentModes];
          const index = agentModes.findIndex((mode) => mode.dimension === dimension);
          const update = { dimension, value: optimisticValue };
          if (index >= 0) agentModes[index] = update;
          else agentModes.push(update);
          return { ...current, agentModes };
        });
      }
    } catch (error) {
      setSendError(String(error));
    } finally {
      setSending(false);
    }
  };
  const chooseAttachments = async () => {
    const selected = await open({ multiple: true, directory: false, title: t.chat.chooseAttachments });
    const paths = selected == null ? [] : Array.isArray(selected) ? selected : [selected];
    setAttachments((current) => {
      const known = new Set(current.map((file) => file.path));
      return [...current, ...paths.filter((path) => !known.has(path)).map(attachmentOf)].slice(0, 12);
    });
  };
  const decideApproval = async (choice: string) => {
    if (!approval || resolvingApproval) return;
    setResolvingApproval(true);
    setBrokerOwnsReview(true);
    try {
      await resolvePendingApproval(sessionId, approval.requestId, choice);
      setApproval(null);
    } catch {
      // 下一次轮询会恢复仍有效的请求；若 hook 已结束则保持消失。
    } finally {
      setResolvingApproval(false);
    }
  };

  const commandAttention = terminalAttention?.id === "claude:command-approval" ? terminalAttention : null;
  const interactiveAttention = terminalAttention?.id === "interactive:numbered-selector" ? terminalAttention : null;
  const commandApproval = commandAttention ? claudeCommandApprovalDetails(commandAttention.text) : null;
  const commandOptions = commandAttention?.options ?? [];
  const commandDeny = commandOptions.find((option) => /^no\b/i.test(option.label));
  const commandAllowOnce = commandOptions.find((option) => /^yes$/i.test(option.label.trim())) ?? commandOptions[0];
  const commandRemember = commandOptions.find((option) => option !== commandDeny && option !== commandAllowOnce);
  const chooseTerminalOption = (option: { input: string } | undefined) => {
    if (!option) return;
    void writeManagedTerminal(sessionId, option.input)
      .then(() => setTerminalAttention(null))
      .catch((error) => setSendError(String(error)));
  };
  const chooseInteractiveOption = (option: TerminalAttentionOption) => {
    if (!option.input) return;
    if (option.kind === "choice") {
      setTerminalAttention((current) => current?.id !== "interactive:numbered-selector" ? current : {
        ...current,
        options: current.options?.map((entry) => {
          const position = entry.position ?? 0;
          const target = option.position ?? 0;
          const delta = position - target;
          return {
            ...entry,
            selected: entry.position === option.position ? !entry.selected : entry.selected,
            focused: entry.position === option.position,
            input: delta < 0 ? "\x1b[A".repeat(-delta) + "\r" : "\x1b[B".repeat(delta) + "\r",
          };
        }),
      });
    }
    void writeManagedTerminal(sessionId, option.input)
      .then(() => { if (option.kind === "submit" || option.kind === "chat") setTerminalAttention(null); })
      .catch((error) => setSendError(String(error)));
  };
  const submitCustomAnswer = (option: TerminalAttentionOption) => {
    const value = questionCustomText.trim();
    if (!value || !option.input) return;
    void writeManagedTerminal(sessionId, option.input + value + "\r")
      .then(() => setQuestionCustomText(""))
      .catch((error) => setSendError(String(error)));
  };

  return (
    <div className={"chat-window" + (view === "terminal" ? " is-terminal" : "")}>
      {!sidebarCollapsed && <ChatSidebar
        activeId={sessionId}
        onSelect={(id) => { if (id !== sessionId) resetTo(id); }}
        onCollapse={toggleSidebar}
      />}
      <div className="chat-main">
      <header className="chat-bar" data-tauri-drag-region>
        {sidebarCollapsed && (
          <button
            type="button"
            className="chat-sidebar-open"
            aria-label={t.chat.sidebarExpand}
            data-tip={t.chat.sidebarExpand}
            onClick={toggleSidebar}
          >
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
              <rect x="3" y="4" width="18" height="16" rx="2.5" />
              <path d="M9.5 4v16" />
            </svg>
          </button>
        )}
        {history?.provider && (() => {
          const Icon = agentAssets(history.provider).Icon;
          return (
            <span className="chat-provider-logo" style={tintStyle(history.provider, true)} aria-label={history.provider} data-tauri-drag-region>
              <Icon />
            </span>
          );
        })()}
        <div className="chat-heading" data-tauri-drag-region>
          <span className="chat-title" data-tauri-drag-region>{history?.title || t.chat.title}</span>
          {history?.cwd && (
            <button
              type="button"
              className="chat-cwd"
              title={t.sticker.openProjectDir}
              onClick={() => history.cwd && void invoke("open_project_dir", { cwd: history.cwd }).catch(() => {})}
            >{history.cwd}</button>
          )}
        </div>
        <span className="chat-live" data-tauri-drag-region><i data-tauri-drag-region />{t.chat.live}</span>
        <div className="chat-view-tabs">
          <button type="button" className={view === "chat" ? "is-active" : ""} onClick={() => setView("chat")}>{t.chat.conversation}</button>
          <button type="button" className={view === "terminal" ? "is-active" : ""} onClick={() => setView("terminal")}>{t.chat.terminal}</button>
        </div>
        <button type="button" className="winclose" aria-label={t.chat.close} data-tip={t.chat.close} onClick={close}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M18 6L6 18" /></svg>
        </button>
      </header>
      {/* 两个视图都留在树上、用 CSS 切换可见性：此前三元表达式会在每次切 tab 时
          dispose + new Terminal()，还要把整个 backlog 重传并让 xterm 重放一遍 ANSI。
          终端侧懒挂载（terminalMounted），纯对话的用户不用白付 xterm 的创建成本。 */}
      <main className="chat-scroll" ref={scrollRef} onScroll={onScroll} hidden={view !== "chat"}>
        {loading ? <div className="chat-empty">{t.chat.loading}</div>
          : failed ? <div className="chat-empty is-error">{t.chat.loadError}</div>
          : history && !history.supported ? (
            /* 不提供结构化 transcript 的 agent：hook 落库的最近往来仍然是真实数据，先渲染它，
               「暂未提供结构化对话记录」降为其下的注脚——有什么就展示什么，而不是只报没有。 */
            provisional.length > 0
              ? <><Transcript items={provisional} /><div className="chat-empty is-note">{t.chat.unsupported}</div></>
              : <div className="chat-empty">{t.chat.unsupported}</div>
          )
          /* 空列表分两种事实：会话已在跑（transcript 尚未落第一条/尚未定位到）≠ 真的没有记录。
             hook 侧已知的最近往来（lastUserText / lastAiText）先顶上；连它也没有时，
             running 态也不能说「没有内容」——那与下面的运行指示互相打架。 */
          : items.length === 0 ? (
            provisional.length > 0
              ? <Transcript items={provisional} />
              : <div className="chat-empty">{history?.status === "running" ? t.chat.emptyWorking : t.chat.empty}</div>
          )
          : <>
            {hasEarlier && (
              <div className="chat-load-earlier">
                <button type="button" onClick={() => void loadEarlier()} disabled={loadingEarlier}>
                  {loadingEarlier ? t.chat.loadingEarlier : t.chat.loadEarlier}
                </button>
              </div>
            )}
            <Transcript items={items} />
          </>}
        {/* Agent 正在跑但 transcript 半天不落新行时，页面此前毫无动静，像卡死。
            running 态常驻一个脉冲指示，有具体活动（工具名）就显示出来。 */}
        {!loading && history?.status === "running" && (
          <div className="chat-running" role="status">
            <i /><span>{history.currentActivity || t.chat.running}</span>
          </div>
        )}
        {interactiveAttention && <section className="chat-inline-question" role="alert">
          <div className="chat-inline-question-head">
            <strong>{history?.pendingReview === "plan" ? t.chat.planTitle : t.chat.questionTitle}</strong>
            {interactiveAttention.text && <span>{interactiveAttention.text}</span>}
          </div>
          <div className="chat-inline-question-options">
            {interactiveAttention.options?.filter((option) => option.kind === "choice").map((option) => (
              <button type="button" disabled={!option.input} className={option.selected ? "is-selected" : ""} key={`${option.position}:${option.label}`} onClick={() => chooseInteractiveOption(option)}>
                <i aria-hidden="true">{option.selected ? "✓" : ""}</i>
                <span><b>{option.label}</b>{option.description && <small>{option.description}</small>}</span>
              </button>
            ))}
          </div>
          {interactiveAttention.options?.filter((option) => option.kind === "input").map((option) => (
            <div className="chat-inline-question-custom" key={option.input}>
              <input value={questionCustomText} onChange={(event) => setQuestionCustomText(event.target.value)} placeholder={t.chat.customAnswerPlaceholder}
                onKeyDown={(event) => { if (event.key === "Enter") submitCustomAnswer(option); }} />
              <button type="button" disabled={!questionCustomText.trim() || !option.input} onClick={() => submitCustomAnswer(option)}>{t.chat.addCustomAnswer}</button>
            </div>
          ))}
          <div className="chat-inline-question-actions">
            {interactiveAttention.options?.filter((option) => option.kind === "chat").map((option) => (
              <button type="button" disabled={!option.input} key={`${option.position}:${option.label}`} onClick={() => chooseInteractiveOption(option)}>{t.chat.chatAboutThis}</button>
            ))}
            {interactiveAttention.options?.filter((option) => option.kind === "submit").map((option) => (
              <button type="button" disabled={!option.input} className="is-primary" key={`${option.position}:${option.label}`} onClick={() => chooseInteractiveOption(option)}>{t.chat.submitAnswer}</button>
            ))}
          </div>
        </section>}
      </main>
      {view === "chat" && commandAttention && commandApproval && <section className="chat-approval chat-terminal-command-approval" role="alert">
        <div className="chat-approval-copy">
          <strong>{t.chat.approvalTitle}</strong>
          <div className="chat-approval-tool"><span>{t.chat.approvalTool}</span><code>Bash</code></div>
          {commandApproval.description && <span>{commandApproval.description}</span>}
          {commandApproval.question && <span>{commandApproval.question}</span>}
          {commandApproval.command && <div className="chat-approval-detail">
            <span>{t.chat.approvalInput}</span>
            <pre>{commandApproval.command}</pre>
          </div>}
        </div>
        <div className="chat-approval-actions">
          {commandDeny && <button type="button" className="is-deny" onClick={() => chooseTerminalOption(commandDeny)}>{t.chat.deny}</button>}
          {commandAllowOnce && <button type="button" className="is-allow" onClick={() => chooseTerminalOption(commandAllowOnce)}>{t.chat.allowOnce}</button>}
          {commandRemember && <button type="button" className="is-allow is-persistent" onClick={() => chooseTerminalOption(commandRemember)}>
            {t.chat.allowRemember}{commandRemember.label.match(/for:\s*(.+)$/i)?.[1] ? ` · ${commandRemember.label.match(/for:\s*(.+)$/i)?.[1]}` : ""}
          </button>}
        </div>
      </section>}
      {view === "chat" && terminalAttention && !commandAttention && !interactiveAttention && <section className="chat-terminal-attention" role="alert">
        <div className="chat-terminal-attention-copy">
          <strong>{terminalAttention.id === "interactive:numbered-selector"
            ? history?.pendingReview === "plan" ? t.chat.planTitle : t.chat.questionTitle
            : terminalAttention.id === "claude:long-session-resume"
            ? t.chat.longSessionPromptTitle
            : terminalAttention.id === "claude:command-approval"
              ? t.chat.approvalTitle
            : terminalAttention.options?.length && terminalAttention.id.startsWith("provider:")
              ? t.chat.trustPromptTitle
              : t.chat.terminalPromptTitle}</strong>
          {terminalAttention.id === "interactive:numbered-selector"
            ? <pre>{terminalAttention.text}</pre>
            : terminalAttention.id === "claude:long-session-resume"
            ? <span>{t.chat.longSessionPromptHelp}</span>
            : terminalAttention.id === "claude:command-approval"
              ? <pre>{terminalAttention.text}</pre>
            : !terminalAttention.options?.length && <>
              <span>{t.chat.terminalPromptHelp}</span>
              <pre>{terminalAttention.text}</pre>
            </>}
        </div>
        <div className={`chat-terminal-attention-actions${terminalAttention.options?.length === 2 ? " has-two-options" : ""}`}>
          {terminalAttention.options?.length ? terminalAttention.options.map((option, index) => (
            <button type="button" className={index === 0 ? "is-primary is-option" : "is-option"} key={`${index}:${option.label}`} onClick={() => {
              void writeManagedTerminal(sessionId, option.input)
                .then(() => setTerminalAttention(null))
                .catch((error) => setSendError(String(error)));
            }}>{option.label}</button>
          )) : <>
            <button type="button" onClick={() => void writeManagedTerminal(sessionId, "\x1b[A")}>{t.chat.terminalPromptUp}</button>
            <button type="button" onClick={() => void writeManagedTerminal(sessionId, "\x1b[B")}>{t.chat.terminalPromptDown}</button>
            <button type="button" className="is-primary" onClick={() => {
              void writeManagedTerminal(sessionId, "\r")
                .then(() => setTerminalAttention(null))
                .catch((error) => setSendError(String(error)));
            }}>{t.chat.terminalPromptConfirm}</button>
            <button type="button" onClick={() => {
              void writeManagedTerminal(sessionId, "\x1b")
                .then(() => setTerminalAttention(null))
                .catch((error) => setSendError(String(error)));
            }}>{t.chat.terminalPromptCancel}</button>
          </>}
        </div>
      </section>}
      {terminalMounted && (
        <div className={`chat-terminal-pane${view !== "terminal" ? " is-background" : ""}`} aria-hidden={view !== "terminal"}>
          <ManagedTerminal
            key={sessionId}
            sessionId={sessionId}
            status={history?.status}
            visible={view === "terminal"}
            attentionMarkers={chatUi?.startup_attention_markers ?? []}
            interactivePrompt={terminalInteractivePrompt}
            onAttention={revealTerminalAttention}
            rearmRef={terminalRearmRef}
          />
        </div>
      )}
      {view === "chat" && !terminalAttention && (approval || (!brokerOwnsReview && history?.pendingReview)) && <section className="chat-approval" role="alert">
        <div className="chat-approval-copy">
          <strong>{approval ? t.chat.approvalTitle : history?.pendingReview === "question" ? t.chat.questionTitle : history?.pendingReview === "plan" ? t.chat.planTitle : t.chat.approvalTitle}</strong>
          {approval ? <>
            <div className="chat-approval-tool"><span>{t.chat.approvalTool}</span><code>{approval.toolName}</code></div>
            {approval.description && <span>{approval.description}</span>}
            {approval.input && <div className="chat-approval-detail">
              <span>{t.chat.approvalInput}</span>
              <pre>{approval.input}</pre>
            </div>}
          </> : <span>{managedPtyActive ? t.chat.approvalReadingTerminal : t.chat.approvalInTerminal}</span>}
        </div>
        {approval ? <div className="chat-approval-actions">
          <button type="button" className="is-deny" disabled={resolvingApproval} onClick={() => void decideApproval("deny")}>{t.chat.deny}</button>
          <button type="button" className="is-allow" disabled={resolvingApproval} onClick={() => void decideApproval("allow_once")}>{t.chat.allowOnce}</button>
          {/* `?? []`：类型上字段恒在（DTO 保证），但旧后端/新前端错配时负载可能缺它——
              一个可选按钮组不值得让整个 ChatWindow 白屏。 */}
          {(approval.permissionSuggestions ?? []).map((suggestion, index) => (
            <button
              type="button"
              className="is-allow is-persistent"
              key={index}
              title={JSON.stringify(suggestion)}
              disabled={resolvingApproval}
              onClick={() => void decideApproval(`suggestion:${index}`)}
            >{approvalSuggestionLabel(suggestion, index, t)}</button>
          ))}
        </div> : <button type="button" onClick={() => setView("terminal")}>{t.chat.openTerminal}</button>}
      </section>}
      {view === "chat" && !terminalAttention && <footer className="chat-compose">
        {slashMatches.length > 0 && <div className="dd-menu chat-slash-menu" role="listbox">
          {slashMatches.map((command) => (
            <button type="button" key={command.name} role="option" aria-selected="false" className="chat-slash-item"
              onClick={() => setPrompt(command.name + " ")}>
              <code>{command.name}</code>
              {/* 自定义命令的描述从命令文件头里读出（后端下发）；内置命令的描述是翻译资产，走 i18n。 */}
              <span>{command.description ?? t.chat.slashDesc[command.name] ?? ""}</span>
            </button>
          ))}
        </div>}
        {attachments.length > 0 && <div className="chat-attachments">
          {attachments.map((file) => <div className="chat-attachment" key={file.path} title={file.path}>
            <span className="chat-file-icon">{file.image ? "IMG" : "FILE"}</span>
            <span>{file.name}</span>
            <button type="button" aria-label={`${t.chat.removeAttachment} ${file.name}`} onClick={() => setAttachments((items) => items.filter((item) => item.path !== file.path))}>×</button>
          </div>)}
        </div>}
        <textarea
          value={prompt}
          rows={1}
          aria-label={t.chat.inputLabel}
          placeholder={sendError ? t.chat.inputUnavailable : t.chat.inputPlaceholder}
          onChange={(event) => { setPrompt(event.target.value); setSendError(""); }}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault();
              void sendPrompt();
            }
          }}
        />
        <div className="chat-compose-actions">
          <button type="button" className="chat-attach-button" aria-label={t.chat.attach} title={t.chat.attach} onClick={() => void chooseAttachments()}>
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65"><path d="M12 5v14M5 12h14" /></svg>
          </button>
          {(modelPresets.length > 0 || history?.model) && <div className="chat-model">
            <button
              type="button"
              className="chat-model-button"
              disabled={modelPresets.length === 0}
              aria-label={t.chat.switchModel}
              title={modelPresets.length > 0 ? t.chat.switchModel : undefined}
              onClick={() => setModelMenu((open) => !open)}
            >
              {history?.model || t.chat.model}
              {modelPresets.length > 0 && <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4"><path d="M6 9l6 6 6-6" /></svg>}
            </button>
            {modelMenu && modelPresets.length > 0 && <div className="dd-menu chat-model-menu" role="menu">
              {modelPresets.map((preset) => {
                const active = history?.model === preset.label;
                return (
                  <button
                    type="button"
                    key={preset.id}
                    role="menuitem"
                    className={"chat-model-item" + (active ? " is-active" : "")}
                    onClick={() => { setModelMenu(false); sendSlash(`/model ${preset.id}`); }}
                  >
                    <span className="chat-model-item-text">
                      <span className="chat-model-item-name">{preset.label}</span>
                      <span className="chat-model-item-desc">{t.chat.modelDesc[preset.id] ?? ""}</span>
                    </span>
                    {active && <svg className="chat-model-check" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><path d="M4.5 12.5l5 5 10-11" /></svg>}
                  </button>
                );
              })}
            </div>}
          </div>}
          {(() => {
            const states = history?.agentModes ?? [];
            const controls = new Map(modeControls.map((control) => [control.dimension, control]));
            const dimensions = [...modeControls.map((control) => control.dimension)];
            for (const state of states) if (!dimensions.includes(state.dimension)) dimensions.push(state.dimension);
            return dimensions.map((dimension) => {
              const control = controls.get(dimension);
              const state = states.find((mode) => mode.dimension === dimension);
              const options = control?.options ?? [];
              const canCycle = Boolean(control?.cycle_input);
              const interactive = options.length > 0 || canCycle;
              const label = t.chat.modeDimensions[dimension] ?? dimension;
              const value = state ? (t.chat.modeNames[state.value] ?? state.value) : "—";
              return <div className="chat-model" key={dimension}>
                <button
                  type="button"
                  className="chat-model-button chat-mode-button"
                  disabled={!interactive || sending}
                  aria-label={interactive ? `${t.chat.switchMode}: ${label}` : label}
                  title={interactive ? t.chat.switchMode : undefined}
                  onClick={() => {
                    if (options.length > 0) setModeMenu((open) => open === dimension ? null : dimension);
                    else if (control?.cycle_input) void changeMode(dimension, [{ data: control.cycle_input, submit: false }]);
                  }}
                >
                  {label}: {value}
                  {options.length > 0
                    ? <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4"><path d="M6 9l6 6 6-6" /></svg>
                    : canCycle && <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M20 11a8 8 0 1 0-2.3 5.7M20 5v6h-6" /></svg>}
                </button>
                {modeMenu === dimension && options.length > 0 && <div className="dd-menu chat-model-menu" role="menu">
                  {options.map((option) => {
                    const active = state?.value === option.value;
                    return <button
                      type="button"
                      key={option.value}
                      role="menuitem"
                      className={"chat-model-item" + (active ? " is-active" : "")}
                      onClick={() => {
                        setModeMenu(null);
                        void changeMode(dimension, option.inputs, option.value);
                      }}
                    >
                      <span className="chat-model-item-name">{t.chat.modeNames[option.value] ?? option.value}</span>
                      {active && <svg className="chat-model-check" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4"><path d="M4.5 12.5l5 5 10-11" /></svg>}
                    </button>;
                  })}
                </div>}
              </div>;
            });
          })()}
          {history?.contextPct != null && (
            <ContextMeter pct={history.contextPct} window={history.contextWindow} t={t} />
          )}
          <span className="chat-compose-hint">Enter ↵</span>
          <button type="button" className="chat-send-button" aria-label={sending ? t.chat.sending : t.chat.send} onClick={() => void sendPrompt()} disabled={(!prompt.trim() && attachments.length === 0) || sending}>
            {sending
              ? <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="2" /></svg>
              : <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 19V5M6.5 10.5 12 5l5.5 5.5" /></svg>}
          </button>
        </div>
        {sendError && <div className="chat-send-error" role="alert">{sendError}</div>}
      </footer>}
      </div>
    </div>
  );
}
