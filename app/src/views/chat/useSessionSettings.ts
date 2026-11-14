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
};

/** TUI 输入框此刻：空 / 有草稿已搬进对话页 / 认不出。 */
export type DraftRescue = "empty" | "moved" | "unknown";

export interface SessionSettingsDeps {
  sessionId: number;
  /** activityTone 阶梯的结果（sessionTone）。 */
  tone: string | null;
  history: ChatHistory | null;
  setHistory: Dispatch<SetStateAction<ChatHistory | null>>;
  setSendError: (message: string) => void;
  /** 未建会话重开后切到新的临时 id（草稿跟着走）。 */
  moveTo: (sessionId: number) => void;
  /** 读 TUI 输入框；有字就搬进对话页输入框（Meowo 先把用户内容保住）。 */
  rescueDraft: () => Promise<DraftRescue>;
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

/** 在跑或有待批审批：不能动终端，也不能重启。 */
export function mustDefer(tone: string | null): boolean {
  return tone === "running" || tone === "pending";
}

/** 合并两次请求：后选的覆盖先选的（运行中连点两次只执行最后的选择）。 */
export function mergeRequests(prev: SettingsRequest | null, next: SettingsRequest): SettingsRequest {
  if (!prev) return next;
  return {
    model: next.model ?? prev.model,
    modes: { ...(prev.modes ?? {}), ...(next.modes ?? {}) },
    label: next.label,
  };
}

function asRequest(value: unknown): SettingsRequest | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Partial<SettingsRequest>;
  return typeof raw.label === "string" ? { model: raw.model ?? undefined, modes: raw.modes ?? undefined, label: raw.label } : null;
}

export function useSessionSettings(deps: SessionSettingsDeps) {
  const t = useT();
  const depsRef = useRef(deps);
  depsRef.current = deps;
  const [queued, setQueued] = useState<{ sessionId: number; request: SettingsRequest } | null>(null);
  const [applying, setApplying] = useState(false);
  const [nextOpen, setNextOpen] = useState<number | null>(null);
  const runningRef = useRef(false);

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
    const merged = asRequest(await queueSessionSettings(sessionId, request).catch(() => request)) ?? request;
    if (depsRef.current.sessionId === sessionId) setQueued({ sessionId, request: merged });
  };

  const apply = async (request: SettingsRequest) => {
    const { sessionId } = depsRef.current;
    if (runningRef.current) { await enqueue(sessionId, request); return; }
    runningRef.current = true;
    setApplying(true);
    try {
      const restarting = depsRef.current.restarts(request);
      if (restarting && depsRef.current.firstTurnPending) {
        depsRef.current.setSendError(t.chat.settingsFirstTurn);
        return;
      }
      // 重启会杀掉 TUI 进程：先把输入框里没发的字搬进对话页；认不出输入框就不重启。
      let composerEmpty = true;
      const rescue = await depsRef.current.rescueDraft().catch((): DraftRescue => "unknown");
      if (rescue === "unknown") {
        if (restarting) {
          depsRef.current.setSendError(t.chat.settingsComposerUnknown);
          return;
        }
        composerEmpty = false;
      }
      const current = await depsRef.current.readCurrent().catch(() => ({}));
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
      if (depsRef.current.sessionId !== sessionId) return;
      switch (outcome.kind) {
        case "applied":
        case "restarted":
          optimistic(request);
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
          depsRef.current.setSendError(t.chat.settingsNeedsNewSession);
          break;
        case "unsupported": {
          if (request.model) depsRef.current.fallbackModel(request.model.id);
          const unhandled = Object.entries(request.modes ?? {}).filter(([dimension, value]) => !depsRef.current.fallbackMode(dimension, value));
          if (unhandled.length > 0) depsRef.current.setSendError(t.chat.settingsModeUnsupported);
          break;
        }
        case "not_now":
          // 服务端复核说此刻不能动：刚开始跑一轮的排到这一轮之后；不在跑却被拒的（后台子任务、输入框有字）说原因。
          if (mustDefer(depsRef.current.tone)) await enqueue(sessionId, request);
          else depsRef.current.setSendError(formatBackendError(outcome.reason ?? "", t.locale));
          break;
      }
    } catch (error) {
      if (depsRef.current.sessionId === sessionId) depsRef.current.setSendError(formatBackendError(error, t.locale));
    } finally {
      runningRef.current = false;
      setApplying(false);
    }
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
    const sessionId = deps.sessionId;
    void queuedSessionSettings(sessionId).then((value) => {
      const pending = asRequest(value);
      if (pending && depsRef.current.sessionId === sessionId) setQueued({ sessionId, request: pending });
    }).catch(() => {});
  }, [deps.sessionId]);

  // 宿主在回合结束时自己执行了排队的切换（不论对话页在看哪个会话）：清掉提示，结果按种类说一句。
  useEffect(() => {
    let un: (() => void) | undefined;
    let cancelled = false;
    listen<{ sessionId: number; kind: string; reason: string | null }>("session-settings-applied", ({ payload }) => {
      if (payload.sessionId !== depsRef.current.sessionId) return;
      setQueued(null);
      if (payload.kind === "next_open") setNextOpen(payload.sessionId);
      else if (payload.kind === "needs_new_session") depsRef.current.setSendError(t.chat.settingsNeedsNewSession);
      else if ((payload.kind === "failed" || payload.kind === "not_now") && payload.reason) depsRef.current.setSendError(formatBackendError(payload.reason, t.locale));
      else if (payload.kind === "applied" || payload.kind === "restarted") void syncLiveRef.current();
    }).then((fn) => { if (cancelled) fn(); else un = fn; }).catch(() => {});
    return () => { cancelled = true; un?.(); };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- 只订阅一次，回调读 ref
  }, []);

  // 回到空闲（tone 离开 running / pending）、上一次切换落地后，执行排着的切换。
  useEffect(() => {
    if (!queued || queued.sessionId !== deps.sessionId) return;
    if (mustDefer(deps.tone) || applying) return;
    const sessionId = queued.sessionId;
    setQueued(null);
    void takeQueuedSessionSettings(sessionId).then((value) => {
      const pending = asRequest(value);
      if (pending && depsRef.current.sessionId === sessionId) void apply(pending);
    }).catch(() => {});
  // eslint-disable-next-line react-hooks/exhaustive-deps -- apply 读 depsRef，只该由 tone / 会话 / 队列变化触发
  }, [deps.tone, deps.sessionId, queued, applying]);

  /// 读会话此刻的模型 / 模式（用户在 TUI 里自己切过的跟上）。回合结束与打开菜单时调，不定时轮询。
  /// 读到的同时写回存档（做空 M5）：之后的恢复、切账号、带参重启按它回放，不再把 TUI 里的选择改回去。
  const syncLive = async () => {
    const { sessionId } = depsRef.current;
    if (sessionId <= 0) return;
    const live = await sessionLiveSettings(sessionId).catch(() => null);
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
    const current = await depsRef.current.readCurrent().catch((): Record<string, string> => ({}));
    if (depsRef.current.sessionId !== sessionId) return;
    const { model, ...modes } = { ...current, ...(live?.model ? { model: live.model } : {}), ...Object.fromEntries((live?.modes ?? []).map((mode) => [mode.dimension, mode.value])) };
    if (model || Object.keys(modes).length > 0) void recordSessionSettings(sessionId, model ?? null, modes).catch(() => {});
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

  const notice = queued && queued.sessionId === deps.sessionId ? t.chat.settingsDeferred(queued.request.label)
    : nextOpen === deps.sessionId ? t.chat.settingsNextOpen
    : null;
  return { request, applying, notice, syncLive, deferredRequest: queued?.sessionId === deps.sessionId ? queued.request : null };
}
