/**
 * 会话里切模型 / 模式（任务 20261114-tui-model-switch）。
 *
 * 原先切模型是往终端敲 `/model` 再识别弹出的菜单，CLI 一改画面、会话没起来就卡死（2026-11-14 leader
 * 手机实拍「启动被卡住」）。现在清单预先列好（useModelPresets 读 CLI 配置 / 自带命令），选中后交给后端
 * 按插件声明的机制生效（会话服务 / 带参重启接回 / TUI 对话框 / 未建会话重开），不写终端识别菜单。
 *
 * 按会话状态分派，判定与 Lead 送达同一套（activityTone 的 tone：running = 在跑，pending = 有待批审批）：
 * - 在跑或有待批审批：不写终端、不重启，记成「这一轮结束后切换」，tone 回到空闲时自动执行。
 *   往终端打字在有审批时会替用户批准审批，重启会打断这一轮（research-kimi-gemini §2.4/§3.4 实测）。
 * - 空闲 / 还没建会话：立刻执行。
 */
import { useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { applySessionSettings, type ChatHistory, type SessionTuiState, type SettingsOutcome } from "../../api";
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

export interface SessionSettingsDeps {
  sessionId: number;
  /** activityTone 阶梯的结果（sessionTone）。 */
  tone: string | null;
  history: ChatHistory | null;
  setHistory: Dispatch<SetStateAction<ChatHistory | null>>;
  setSendError: (message: string) => void;
  /** 未建会话重开后切到新的临时 id（草稿跟着走）。 */
  moveTo: (sessionId: number) => void;
  /** 读 TUI 输入框是否为空（OpenCode 对话框路线的调用约定）。 */
  composerEmpty: () => Promise<boolean>;
  /** 会话够不着服务（Lead 未装、外部终端）时的模型兜底：旧的终端菜单路线。 */
  fallbackModel: (id: string) => void;
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

export function useSessionSettings(deps: SessionSettingsDeps) {
  const t = useT();
  const depsRef = useRef(deps);
  depsRef.current = deps;
  const [deferred, setDeferred] = useState<{ sessionId: number; request: SettingsRequest } | null>(null);
  const [applying, setApplying] = useState(false);
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

  const apply = async (request: SettingsRequest) => {
    const { sessionId, history } = depsRef.current;
    if (runningRef.current) return;
    runningRef.current = true;
    setApplying(true);
    try {
      const current: Record<string, string> = {};
      for (const mode of history?.agentModes ?? []) current[mode.dimension] = mode.value;
      const tui: SessionTuiState = {
        idle: !mustDefer(depsRef.current.tone),
        composerEmpty: await depsRef.current.composerEmpty().catch(() => false),
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
          break;
        case "reopened":
          depsRef.current.moveTo(outcome.sessionId);
          break;
        case "needs_new_session":
          depsRef.current.setSendError(t.chat.settingsNeedsNewSession);
          break;
        case "unsupported":
          if (request.model) depsRef.current.fallbackModel(request.model.id);
          else depsRef.current.setSendError(t.chat.settingsModeUnsupported);
          break;
        case "not_now":
          // 服务端复核说此刻不能动（刚开始跑一轮、输入框有字…）：排到这一轮之后再试；
          // 不在跑却被拒的（输入框有字），把原因告诉用户。
          if (mustDefer(depsRef.current.tone)) setDeferred({ sessionId, request });
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

  /** 用户选了一项：能做就做，不能做就排到这一轮之后。 */
  const request = (next: SettingsRequest) => {
    const { sessionId, tone } = depsRef.current;
    if (mustDefer(tone)) {
      setDeferred((prev) => ({ sessionId, request: mergeRequests(prev?.sessionId === sessionId ? prev.request : null, next) }));
      return;
    }
    void apply(next);
  };

  // 回合结束（tone 离开 running / pending）时执行排着的切换。换了会话的作废。
  useEffect(() => {
    if (!deferred) return;
    if (deferred.sessionId !== deps.sessionId) { setDeferred(null); return; }
    if (mustDefer(deps.tone)) return;
    const pending = deferred.request;
    setDeferred(null);
    void apply(pending);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- apply 读 depsRef，只该由 tone / 会话变化触发
  }, [deps.tone, deps.sessionId, deferred]);

  const notice = deferred && deferred.sessionId === deps.sessionId ? t.chat.settingsDeferred(deferred.request.label) : null;
  return { request, applying, notice, deferredRequest: deferred?.sessionId === deps.sessionId ? deferred.request : null };
}
