/// Lead 工作流（停靠栏的 Lead 入口 + 关卡卡片）。
///
/// 数据只来自 `lead_task_snapshot`：后端按约定读 `.lead/active` 指向的任务目录（手机端也走
/// 同一命令，远程桥放行理由见 remote.rs「例外（2026-11-09）」）。前端不直接读文件。
///
/// 关卡卡片的作答只写 gate.json（answer_lead_gate），答复由 agent 侧的 asyncRewake 后台
/// hook 看到后唤醒模型——卡片不往终端写字，所以 Meowo 重启、手机断线都不会丢答复。

import { memo, useCallback, useEffect, useRef, useState } from "react";
import { answerLeadGate, leadTaskSnapshot, type LeadGateDto, type LeadTaskDto } from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { ChatMarkdown } from "../ChatMarkdown";
import { ApprovalCard } from "./ApprovalCard";

/// 轮询间隔：任务文件由 agent 随手改写，没有推送通道；读的是本地几个小文件，开销可忽略。
const POLL_MS = 4000;

/// 拉取会话 cwd 所在项目的 Lead 任务；没有任务为 null。cwd 为空（会话还没起来）时不请求。
export function useLeadTask(cwd: string | null | undefined): [LeadTaskDto | null, () => void] {
  const [task, setTask] = useState<LeadTaskDto | null>(null);
  const seq = useRef(0);
  const refresh = useCallback(() => {
    if (!cwd) {
      setTask(null);
      return;
    }
    const mine = ++seq.current;
    leadTaskSnapshot(cwd)
      .then((next) => { if (mine === seq.current) setTask(next); })
      // 读失败（手机远程中继抖一下、后端偶发异常）保留上一次的数据：卡片若因此卸载，leader 在手机上
      // 写了一半的驳回意见会丢（做空审查 M3）。真没有任务时后端返回的是 null，不是报错。
      .catch(() => {});
  }, [cwd]);
  useEffect(() => {
    refresh();
    if (!cwd) return;
    const timer = window.setInterval(refresh, POLL_MS);
    return () => window.clearInterval(timer);
  }, [cwd, refresh]);
  return [task, refresh];
}

/// 关卡是否在等 leader：只有 pending 需要作答；answered 表示已答、等 agent 接手。
export function gateWaiting(task: LeadTaskDto | null): boolean {
  return task?.gate?.status === "pending";
}

export function leadProgress(task: LeadTaskDto): { passed: number; total: number } {
  return { passed: task.checks.filter((c) => c.passes).length, total: task.checks.length };
}

function LeadIcon({ className }: { className: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M4 21V4h11l-1.5 4L15 12H4" />
    </svg>
  );
}

export function LeadPill({ task, active, onClick }: { task: LeadTaskDto; active: boolean; onClick: () => void }) {
  const t = useT();
  const { passed, total } = leadProgress(task);
  return (
    <button type="button" className={"ui-pill" + (active ? " is-active" : "")} onClick={onClick}>
      <LeadIcon className="ui-pill-icon" />
      {t.chat.leadEntry}
      <span className="ui-pill-meta">{total > 0 ? `${passed}/${total}` : t.chat.leadPhase(task.phase)}</span>
      {gateWaiting(task) && <span className="lead-pill-dot" aria-label={t.chat.leadGateWaiting} />}
    </button>
  );
}

export const LeadPanel = memo(function LeadPanel({ task, onClose }: { task: LeadTaskDto; onClose: () => void }) {
  const t = useT();
  const { passed, total } = leadProgress(task);
  const [section, setSection] = useState<"checks" | "spec" | "decisions" | "progress">("checks");
  return (
    <div className="dock-work-panel panel-lead">
      <div className="dock-work-head">
        <span className="dock-work-title">
          <LeadIcon className="dock-work-title-icon" />
          {t.chat.leadEntry}
          <span className="dock-work-meta">{t.chat.leadPhase(task.phase)}</span>
        </span>
        <button type="button" className="dock-work-close" aria-label={t.chat.close} onClick={onClose}>×</button>
      </div>
      <div className="lead-meter" role="progressbar" aria-valuemin={0} aria-valuemax={total} aria-valuenow={passed}
        aria-label={t.chat.leadChecksProgress(passed, total)}>
        <span className="lead-meter-fill" style={{ width: total ? `${(passed / total) * 100}%` : "0%" }} />
      </div>
      <div className="lead-tabs" role="tablist">
        {(["checks", "spec", "decisions", "progress"] as const).map((key) => (
          <button key={key} type="button" role="tab" aria-selected={section === key}
            className={"lead-tab" + (section === key ? " is-active" : "")} onClick={() => setSection(key)}>
            {key === "checks" ? t.chat.leadChecksProgress(passed, total) : t.chat.leadSection[key]}
          </button>
        ))}
      </div>
      <div className="dock-work-body">
        {section === "checks" && (
          task.checks.length === 0
            ? <div className="chat-tasks-empty">{t.chat.leadChecksEmpty}</div>
            : (
              <ul className="lead-checks">
                {task.checks.map((c) => (
                  <li key={c.id} className={"lead-check" + (c.passes ? " is-pass" : "")}>
                    <span className="lead-check-mark" aria-label={c.passes ? t.chat.leadCheckPass : t.chat.leadCheckPending}>{c.passes ? "✓" : "○"}</span>
                    <span className="lead-check-id">#{c.id}</span>
                    <span className="lead-check-text">{c.check}</span>
                  </li>
                ))}
              </ul>
            )
        )}
        {section !== "checks" && (
          <div className="chat-plan-doc chat-md">
            <ChatMarkdown text={(section === "spec" ? task.spec : section === "decisions" ? task.decisions : task.progress) || t.chat.leadSectionEmpty} />
          </div>
        )}
      </div>
    </div>
  );
});

/// A、C 的选项固定；B 用 agent 写进 gate.json 的方案。
function gateChoices(gate: LeadGateDto, t: ReturnType<typeof useT>): { id: string; label: string; recommended?: boolean; detail?: string }[] {
  if (gate.kind === "A") return [{ id: "confirm", label: t.chat.leadGateConfirm }, { id: "revise", label: t.chat.leadGateRevise }];
  if (gate.kind === "C") return [{ id: "accept", label: t.chat.leadGateAccept }, { id: "reject", label: t.chat.leadGateReject }];
  return gate.options.map((o) => ({ id: o.id, label: o.label, recommended: o.recommended, detail: o.detail }));
}

/// 关卡卡片。修改 / 驳回必须写意见（agent 要知道改什么）；B 可选附意见。
export function LeadGateCard({ cwd, gate, onAnswered }: { cwd: string; gate: LeadGateDto; onAnswered: () => void }) {
  const t = useT();
  const [choice, setChoice] = useState<string | null>(null);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const choices = gateChoices(gate, t);
  const needsNote = choice === "revise" || choice === "reject";
  const submit = () => {
    if (!choice || busy || (needsNote && !note.trim())) return;
    setBusy(true);
    setError(null);
    answerLeadGate(cwd, gate.id, choice, note.trim())
      .then(onAnswered)
      .catch((e) => setError(formatBackendError(e, t.locale)))
      .finally(() => setBusy(false));
  };
  return (
    <ApprovalCard
      className="lead-gate-card"
      title={`${t.chat.leadGateKind[gate.kind as "A" | "B" | "C"] ?? gate.kind} · ${gate.title}`}
      badge={t.chat.leadGateWaiting}
      actions={
        <button type="button" className="is-allow" disabled={!choice || busy || (needsNote && !note.trim())} onClick={submit}>
          {busy ? t.chat.leadGateSending : t.chat.leadGateSubmit}
        </button>
      }
    >
      {gate.summary && (
        <div className={"lead-gate-summary chat-md" + (expanded ? " is-expanded" : "")}>
          <ChatMarkdown text={gate.summary} />
        </div>
      )}
      {gate.summary.length > 280 && (
        <button type="button" className="lead-gate-more" onClick={() => setExpanded((v) => !v)}>
          {expanded ? t.chat.leadGateLess : t.chat.leadGateMore}
        </button>
      )}
      <div className="lead-gate-options" role="radiogroup">
        {choices.map((c) => (
          <label key={c.id} className={"lead-gate-option" + (choice === c.id ? " is-picked" : "")}>
            <input type="radio" name={`gate-${gate.id}`} value={c.id} checked={choice === c.id} onChange={() => setChoice(c.id)} />
            <span className="lead-gate-option-label">{c.label}</span>
            {c.recommended && <span className="lead-gate-rec">{t.chat.leadGateRecommended}</span>}
            {c.detail && <span className="lead-gate-option-detail">{c.detail}</span>}
          </label>
        ))}
      </div>
      <textarea
        className="lead-gate-note"
        rows={2}
        placeholder={needsNote ? t.chat.leadGateNoteRequired : t.chat.leadGateNoteOptional}
        value={note}
        onChange={(e) => setNote(e.target.value)}
      />
      {error && <div className="lead-gate-error" role="alert">{error}</div>}
    </ApprovalCard>
  );
}
