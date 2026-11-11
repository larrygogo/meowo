/// Lead 工作流（停靠栏的 Lead 入口 + 关卡卡片）。
///
/// 数据只来自 `lead_task_snapshot`：后端按约定读 `.lead/active` 指向的任务目录（手机端也走
/// 同一命令，远程桥放行理由见 remote.rs「例外（2026-11-09）」）。前端不直接读文件。
///
/// 关卡卡片的作答只写 gate.json（answer_lead_gate），答复由 agent 侧的 asyncRewake 后台
/// hook 看到后唤醒模型——卡片不往终端写字，所以 Meowo 重启、手机断线都不会丢答复。

import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { answerLeadGate, leadTaskSnapshot, type LeadGateDto, type LeadTaskDto } from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { useVisibleInterval } from "../../hooks/useVisibleInterval";
import { ChatMarkdown } from "../ChatMarkdown";
import { ApprovalCard } from "./ApprovalCard";
import { useTablist } from "../useTablist";

/// 轮询间隔：任务文件由 agent 随手改写，没有推送通道；读的是本地几个小文件，开销可忽略。
const POLL_MS = 4000;

/// 拉取会话 cwd 所在项目的 Lead 任务；没有任务为 null。cwd 为空（会话还没起来）时不请求。
/// session：当前会话的 cc_session_id。还不知道（会话历史没到）时先不查，免得别的会话的任务闪一下。
export function useLeadTask(cwd: string | null | undefined, session?: string | null): [LeadTaskDto | null, () => void] {
  const [task, setTask] = useState<LeadTaskDto | null>(null);
  const seq = useRef(0);
  const refresh = useCallback(() => {
    if (!cwd || !session) {
      setTask(null);
      return;
    }
    const mine = ++seq.current;
    leadTaskSnapshot(cwd, session)
      .then((next) => { if (mine === seq.current) setTask(next); })
      // 读失败（手机远程中继抖一下、后端偶发异常）保留上一次的数据：卡片若因此卸载，leader 在手机上
      // 写了一半的驳回意见会丢（做空审查 M3）。真没有任务时后端返回的是 null，不是报错。
      .catch(() => {});
  }, [cwd, session]);
  useEffect(() => {
    refresh();
  }, [cwd, refresh]);
  // 窗口隐藏时不拉：关卡等的是 leader，看不见的时候拉了也没人看；重新可见那刻立即补一次。
  useVisibleInterval(() => {
    if (cwd && session) refresh();
  }, POLL_MS);
  return [task, refresh];
}

/// 关卡是否在等 leader：只有 pending 需要作答；answered 表示已答、等 agent 接手。
export function gateWaiting(task: LeadTaskDto | null): boolean {
  return task?.gate?.status === "pending";
}

/// 进度不算豁免项：leader 已经决定不做的条目留在分母里，任务就永远差几条（实测三个任务带红收工）。
/// 已通过的条目即使还挂着豁免原因（先豁免、后来又做完了），也按通过算，与后端 failing() 一致（PR #156 审查）。
export function leadProgress(task: LeadTaskDto): { passed: number; total: number } {
  const live = task.checks.filter((c) => c.passes || !c.waived);
  return { passed: live.filter((c) => c.passes).length, total: live.length };
}

/// 排序权重：还要 agent 做的 → 等 leader 验收的 → 已通过 → 已豁免。同组保持原顺序。
function checkRank(c: LeadTaskDto["checks"][number]): number {
  if (c.passes) return 2;
  if (c.waived) return 3;
  return c.leader ? 1 : 0;
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
      {/* 已答复、agent 还没接手：答复已落盘，agent 醒来或会话被恢复后会接着做。会话没能自动恢复时
          （安装器杀了等待进程、外部终端会话），这就是界面上的降级提示（做空审查第二轮 M2）。 */}
      {task.gate?.status === "answered" && <span className="ui-pill-meta lead-pill-answered">{t.chat.leadGateAnswered}</span>}
    </button>
  );
}

/// 标题行右侧的小进度环（用户选定 2026-11-09：进度并进标题行，去掉单独的细长进度条）。
function LeadRing({ passed, total }: { passed: number; total: number }) {
  const r = 6;
  const c = 2 * Math.PI * r;
  const done = total > 0 ? passed / total : 0;
  return (
    <svg className="lead-ring" width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
      <circle cx="8" cy="8" r={r} fill="none" stroke="var(--fill-selected)" strokeWidth="2" />
      <circle cx="8" cy="8" r={r} fill="none" stroke="var(--cc-ok-vivid)" strokeWidth="2" strokeLinecap="round"
        strokeDasharray={`${c * done} ${c}`} transform="rotate(-90 8 8)" />
    </svg>
  );
}

const CHECK_ICONS = {
  pass: <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg>,
  pending: <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true"><circle cx="12" cy="12" r="7.5" /></svg>,
  waived: <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true"><path d="M7 12h10" /></svg>,
};

const LEAD_SECTIONS = ["checks", "spec", "decisions", "progress"] as const;

export const LeadPanel = memo(function LeadPanel({ task, onClose }: { task: LeadTaskDto; onClose: () => void }) {
  const t = useT();
  const { passed, total } = leadProgress(task);
  const [section, setSection] = useState<(typeof LEAD_SECTIONS)[number]>("checks");
  const tabs = useTablist({ keys: LEAD_SECTIONS, active: section, onSelect: setSection });
  // 展开看全文的验收项（默认两行截断）。
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  // 还要做的排前面，豁免的沉底：一眼看到剩下的事。
  const checks = [...task.checks].sort((a, b) => checkRank(a) - checkRank(b));
  return (
    <div className="dock-work-panel panel-lead">
      <div className="dock-work-head">
        <span className="dock-work-title">
          <LeadIcon className="dock-work-title-icon" />
          {t.chat.leadEntry}
          <span className="dock-work-meta lead-phase">{t.chat.leadPhase(task.phase)}</span>
        </span>
        <span className="lead-head-end">
          {total > 0 && (
            <span className="lead-progress" role="progressbar" aria-valuemin={0} aria-valuemax={total} aria-valuenow={passed}
              aria-label={t.chat.leadChecksProgress(passed, total)}>
              <LeadRing passed={passed} total={total} />
              {passed}/{total}
            </span>
          )}
          <button type="button" className="dock-work-close" aria-label={t.chat.close} onClick={onClose}>×</button>
        </span>
      </div>
      <div className="lead-tabs" {...tabs.listProps}>
        {LEAD_SECTIONS.map((key) => (
          <button key={key} type="button" {...tabs.tabProps(key)}
            className={"lead-tab" + (section === key ? " is-active" : "")} onClick={() => setSection(key)}>
            {key === "checks" ? t.chat.leadSection.checks : t.chat.leadSection[key]}
          </button>
        ))}
      </div>
      <div className="dock-work-body" {...tabs.panelProps}>
        {section === "checks" && (
          checks.length === 0
            ? <div className="chat-tasks-empty">{t.chat.leadChecksEmpty}</div>
            : (
              <ul className="lead-checks">
                {checks.map((c) => {
                  const state = c.passes ? "pass" : c.waived ? "waived" : "pending";
                  const label = state === "pass" ? t.chat.leadCheckPass : state === "waived" ? t.chat.leadCheckWaived : t.chat.leadCheckPending;
                  return (
                    <li key={c.id} className={"lead-check is-" + state + (open.has(c.id) ? " is-open" : "")}
                      title={state === "waived" ? `${c.check}
${t.chat.leadCheckWaivedReason(c.waived ?? "")}` : c.check}
                      onClick={() => setOpen((prev) => { const next = new Set(prev); if (next.has(c.id)) next.delete(c.id); else next.add(c.id); return next; })}>
                      <span className="lead-check-mark" aria-label={label}>{CHECK_ICONS[state]}</span>
                      <span className="lead-check-body">
                        <span className="lead-check-text">{c.check}</span>
                        {state === "waived" && <span className="lead-check-note">{t.chat.leadCheckWaivedReason(c.waived ?? "")}</span>}
                        {!c.waived && !c.passes && c.leader && <span className="lead-check-note is-leader">{t.chat.leadCheckLeader}</span>}
                      </span>
                    </li>
                  );
                })}
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

/// A、C、D 的选项固定（与 reporter 的 fixed_choices 一致，后端会再校验一次）；B、Q 用 agent 写进 gate.json 的选项。
type GateChoice = { id: string; label: string; recommended?: boolean; detail?: string; requiresNote?: boolean };

function gateChoices(gate: LeadGateDto, t: ReturnType<typeof useT>): GateChoice[] {
  if (gate.kind === "A") return [{ id: "confirm", label: t.chat.leadGateConfirm }, { id: "revise", label: t.chat.leadGateRevise, requiresNote: true }];
  if (gate.kind === "C") return [{ id: "accept", label: t.chat.leadGateAccept }, { id: "reject", label: t.chat.leadGateReject, requiresNote: true }];
  if (gate.kind === "D") return [{ id: "approve", label: t.chat.leadGateApprove }, { id: "done", label: t.chat.leadGateDone }, { id: "deny", label: t.chat.leadGateDeny }];
  const own = gate.options.map((o) => ({ id: o.id, label: o.label, recommended: o.recommended, detail: o.detail }));
  // Q 是访谈：开放式问题常常没有合适的选项，留一个「其他」只写意见作答（做空审查第 1 轮：选项为空时卡片提交不了）。
  // agent 自己写了 id 为 other 的选项就不再追加，免得 key 重复、语义打架（做空审查第 2 轮）。
  const addOther = gate.kind === "Q" && !own.some((o) => o.id === "other");
  return addOther ? [...own, { id: "other", label: t.chat.leadGateOther, requiresNote: true }] : own;
}

/// 关卡卡片。修改 / 驳回必须写意见（agent 要知道改什么）；B 可选附意见。
export function LeadGateCard({ cwd, gate, onAnswered }: { cwd: string; gate: LeadGateDto; onAnswered: () => void }) {
  const t = useT();
  const [choice, setChoice] = useState<string | null>(null);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  // 「展开全部」按实际是否溢出决定，不按字数：按字数判断时短段落多的摘要被截断却没有展开钮（实拍）。
  const summaryRef = useRef<HTMLDivElement>(null);
  const [overflows, setOverflows] = useState(false);
  // 卡片宽度会变（面板拉宽拉窄、窗口缩放、手机横竖屏），只在摘要变化时量一次会漏判，
  // 所以挂 ResizeObserver 跟着重量。
  useLayoutEffect(() => {
    const el = summaryRef.current;
    if (!el || expanded) return;
    const measure = () => setOverflows(el.scrollHeight > el.clientHeight + 1);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [gate.summary, expanded]);
  const choices = gateChoices(gate, t);
  // 哪个选项要求写意见由 gateChoices 一处决定，这里和提示语都只读这个标记（PR #156 审查：条件写了三遍）。
  const picked = choices.find((c) => c.id === choice);
  const needsNote = picked?.requiresNote === true;
  const answering = gate.kind === "Q" && needsNote;
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
      title={`${t.chat.leadGateKind[gate.kind as keyof typeof t.chat.leadGateKind] ?? gate.kind} · ${gate.title}`}
      badge={t.chat.leadGateWaiting}
      actions={
        <button type="button" className="is-allow" disabled={!choice || busy || (needsNote && !note.trim())} onClick={submit}>
          {busy ? t.chat.leadGateSending : t.chat.leadGateSubmit}
        </button>
      }
    >
      {gate.summary && (
        <div ref={summaryRef} className={"lead-gate-summary chat-md" + (expanded ? " is-expanded" : overflows ? " is-clipped" : "")}>
          <ChatMarkdown text={gate.summary} />
        </div>
      )}
      {(overflows || expanded) && (
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
        placeholder={answering ? t.chat.leadGateAnswerRequired : needsNote ? t.chat.leadGateNoteRequired : t.chat.leadGateNoteOptional}
        value={note}
        onChange={(e) => setNote(e.target.value)}
      />
      {error && <div className="lead-gate-error" role="alert">{error}</div>}
    </ApprovalCard>
  );
}
