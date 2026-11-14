/**
 * 会话里切模型 / 模式（任务 20261114-tui-model-switch）。
 *
 * 原先切模型是往终端敲 `/model` 再识别弹出的菜单，CLI 一改画面、会话没起来就卡死（2026-11-14 leader
 * 手机实拍「启动被卡住」）。现在清单预先列好（useModelPresets 读 CLI 配置 / 自带命令），选中后交给后端
 * 按插件声明的机制生效（会话服务 / 带参重启接回 / TUI 对话框 / 未建会话重开），不写终端识别菜单。
 *
 * 按会话状态分派，判定与 Lead 送达同一套（activityTone 的 tone：running = 在跑，pending = 有待批审批）：
 * - 在跑、有待批审批、或上一次切换还在路上：不写终端、不重启，排到宿主按会话记的队列里（换会话、刷新
 *   手机页都还在，后选的合并进同一份），界面一直说「这一轮结束后切换到 X」，回到空闲时自动执行（做空 M3）。
 *   往终端打字在有审批时会替用户批准审批，重启会打断这一轮（research-kimi-gemini §2.4/§3.4 实测）。
 * - 空闲：立刻执行。带参重启的先把 TUI 输入框里没发的字搬进对话页（做空 H2），并带上会话当前的实际值
 *   （transcript 与底栏读到的），后端按它拼参数，不按存档回放（做空 H1）。
 */
import { useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  applySessionSettings,
  queueSessionSettings,
  queuedSessionSettings,
  recordSessionSettings,
  sessionLiveSettings,
  takeQueuedSessionSettings,
  type ChatHistory,
  type LiveSettings,
  type ModelPreset,
  type ModeScreenMarker,
  type SessionTuiState,
  type SettingsOutcome,
} from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";

export type SettingsRequest = {
  /** 模型 id（清单里的），带显示名供乐观更新与提示。 */
  model?: { id: string; label: string; effort?: string | null };
  /** 启动选项栏 id → 档 id。 */
  modes?: Record<string, string>;
  /** 提示里说「切换到 X」用的名字。 */
  label: string;
  /** 宿主队列里标的等待条件：lead = Lead 关卡答复并续跑后（重启会杀掉会话内等待器，做空 N3）；
   *  open = 会话打开、服务 / TUI 就绪后由宿主补设（做空 N6/R5）；ready = 对话页执行时会话服务或 TUI
   *  还没就绪，交给宿主等就绪再执行（做空 R5）。有它时只由宿主执行。 */
  waitFor?: "lead" | "open" | "ready";
  /** 要往终端里打字才能切的项（Kimi `/plan on`、Claude `/model`、循环快捷键等）：运行中排进同一个队列、
   *  显示同一套提示，回合结束由对话页照原样执行（宿主不往终端打字，见 session_settings run_queued）。 */
  terminal?: TerminalAction[];
};

/** 一条要往终端里打字的切换（运行中排队用，参数原样存，执行时不再查表）。 */
export type TerminalAction =
  | { kind: "mode"; dimension: string; value: string; inputs: { data: string; submit: boolean }[]; cycleInput?: string | null; markers?: ModeScreenMarker[] }
  | { kind: "model"; preset: ModelPreset }
  | { kind: "effort"; level: string };

/** 同一件事（同一栏的模式、模型、思考档）后选的覆盖先选的。 */
function terminalKey(action: TerminalAction): string {
  return action.kind === "mode" ? `mode:${action.dimension}` : action.kind;
}

export function mergeTerminal(prev: TerminalAction[] | undefined, next: TerminalAction[] | undefined): TerminalAction[] | undefined {
  if (!prev?.length) return next;
  if (!next?.length) return prev;
  const keys = new Set(next.map(terminalKey));
  return [...prev.filter((action) => !keys.has(terminalKey(action))), ...next];
}

/** 去掉终端项之后还要经后端切的部分；没有就 null。 */
function withoutTerminal(request: SettingsRequest): SettingsRequest | null {
  const terminalDims = new Set((request.terminal ?? []).flatMap((action) => action.kind === "mode" ? [action.dimension] : []));
  const modes = Object.fromEntries(Object.entries(request.modes ?? {}).filter(([dimension]) => !terminalDims.has(dimension)));
  if (!request.model && Object.keys(modes).length === 0) return null;
  const { terminal: _terminal, ...rest } = request;
  return { ...rest, modes };
}

/** 「此刻还不能动、等一会儿就行」的失败原因：放回宿主队列等就绪，不丢这次选择（做空 R5）。 */
const RETRY_WHEN_READY = new Set([
  "settings/server_not_ready",
  "settings/busy_now",
  "settings/background_busy",
  "lead/opencode_tui_not_ready",
  "lead/opencode_busy",
]);

/** 「输入框里有字」的失败原因：不重启的路线（会话服务 / TUI 对话框）要求输入框是空的。Meowo 自己把草稿
 *  清掉并搬进对话页输入框，再切一次，不叫用户去终端清。 */
const COMPOSER_BUSY = new Set(["settings/composer_not_empty", "lead/opencode_composer_not_empty"]);

/** 后端明确没动进程的结果：只有这些才把搬进对话页的草稿撤回（做空 R3）。出错、next_open（进程恰好
 *  没了）、restart_stuck 之类都可能已经杀了进程，TUI 里的原稿跟着没了，对话页这份必须留着。 */
const PROCESS_UNTOUCHED = new Set<SettingsOutcome["kind"]>(["applied", "queued", "not_now", "unsupported", "needs_new_session"]);

/** TUI 输入框此刻：空 / 有草稿已搬进对话页 / 有字没搬（不重启的路线不搬，做空 N9）/ 认不出。 */
export type DraftRescue = "empty" | "moved" | "occupied" | "unknown";

/** 调后端：同步抛错、返回 undefined（mock 被 reset）、异步失败都落到 fallback，不留未处理的 rejection（做空 N8）。 */
async function attempt<T>(run: () => Promise<T> | T | undefined, fallback: T): Promise<T> {
  try {
    const value = await run();
    return value === undefined ? fallback : value;
  } catch {
    return fallback;
  }
}

export interface SessionSettingsDeps {
  sessionId: number;
  /** activityTone 阶梯的结果（sessionTone）。 */
  tone: string | null;
  history: ChatHistory | null;
  setHistory: Dispatch<SetStateAction<ChatHistory | null>>;
  /** exit：错误条上的出路（重试 / 看终端画面）。 */
  setSendError: (message: string, exit?: { retry?: () => void; terminal?: boolean }) => void;
  /** 未建会话重开后切到新的临时 id（草稿跟着走）。 */
  moveTo: (sessionId: number) => void;
  /** 读 TUI 输入框；`move` 为真且有字时搬进对话页输入框（要重启时 Meowo 先把用户内容保住），
   *  不重启的路线只读不搬，免得对话页与 TUI 各一份。 */
  rescueDraft: (move: boolean) => Promise<DraftRescue>;
  /** 这一家的终端输入框认不认得出（插件声明了输入框规格）。认不出的（旧版本 CLI）排着的终端项不等
   *  「输入框认得出」这个条件，直接执行；缺省按认得出。 */
  readsComposer?: () => boolean;
  /** 把 TUI 输入框里的草稿清掉并搬进对话页输入框（不重启的路线遇到「输入框里有字」时用）。 */
  takeDraft?: () => Promise<DraftRescue>;
  /** 执行一条排着的终端项（对话页照原样打字切换，见 SettingsRequest.terminal）。 */
  applyTerminal?: (action: TerminalAction) => Promise<void>;
  /** 搬了草稿最后却没重启（排进队列、被拒、出错）：把搬进对话页的那份撤回，TUI 里原稿还在，免得发两遍。 */
  undoRescue?: () => void;
  /** 这次请求会不会重启 / 重开 TUI 进程（带参重启机制、未建会话重开）。 */
  restarts: (request: SettingsRequest) => boolean;
  /** 会话此刻的实际值：栏 id → 档 id，`model` = 当前模型 id。transcript 与底栏读到的，读不到的不写。 */
  readCurrent: () => Promise<Record<string, string>>;
  /** 未认领会话的第一条消息已发出：此时重开会掐掉第一轮（做空 L4）。 */
  firstTurnPending: boolean;
  /** 会话够不着服务（外部终端、服务没起来）时的模型兜底：旧的终端菜单路线。 */
  fallbackModel: (id: string) => void;
  /** 同上，模式的兜底（OpenCode agent 退回 Tab 循环）；返回 false = 没有兜底。 */
  fallbackMode: (dimension: string, value: string) => boolean;
  /** 清单 id → 显示名（同步当前模型时用）。 */
  modelLabel: (id: string) => string;
}

type RestartOption = { id: string; switch?: string | null; choices?: readonly { id: string; args: readonly string[] }[] };

/** 这次请求会不会重启 / 重开 TUI 进程。按声明表判断，不按 agent 身份分支（架构守卫）：
 *  - 还没建会话（临时 id）：重开；
 *  - 模型的切换机制是带参重启接回；
 *  - 某一栏声明了「重启生效」；
 *  - 会话服务栏从别的档切进启动参数带 `--sandbox workspace-write` 的档：进程若没带 Windows 沙箱参数，后端改走
 *    带参接回（做空第五轮 P3）。前端看不到进程参数，按「可能重启」先把 TUI 草稿搬走；已在这类档里互切不算。 */
export function requestRestarts(request: SettingsRequest, context: {
  sessionId: number;
  modelSwitch: string | null | undefined;
  options: readonly RestartOption[] | null | undefined;
  currentModes: readonly { dimension: string; value: string }[] | null | undefined;
}): boolean {
  if (context.sessionId < 0) return true;
  if (request.model && context.modelSwitch === "restart_resume") return true;
  return Object.entries(request.modes ?? {}).some(([dimension, value]) => {
    const option = context.options?.find((candidate) => candidate.id === dimension);
    if (option?.switch === "restart") return true;
    const choices = option?.switch === "server" ? option.choices : undefined;
    const workspaceWrite = (id: string | undefined) => !!choices?.find((choice) => choice.id === id)?.args.join(" ").includes("--sandbox workspace-write");
    return workspaceWrite(value) && !workspaceWrite(context.currentModes?.find((mode) => mode.dimension === dimension)?.value);
  });
}

/** 在跑或有待批审批：不能动终端，也不能重启。 */
export function mustDefer(tone: string | null): boolean {
  return tone === "running" || tone === "pending";
}

/** 合并两次请求：后选的覆盖先选的（运行中连点两次只执行最后的选择）。 */
export function mergeRequests(prev: SettingsRequest | null, next: SettingsRequest): SettingsRequest {
  if (!prev) return next;
  const terminal = mergeTerminal(prev.terminal, next.terminal);
  return {
    model: next.model ?? prev.model,
    modes: { ...(prev.modes ?? {}), ...(next.modes ?? {}) },
    label: next.label,
    ...(terminal?.length ? { terminal } : {}),
  };
}

function asRequest(value: unknown): SettingsRequest | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Partial<SettingsRequest>;
  if (typeof raw.label !== "string") return null;
  const waitFor = raw.waitFor === "lead" || raw.waitFor === "open" || raw.waitFor === "ready" ? raw.waitFor : undefined;
  const terminal = Array.isArray(raw.terminal) && raw.terminal.length > 0 ? raw.terminal : undefined;
  return { model: raw.model ?? undefined, modes: raw.modes ?? undefined, label: raw.label, ...(waitFor ? { waitFor } : {}), ...(terminal ? { terminal } : {}) };
}

export function useSessionSettings(deps: SessionSettingsDeps) {
  const t = useT();
  const depsRef = useRef(deps);
  depsRef.current = deps;
  const [queued, setQueued] = useState<{ sessionId: number; request: SettingsRequest } | null>(null);
  const [applying, setApplying] = useState(false);
  const [nextOpen, setNextOpen] = useState<number | null>(null);
  const runningRef = useRef(false);
  // 一次性的说明（Codex 为沙箱带参重开会话）：下一次选择或换会话时清掉，8 秒后自动消失。
  const [info, setInfo] = useState<{ sessionId: number; text: string } | null>(null);
  useEffect(() => {
    if (!info) return;
    const timer = setTimeout(() => setInfo(null), 8000);
    return () => clearTimeout(timer);
  }, [info]);
  // 宿主执行的排队结果事件不带提示名：用队列里排着的那份。
  const pendingLabelRef = useRef<string | null>(null);

  const optimistic = (request: SettingsRequest) => {
    depsRef.current.setHistory((current) => {
      if (!current) return current;
      const agentModes = [...current.agentModes];
      for (const [dimension, value] of Object.entries(request.modes ?? {})) {
        const index = agentModes.findIndex((mode) => mode.dimension === dimension);
        if (index >= 0) agentModes[index] = { dimension, value };
        else agentModes.push({ dimension, value });
      }
      return { ...current, agentModes, model: request.model?.label ?? current.model };
    });
  };

  /** 排进宿主的队列（合并后的结果回来作为界面显示）。 */
  const enqueue = async (sessionId: number, request: SettingsRequest) => {
    const merged = asRequest(await attempt<unknown>(() => queueSessionSettings(sessionId, request), request)) ?? request;
    if (depsRef.current.sessionId === sessionId) setQueued({ sessionId, request: merged });
  };

  /** 重读宿主队列里这个会话排着的（宿主收进队列、改了等待条件之后）。 */
  const refreshQueued = async (sessionId: number) => {
    const pending = asRequest(await attempt<unknown>(() => queuedSessionSettings(sessionId), null));
    if (depsRef.current.sessionId === sessionId) setQueued(pending ? { sessionId, request: pending } : null);
  };
  const refreshQueuedRef = useRef(refreshQueued);
  refreshQueuedRef.current = refreshQueued;

  const apply = async (request: SettingsRequest) => {
    const actions = request.terminal ?? [];
    if (actions.length === 0) { await applyBackend(request); return; }
    // 混合条目：先做经后端的部分（重启 / 会话服务 / 模型），再做终端项（Kimi /plan、Claude /model 等），
    // 与宿主执行线程同一顺序（最终做空 P6）。后端那部分又排进了队列（Lead 在等、服务没就绪……），终端项
    // 跟着一起等，不抢先打字。
    const { sessionId } = depsRef.current;
    if (runningRef.current) { await enqueue(sessionId, request); return; }
    const rest = withoutTerminal(request);
    const terminalPart: SettingsRequest = {
      label: request.label,
      terminal: actions,
      modes: Object.fromEntries(actions.flatMap((action) => action.kind === "mode" ? [[action.dimension, action.value]] : [])),
    };
    if (rest) {
      const result = await applyBackend(rest);
      // 后端那部分排进了队列：终端项跟着等。失败了：终端项也不做，整条按失败提示（第六轮 L3：不只生效一半）。
      if (result === "deferred" && depsRef.current.sessionId === sessionId) await enqueue(sessionId, terminalPart);
      if (result !== "done") return;
    }
    if (depsRef.current.sessionId !== sessionId) return;
    runningRef.current = true;
    setApplying(true);
    try {
      // 刚带参重启过的话新 TUI 可能还没起来：等输入框认得出再打字（最多约 10 秒）。等不到不悄悄丢掉
      // 这次选择：错误条说明没切成，「重试」再执行同一条。
      if (!(await composerReady())) {
        const retry = () => { if (depsRef.current.sessionId === sessionId) void apply(terminalPart); };
        depsRef.current.setSendError(t.chat.settingsTerminalNotReady, { retry, terminal: true });
        return;
      }
      for (const action of actions) await attempt(() => depsRef.current.applyTerminal?.(action), undefined);
    } finally {
      runningRef.current = false;
      setApplying(false);
    }
  };

  /** 终端输入框认得出（空或有字都算）就算就绪。这一家没有输入框规格时没法认，不等：此前 Claude 没声明
   *  规格，排着的模型 / 思考档在回合结束后空等 10 秒再报错丢弃（走查输入 A1）。 */
  const composerReady = async () => {
    if (depsRef.current.readsComposer && !depsRef.current.readsComposer()) return true;
    for (let attempt_ = 0; attempt_ < 20; attempt_ += 1) {
      const state = await attempt<DraftRescue>(() => depsRef.current.rescueDraft(false), "unknown");
      if (state !== "unknown") return true;
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
    return false;
  };

  /** 经后端切。返回 false 表示这次没做成、已排进队列等下一次（宿主 queued / not_now 排队 / 等就绪）。 */
  /** 经后端切：done = 做成了；deferred = 没做成、已排进队列等下一次（宿主 queued / not_now 排队 / 等就绪）；
   *  failed = 出错或被拒（已提示）。 */
  const applyBackend = async (request: SettingsRequest, draftTaken = false): Promise<"done" | "deferred" | "failed"> => {
    const { sessionId } = depsRef.current;
    if (runningRef.current) { await enqueue(sessionId, request); return "deferred"; }
    runningRef.current = true;
    setApplying(true);
    let moved = false;
    let untouched = false;
    let retryAfterTake = false;
    let result: "done" | "deferred" | "failed" = "done";
    const fail = (message: string) => { result = "failed"; depsRef.current.setSendError(message); };
    try {
      const restarting = depsRef.current.restarts(request);
      if (restarting && depsRef.current.firstTurnPending) {
        fail(t.chat.settingsFirstTurn);
        return result;
      }
      // 重启会杀掉 TUI 进程：先把输入框里没发的字搬进对话页；认不出输入框就不重启。
      // 不重启的路线（会话服务 / TUI 对话框）不搬：原稿留在 TUI 里，对话框路线见字就不动手（做空 N9）。
      const rescue = await attempt<DraftRescue>(() => depsRef.current.rescueDraft(restarting), "unknown");
      if (restarting && rescue !== "empty" && rescue !== "moved") {
        fail(t.chat.settingsComposerUnknown);
        return result;
      }
      moved = rescue === "moved";
      const composerEmpty = rescue === "empty" || rescue === "moved";
      const current = await attempt<Record<string, string>>(() => depsRef.current.readCurrent(), {});
      const tui: SessionTuiState = {
        idle: !mustDefer(depsRef.current.tone),
        composerEmpty,
        approvalPending: depsRef.current.tone === "pending",
      };
      const outcome: SettingsOutcome = await applySessionSettings({
        sessionId,
        model: request.model?.id ?? null,
        effort: request.model?.effort ?? null,
        modes: request.modes,
        current,
        tui,
      });
      untouched = PROCESS_UNTOUCHED.has(outcome.kind);
      if (depsRef.current.sessionId !== sessionId) return result;
      switch (outcome.kind) {
        case "applied":
        case "restarted":
          optimistic(request);
          // Codex 会话内首次切进要沙箱的档：后端带沙箱参数重开了会话（第五轮 P3），说清楚为什么重开。
          if (outcome.kind === "restarted" && outcome.reason === "settings/sandbox_reopen") setInfo({ sessionId, text: t.chat.settingsSandboxReopen(request.label) });
          // 只切 agent 时 OpenCode 换成了该 agent 记住的模型：后端当场读回并钉在宿主存档里（之后的快照、
          // 菜单同步都以它为准，最终做空 P2），这里先立刻显示。
          if (outcome.model && !request.model) {
            const readBack = outcome.model;
            depsRef.current.setHistory((current) => current && current.sessionId === sessionId ? { ...current, model: readBack } : current);
          }
          // TUI 对话框路线：回车后底栏没认出新值（终端太窄被截断等）不算失败，下一条消息再核对。
          if (outcome.kind === "applied" && !outcome.confirmed) depsRef.current.setSendError(t.chat.settingsUnconfirmed);
          break;
        case "reopened":
          depsRef.current.moveTo(outcome.sessionId);
          break;
        case "next_open":
          // 会话没在跑：选择记成下次打开的启动参数，如实说（做空 M4）。
          setNextOpen(sessionId);
          break;
        case "needs_new_session":
          fail(t.chat.settingsNeedsNewSession);
          break;
        case "unsupported": {
          if (request.model) depsRef.current.fallbackModel(request.model.id);
          const unhandled = Object.entries(request.modes ?? {}).filter(([dimension, value]) => !depsRef.current.fallbackMode(dimension, value));
          if (unhandled.length > 0) depsRef.current.setSendError(t.chat.settingsModeUnsupported);
          break;
        }
        case "queued":
          // 宿主自己收进了队列、标了等什么（Lead 在等关卡答复 / 会话没在运行、打开后经服务补设）；
          // 把这次请求合并进去补上提示名，合并结果带着宿主标的 waitFor。
          result = "deferred";
          await enqueue(sessionId, request);
          break;
        case "not_now":
          // 服务端复核说此刻不能动：刚开始跑一轮的排到这一轮之后；服务 / TUI 没就绪的交给宿主等就绪；
          // 其余（输入框有字等）说原因。
          if (mustDefer(depsRef.current.tone)) { result = "deferred"; await enqueue(sessionId, request); }
          else if (RETRY_WHEN_READY.has(outcome.reason ?? "")) { result = "deferred"; await enqueue(sessionId, { ...request, waitFor: "ready" }); }
          // 输入框里有字：清掉并搬进对话页输入框，再切一次（只试一次，清不掉才说原因）。
          else if (COMPOSER_BUSY.has(outcome.reason ?? "") && !draftTaken && depsRef.current.takeDraft
            && ["moved", "empty"].includes(await attempt<DraftRescue>(() => depsRef.current.takeDraft?.(), "unknown"))) retryAfterTake = true;
          else fail(formatBackendError(outcome.reason ?? "", t.locale));
          break;
      }
    } catch (error) {
      // 会话服务还没握手（刚打开的 Codex 等）：放回宿主队列等就绪，不丢这次选择（做空 R5）。
      if (typeof error === "string" && RETRY_WHEN_READY.has(error)) { result = "deferred"; await enqueue(sessionId, { ...request, waitFor: "ready" }); }
      else {
        result = "failed";
        if (depsRef.current.sessionId === sessionId) depsRef.current.setSendError(formatBackendError(error, t.locale));
      }
    } finally {
      if (moved && untouched && depsRef.current.sessionId === sessionId) depsRef.current.undoRescue?.();
      runningRef.current = false;
      setApplying(false);
    }
    if (retryAfterTake && depsRef.current.sessionId === sessionId) return applyBackend(request, true);
    return result;
  };

  /** 用户选了一项：能做就做；在跑、有审批、上一次还在路上的，排到这一轮之后。 */
  const request = (next: SettingsRequest) => {
    const { sessionId, tone } = depsRef.current;
    if (mustDefer(tone) || runningRef.current) {
      void enqueue(sessionId, next);
      return;
    }
    void apply(next);
  };

  // 进会话时取宿主队列里排着的（换会话、刷新后回来照样显示与执行）。
  useEffect(() => {
    setQueued(null);
    setNextOpen(null);
    setInfo(null);
    const sessionId = deps.sessionId;
    void refreshQueued(sessionId);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- refreshQueued 只读 ref 与 setter
  }, [deps.sessionId]);

  // 宿主在回合结束时自己执行了排队的切换（不论对话页在看哪个会话）：清掉提示，结果按种类说一句。
  useEffect(() => {
    let un: (() => void) | undefined;
    let cancelled = false;
    listen<{ sessionId: number; kind: string; reason: string | null }>("session-settings-applied", ({ payload }) => {
      if (payload.sessionId !== depsRef.current.sessionId) return;
      // 宿主改了等待条件（如回合结束时发现 Lead 在等关卡答复），重读显示。
      if (payload.kind === "queued") { void refreshQueuedRef.current(payload.sessionId); return; }
      setQueued(null);
      // 排太久（30 分钟）出队：说一声，不让选择悄悄消失（做空 N5）。会话没了的不用说。
      if (payload.kind === "expired") depsRef.current.setSendError(t.chat.settingsQueueExpired);
      else if (payload.kind === "next_open") setNextOpen(payload.sessionId);
      else if (payload.kind === "needs_new_session") depsRef.current.setSendError(t.chat.settingsNeedsNewSession);
      else if ((payload.kind === "failed" || payload.kind === "not_now") && payload.reason) depsRef.current.setSendError(formatBackendError(payload.reason, t.locale));
      else if (payload.kind === "applied" || payload.kind === "restarted") {
        if (payload.reason === "settings/sandbox_reopen") setInfo({ sessionId: payload.sessionId, text: t.chat.settingsSandboxReopen(pendingLabelRef.current ?? "") });
        void syncLiveRef.current();
      }
    }).then((fn) => { if (cancelled) fn(); else un = fn; }).catch(() => {});
    return () => { cancelled = true; un?.(); };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- 只订阅一次，回调读 ref
  }, []);

  // 回到空闲（tone 离开 running / pending）、上一次切换落地后，执行排着的切换。
  useEffect(() => {
    if (!queued || queued.sessionId !== deps.sessionId) return;
    // 标了等待条件的只由宿主执行（它看得到 Lead 等待器与 TUI 握手）；宿主那边取走也会拒绝。
    if (queued.request.waitFor || mustDefer(deps.tone) || applying) return;
    const sessionId = queued.sessionId;
    void attempt<unknown>(() => takeQueuedSessionSettings(sessionId), null).then((value) => {
      const pending = asRequest(value);
      if (!pending || depsRef.current.sessionId !== sessionId) return;
      setQueued(null);
      void apply(pending);
    });
  // eslint-disable-next-line react-hooks/exhaustive-deps -- apply 读 depsRef，只该由 tone / 会话 / 队列变化触发
  }, [deps.tone, deps.sessionId, queued, applying]);

  /// 读会话此刻的模型 / 模式（用户在 TUI 里自己切过的跟上）。回合结束与打开菜单时调，不定时轮询。
  /// 读到的同时写回存档（做空 M5）：之后的恢复、切账号、带参重启按它回放，不再把 TUI 里的选择改回去。
  const syncLive = async () => {
    const { sessionId } = depsRef.current;
    if (sessionId <= 0) return;
    const live = await attempt<LiveSettings | null>(() => sessionLiveSettings(sessionId), null);
    if (live && depsRef.current.sessionId === sessionId && (live.model || live.modes.length > 0)) {
      depsRef.current.setHistory((current) => {
        if (!current || current.sessionId !== sessionId) return current;
        const agentModes = [...current.agentModes];
        for (const mode of live.modes) {
          const index = agentModes.findIndex((existing) => existing.dimension === mode.dimension);
          if (index >= 0) agentModes[index] = mode;
          else agentModes.push(mode);
        }
        return {
          ...current,
          agentModes,
          model: live.model ? depsRef.current.modelLabel(live.model) : current.model,
          effort: live.effort ?? current.effort,
        };
      });
    }
    const current = await attempt<Record<string, string>>(() => depsRef.current.readCurrent(), {});
    if (depsRef.current.sessionId !== sessionId) return;
    const { model, ...modes } = { ...current, ...(live?.model ? { model: live.model } : {}), ...Object.fromEntries((live?.modes ?? []).map((mode) => [mode.dimension, mode.value])) };
    if (model || Object.keys(modes).length > 0) void attempt(() => recordSessionSettings(sessionId, model ?? null, modes), undefined);
  };
  const syncLiveRef = useRef(syncLive);
  syncLiveRef.current = syncLive;
  // 回合结束（running → 空闲）读一次。
  const prevToneRef = useRef(deps.tone);
  useEffect(() => {
    const was = prevToneRef.current;
    prevToneRef.current = deps.tone;
    if (was === "running" && !mustDefer(deps.tone)) void syncLive();
  // eslint-disable-next-line react-hooks/exhaustive-deps -- syncLive 读 depsRef，只由 tone 变化触发
  }, [deps.tone]);

  pendingLabelRef.current = queued?.request.label ?? pendingLabelRef.current;
  const deferredText = (request: SettingsRequest) => request.waitFor === "lead" ? t.chat.settingsDeferredLead(request.label)
    : request.waitFor === "open" ? t.chat.settingsDeferredOpen(request.label)
    : request.waitFor === "ready" ? t.chat.settingsDeferredReady(request.label)
    : t.chat.settingsDeferred(request.label);
  const notice = queued && queued.sessionId === deps.sessionId ? deferredText(queued.request)
    : nextOpen === deps.sessionId ? t.chat.settingsNextOpen
    : info && info.sessionId === deps.sessionId ? info.text
    : null;
  return { request, applying, notice, syncLive, deferredRequest: queued?.sessionId === deps.sessionId ? queued.request : null };
}
