import { useCallback, useEffect, useState } from "react";
import { type WorktreeSetupDto, worktreeSetup, worktreeSetupTrust } from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import { AppModal } from "../AppModal";

/// 会话页顶部的 setup 进度条（包 5）：在新 worktree 里开的会话，setup 在后台跑，会话不等它。
/// 这里能看日志、跳过、重跑；脚本没信任过时给「查看脚本」，在确认框里看原文后再跑。
/// 按会话 cwd 查任务：不是 Meowo 建的 worktree（或 Meowo 重启过）后端回 null，整条不显示。

/** 运行中 / 待确认时每 1.5s 拉一次；只在窗口可见时拉。桌面另有事件推送，手机只靠轮询。 */
const POLL_MS = 1500;

const SOURCE_FILE: Record<string, string> = {
  meowo: ".meowo/worktree.json",
  cursor: ".cursor/worktrees.json",
  conductor: "conductor.json",
};

export function WorktreeSetupBar({ cwd }: { cwd: string | null }) {
  const t = useT();
  const [setup, setSetup] = useState<WorktreeSetupDto | null>(null);
  const [logOpen, setLogOpen] = useState(false);
  const [trustOpen, setTrustOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // 用户关掉的是「这一轮」：重跑后状态变了还会再出来。
  const [dismissed, setDismissed] = useState<string | null>(null);

  const refresh = useCallback(() => {
    if (!cwd) return Promise.resolve();
    return worktreeSetup(cwd)
      .then((dto) => setSetup(dto))
      .catch(() => { /* 查不到就保持原样：进度条是附加信息，不为它报错 */ });
  }, [cwd]);

  useEffect(() => {
    setSetup(null);
    setLogOpen(false);
    setDismissed(null);
    setError(null);
    void refresh();
  }, [refresh]);

  const live = setup?.state === "running" || setup?.state === "needs_trust";
  useEffect(() => {
    if (!live && !logOpen) return;
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") void refresh();
    }, POLL_MS);
    return () => window.clearInterval(timer);
  }, [live, logOpen, refresh]);
  useTauriEvent<string>("worktree-setup-changed", (event) => {
    if (setup && event.payload === setup.taskId) void refresh();
  });

  if (!setup) return null;
  // 没配 setup 的仓库：没什么可显示的。
  if (setup.state === "skipped" && setup.reason === "none") return null;
  const roundKey = `${setup.state}:${setup.startedAt ?? 0}`;
  if (dismissed === roundKey) return null;

  const act = async (action: "rerun" | "skip" | "stop") => {
    setBusy(true);
    setError(null);
    try {
      setSetup(await worktreeSetup(setup.path, action));
    } catch (e) {
      setError(formatBackendError(e, t.locale));
    } finally {
      setBusy(false);
    }
  };
  const trust = async () => {
    if (!setup.configHash) return;
    setBusy(true);
    setError(null);
    try {
      setSetup(await worktreeSetupTrust(setup.path, setup.configHash));
      setTrustOpen(false);
    } catch (e) {
      setError(formatBackendError(e, t.locale));
      // 配置在确认框打开期间被改了：拿新的那份重新给用户看。
      void refresh();
    } finally {
      setBusy(false);
    }
  };

  const state = setup.state;
  const label = t.wtnew.state[state] ?? state;
  const detail = [
    state === "failed" && setup.exitCode != null ? t.wtnew.exitCode(setup.exitCode) : null,
    setup.reason ? t.wtnew.reason[setup.reason] ?? null : null,
    state === "running" && setup.runner ? t.wtnew.runner(setup.runner) : null,
  ].filter(Boolean).join(" · ");
  const tone = state === "failed" || state === "timeout" ? "is-err"
    : state === "needs_trust" ? "is-warn"
    : state === "ok" ? "is-ok"
    : "";
  const canRerun = state === "failed" || state === "timeout" || state === "stopped"
    || (state === "skipped" && setup.reason === "user") || state === "ok";
  const canSkip = state === "running" || state === "needs_trust" || state === "failed" || state === "timeout";
  const settled = state !== "running" && state !== "needs_trust";

  return (
    <div className={"wt-setup " + tone} data-testid="wt-setup-bar" data-state={state} role="status">
      <div className="wt-setup-row">
        <span className="wt-setup-label">{label}</span>
        {detail && <span className="wt-setup-detail">{detail}</span>}
        <span className="wt-setup-actions">
          {state === "needs_trust" && (
            <button type="button" className="wt-opt-btn" data-testid="wt-setup-review" disabled={busy} onClick={() => setTrustOpen(true)}>{t.wtnew.review}</button>
          )}
          {state !== "needs_trust" && (
            <button type="button" className="wt-opt-btn" data-testid="wt-setup-log" onClick={() => { setLogOpen((open) => !open); void refresh(); }}>
              {logOpen ? t.wtnew.hideLog : t.wtnew.viewLog}
            </button>
          )}
          {canRerun && (
            <button type="button" className="wt-opt-btn" data-testid="wt-setup-rerun" disabled={busy} onClick={() => void act("rerun")}>{t.wtnew.rerun}</button>
          )}
          {canSkip && (
            <button type="button" className="wt-opt-btn" data-testid="wt-setup-skip" disabled={busy} onClick={() => void act("skip")}>{t.wtnew.skip}</button>
          )}
          {settled && (
            <button type="button" className="wt-opt-btn" data-testid="wt-setup-dismiss" aria-label={t.wtnew.dismiss} data-tip={t.wtnew.dismiss} onClick={() => setDismissed(roundKey)}>×</button>
          )}
        </span>
      </div>
      {state === "running" && <div className="wt-setup-progress" aria-hidden="true"><span /></div>}
      {error && <div className="wt-setup-error" role="alert">{error}</div>}
      {logOpen && (
        <pre className="wt-setup-log" data-testid="wt-setup-log-text">{setup.logTail || t.wtnew.noLog}</pre>
      )}
      {trustOpen && (
        <TrustDialog setup={setup} busy={busy} error={error} onRun={() => void trust()} onSkip={() => { setTrustOpen(false); void act("skip"); }} onClose={() => setTrustOpen(false)} />
      )}
    </div>
  );
}

/// setup 脚本的信任确认：原样展示要跑的脚本，以及配置里会注入 setup 进程的环境变量（hash 覆盖整份配置，
/// NODE_OPTIONS 这类变量能让无害的脚本加载任意代码，只给看脚本等于盲签，做空审查实测）。走 AppModal（Esc 层 escLayers、焦点陷阱），
/// Esc 只关确认框，不会被窗口级「Esc=拒绝审批」当成拒绝。
export function TrustDialog({ setup, busy, error, onRun, onSkip, onClose }: {
  setup: WorktreeSetupDto;
  busy: boolean;
  error: string | null;
  onRun: () => void;
  onSkip: () => void;
  onClose: () => void;
}) {
  const t = useT();
  const file = SOURCE_FILE[setup.configSource ?? "meowo"] ?? ".meowo/worktree.json";
  return (
    <AppModal label={t.wtnew.trustTitle} className="wt-trust" onClose={onClose}>
      <div className="wt-trust-body" data-testid="wt-trust-dialog">
        <h2 className="wt-trust-title">{t.wtnew.trustTitle}</h2>
        <p className="wt-trust-text">{t.wtnew.trustSource(file)}</p>
        <pre className="wt-trust-script" data-testid="wt-trust-script">{setup.script ?? ""}</pre>
        {(setup.setupEnv?.length ?? 0) > 0 && (
          <>
            <p className="wt-trust-text">{t.wtnew.trustEnv}</p>
            <pre className="wt-trust-script" data-testid="wt-trust-env">{setup.setupEnv!.map(([k, v]) => `${k}=${v}`).join("\n")}</pre>
          </>
        )}
        {setup.runner && <p className="wt-trust-text">{t.wtnew.runner(setup.runner)}</p>}
        <p className="wt-trust-note">{t.wtnew.trustNote}</p>
        {error && <p className="wt-setup-error" role="alert">{error}</p>}
        <div className="wt-trust-actions">
          <button type="button" className="wt-opt-btn" data-testid="wt-trust-skip" disabled={busy} onClick={onSkip}>{t.wtnew.skip}</button>
          <button type="button" className="wt-opt-btn btn-primary" data-testid="wt-trust-run" disabled={busy || !setup.configHash} onClick={onRun}>{t.wtnew.trustRun}</button>
        </div>
      </div>
    </AppModal>
  );
}
