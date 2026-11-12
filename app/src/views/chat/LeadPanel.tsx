/// Lead 工作流（停靠栏的 Lead 入口 + 关卡卡片）。
///
/// 数据只来自 `lead_task_snapshot`：后端按约定读 `.lead/active` 指向的任务目录（手机端也走
/// 同一命令，远程桥放行理由见 remote.rs「例外（2026-11-09）」）。前端不直接读文件。
///
/// 关卡卡片的作答只写 gate.json（answer_lead_gate），答复由 agent 侧的 asyncRewake 后台
/// hook 看到后唤醒模型——卡片不往终端写字，所以 Meowo 重启、手机断线都不会丢答复。

import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { answerLeadGate, leadTaskSnapshot, type LeadGateDto, type LeadTaskDto } from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { useVisibleInterval } from "../../hooks/useVisibleInterval";
import { ChatMarkdown } from "../ChatMarkdown";
import { ApprovalCard } from "./ApprovalCard";
import { useTablist } from "../useTablist";
import { Icon } from "../../icons/Icon";
import { icCircle, icClock, icFlag, icMinus } from "../../icons/glyphs.chat";
import { icCheck } from "../../icons/glyphs";

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

/// 排序权重：还要 agent 做的 → 只差实测、等你授权的 → 等 leader 验收的 → 已通过 → 已豁免。同组保持原顺序。
function checkRank(c: LeadTaskDto["checks"][number]): number {
  if (c.passes) return 3;
  if (c.waived) return 4;
  if (c.leader) return 2;
  return c.blocked ? 1 : 0;
}

/// 只差环境实测或授权的条目数（已通过、已豁免的不算）。
export function leadBlockedCount(task: LeadTaskDto): number {
  // 要 leader 亲自验收的以 leader 为准：hook 不为它催关卡 D，面板也只标「待你验收」（做空审查：两边都不管）。
  return task.checks.filter((c) => !c.passes && !c.waived && !c.leader && c.blocked).length;
}

function LeadIcon({ className }: { className: string }) {
  return (
    <Icon glyph={icFlag} size={16} className={className} />
  );
}

export function LeadPill({ task, active, onClick }: { task: LeadTaskDto; active: boolean; onClick: () => void }) {
  const t = useT();
  const { passed, total } = leadProgress(task);
  // 面板收起时 leader 先扫到的是这颗胶囊：只写「5/18」会被读成只做了三成，待实测的也报出来（做空审查）。
  const blockedCount = leadBlockedCount(task);
  return (
    <button type="button" className={"ui-pill" + (active ? " is-active" : "")} onClick={onClick}>
      <LeadIcon className="ui-pill-icon" />
      {t.chat.leadEntry}
      <span className="ui-pill-meta">{total > 0 ? `${passed}/${total}` : t.chat.leadPhase(task.phase)}</span>
      {blockedCount > 0 && <span className="ui-pill-meta">{t.chat.leadPillBlocked(blockedCount)}</span>}
      {gateWaiting(task) && <span className="lead-pill-dot" role="img" aria-label={t.chat.leadGateWaiting} />}
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
  pass: <Icon glyph={icCheck} size={14} />,
  pending: <Icon glyph={icCircle} size={14} />,
  waived: <Icon glyph={icMinus} size={14} />,
  blocked: <Icon glyph={icClock} size={14} />,
};

const LEAD_SECTIONS = ["checks", "spec", "decisions", "progress"] as const;

/// 流程阶段，顺序即 skill 的 1→6。blocked / unknown 不在条上，单独标。
export const LEAD_STAGES = ["interview", "spec", "design", "implement", "verify", "done"] as const;

/// 任务名：SPEC 第一个 `#` 标题，去掉末尾括号里的目录名（skill 惯例写成「标题（slug）」）；取不到用目录名。
/// 面板原先只写「Lead · 出方案」，同一项目几个任务轮着做时看不出是哪个（2026-11-12 优化）。
export function leadTaskTitle(task: Pick<LeadTaskDto, "spec" | "slug">): string {
  const m = /^#\s+(.+?)\s*$/m.exec(task.spec);
  if (!m) return task.slug;
  const title = m[1].replace(/\s*[（(]\s*[\w.-]+\s*[）)]\s*$/, "").trim();
  return title || task.slug;
}

/// agent 最近在做什么：progress.md 最后一条**顶层**的行（去掉列表符号和勾选框）。缩进的子条目是上一条的细节，
/// 不是「最近在做什么」；`phase:`、标题、代码块围栏、表格行也跳过（做空审查：曾显示成子条目、``` 和 |---|）。
export function leadLatest(progress: string): string | null {
  let fenced = false;
  const lines = progress.split(/\r?\n/).filter((l) => {
    const t = l.trim();
    if (t.startsWith("```")) {
      fenced = !fenced;
      return false;
    }
    return !fenced && t && !/^\s/.test(l) && !/^phase\s*:/i.test(t) && !t.startsWith("#") && !t.startsWith("|");
  });
  const last = lines.at(-1)?.trim();
  if (!last) return null;
  const text = last.replace(/^[-*+]\s+/, "").replace(/^\d+\.\s+/, "").replace(/^\[[ xX]\]\s+/, "").trim();
  return text || null;
}

function LeadStages({ phase }: { phase: string }) {
  const t = useT();
  const at = (LEAD_STAGES as readonly string[]).indexOf(phase);
  return (
    <div className="lead-stages-row">
      <ol className="lead-stages" aria-label={t.chat.leadStagesLabel}>
        {LEAD_STAGES.map((stage, i) => {
          // done 是终点：走到这里整条都算完成，没有「当前」。
          const state = phase === "done" || (at >= 0 && i < at) ? "is-done" : i === at ? "is-current" : "";
          return (
            <li key={stage} className={("lead-stage " + state).trim()} aria-current={i === at && phase !== "done" ? "step" : undefined}>
              {t.chat.leadStage(stage)}
            </li>
          );
        })}
      </ol>
      {phase === "blocked" && <span className="lead-stage-flag is-blocked">{t.chat.leadPhase("blocked")}</span>}
      {/* 解析不到、或 agent 写了约定外的阶段名：六段全灰会被看成「还没开始」，标出来（做空审查）。 */}
      {phase !== "blocked" && at < 0 && <span className="lead-stage-flag">{t.chat.leadPhaseOther(phase)}</span>}
    </div>
  );
}

export const LeadPanel = memo(function LeadPanel({ task, onClose }: { task: LeadTaskDto; onClose: () => void }) {
  const t = useT();
  const { passed, total } = leadProgress(task);
  const [section, setSection] = useState<(typeof LEAD_SECTIONS)[number]>("checks");
  const tabs = useTablist({ keys: LEAD_SECTIONS, active: section, onSelect: setSection });
  // 展开看全文的验收项（默认两行截断）。
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  // 还要做的排前面，豁免的沉底：一眼看到剩下的事。
  const checks = [...task.checks].sort((a, b) => checkRank(a) - checkRank(b));
  const title = leadTaskTitle(task);
  const latest = leadLatest(task.progress);
  // 「5/18」只数实测过的。代码写完、只差实测的条目单独报个数，免得 leader 以为只做了三成
  //（2026-11-12 leader 看云端 Xbot 第三期：「为什么都已经到了验证阶段进度却才 5/18」）。
  const blockedCount = leadBlockedCount(task);
  return (
    <div className="dock-work-panel panel-lead">
      <div className="dock-work-head">
        <span className="dock-work-title lead-title" title={title}>
          <LeadIcon className="dock-work-title-icon" />
          <span className="lead-title-text">{title}</span>
        </span>
        <span className="lead-head-end">
          {blockedCount > 0 && <span className="lead-blocked-count">{t.chat.leadBlockedCount(blockedCount)}</span>}
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
      <div className="lead-overview">
        <LeadStages phase={task.phase} />
        {latest && <p className="lead-latest" title={latest}><span className="lead-latest-label">{t.chat.leadLatest}</span>{latest}</p>}
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
                  const state = c.passes ? "pass" : c.waived ? "waived" : c.blocked && !c.leader ? "blocked" : "pending";
                  const label = state === "pass" ? t.chat.leadCheckPass : state === "waived" ? t.chat.leadCheckWaived
                    : state === "blocked" ? t.chat.leadCheckBlocked : t.chat.leadCheckPending;
                  return (
                    // 展开靠 <button aria-expanded>：原先是 <li onClick>，键盘到不了、读屏不知道能展开，
                    // 截断的全文键盘用户看不到（调研 i11）。图标 role="img" 才让 aria-label 生效。
                    <li key={c.id}>
                      <button type="button" className={"lead-check is-" + state + (open.has(c.id) ? " is-open" : "")}
                        aria-expanded={open.has(c.id)}
                        title={state === "waived" ? `${c.check}
${t.chat.leadCheckWaivedReason(c.waived ?? "")}` : c.check}
                        onClick={() => setOpen((prev) => { const next = new Set(prev); if (next.has(c.id)) next.delete(c.id); else next.add(c.id); return next; })}>
                        <span className="lead-check-mark" role="img" aria-label={label}>{CHECK_ICONS[state]}</span>
                        <span className="lead-check-body">
                          <span className="lead-check-text">{c.check}</span>
                          {state === "waived" && <span className="lead-check-note">{t.chat.leadCheckWaivedReason(c.waived ?? "")}</span>}
                          {state === "blocked" && <span className="lead-check-note is-blocked">{t.chat.leadCheckBlockedReason(c.blocked ?? "")}</span>}
                          {!c.waived && !c.passes && c.leader && <span className="lead-check-note is-leader">{t.chat.leadCheckLeader}</span>}
                        </span>
                      </button>
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
/// negative：否定类（要改 / 驳回 / 不允许）。点了不提交，先展开意见框：要改什么得说清楚，而且本仓有「回车即误拒」
/// 的前科，否定动作不能一步到位。
type GateChoice = { id: string; label: string; recommended?: boolean; detail?: string; requiresNote?: boolean; negative?: boolean; primary?: boolean };

/// 固定选项的关卡直接给动作按钮（对标 GitHub 的 Approve and Submit：决定和提交是一步）。原先要先点单选再点提交，
/// 确认、验收这类最常见的作答也得两下（2026-11-12 优化）。
const DIRECT_KINDS = new Set(["A", "C", "D"]);

function gateChoices(gate: LeadGateDto, t: ReturnType<typeof useT>): GateChoice[] {
  if (gate.kind === "A") return [{ id: "revise", label: t.chat.leadGateRevise, requiresNote: true, negative: true }, { id: "confirm", label: t.chat.leadGateConfirm, primary: true }];
  if (gate.kind === "C") return [{ id: "reject", label: t.chat.leadGateReject, requiresNote: true, negative: true }, { id: "accept", label: t.chat.leadGateAccept, primary: true }];
  if (gate.kind === "D") return [{ id: "deny", label: t.chat.leadGateDeny, negative: true }, { id: "done", label: t.chat.leadGateDone }, { id: "approve", label: t.chat.leadGateApprove, primary: true }];
  const own = gate.options.map((o) => ({ id: o.id, label: o.label, recommended: o.recommended, detail: o.detail }));
  // Q 是访谈：开放式问题常常没有合适的选项，留一个「其他」只写意见作答（做空审查第 1 轮：选项为空时卡片提交不了）。
  // agent 自己写了 id 为 other 的选项就不再追加，免得 key 重复、语义打架（做空审查第 2 轮）。
  const addOther = gate.kind === "Q" && !own.some((o) => o.id === "other");
  return addOther ? [...own, { id: "other", label: t.chat.leadGateOther, requiresNote: true }] : own;
}

/// 关卡卡片。固定选项（A、C、D）是一排动作按钮：肯定类一步提交，否定类先写意见；B、Q 是选项列表加「提交」。
/// 意见框默认收起：选了必须写意见的项、或者点了「附加意见」才出现（原先常驻两行，是卡片里最占地方的一块）。
export function LeadGateCard({ cwd, gate, onAnswered }: { cwd: string; gate: LeadGateDto; onAnswered: () => void }) {
  const t = useT();
  const direct = DIRECT_KINDS.has(gate.kind);
  // B、Q：选中的选项；A、C、D：点了否定按钮、正在写意见的那一项。不预选推荐项：复盘里关卡多是一路点过去的。
  const [choice, setChoice] = useState<string | null>(null);
  const [note, setNote] = useState("");
  const [noteOpen, setNoteOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const noteRef = useRef<HTMLTextAreaElement>(null);
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
  const writing = direct && picked?.negative === true;
  const showNote = noteOpen || needsNote || writing;
  // 意见框总是用户点出来的（否定按钮、附加意见、选了「其他」），出现就把焦点给它，接着打字。
  useEffect(() => {
    if (showNote) noteRef.current?.focus({ preventScroll: true });
  }, [showNote]);
  // 正在提交的那一项：一步提交的按钮也要显示「提交中」，手机中继慢时不然像没点上（做空审查）。
  const [sending, setSending] = useState<string | null>(null);
  const send = (id: string) => {
    const want = choices.find((c) => c.id === id);
    if (busy || (want?.requiresNote && !note.trim())) return;
    setBusy(true);
    setSending(id);
    setError(null);
    // 只发看得见的意见。意见框藏起来时（取消驳回后点通过、改选了不需要意见的项）文字还留在状态里，
    // 原先照发：「通过 + 别合」「允许 + 别花钱」这种自相矛盾的答复会送到 agent（做空审查复现）。
    answerLeadGate(cwd, gate.id, id, showNote ? note.trim() : "")
      .then(onAnswered)
      .catch((e) => setError(formatBackendError(e, t.locale)))
      .finally(() => { setBusy(false); setSending(null); });
  };
  const placeholder = gate.kind === "Q" && needsNote ? t.chat.leadGateAnswerRequired : needsNote ? t.chat.leadGateNoteRequired : t.chat.leadGateNoteOptional;
  const kindLabel = t.chat.leadGateKind[gate.kind as keyof typeof t.chat.leadGateKind] ?? gate.kind;
  let actions: ReactNode;
  if (!direct) {
    actions = (
      <button type="button" className="btn-primary" disabled={!choice || busy || (needsNote && !note.trim())} onClick={() => choice && send(choice)}>
        {busy ? t.chat.leadGateSending : t.chat.leadGateSubmit}
      </button>
    );
  } else if (writing && picked) {
    actions = (
      <>
        <button type="button" disabled={busy} onClick={() => { setChoice(null); setNote(""); setError(null); }}>{t.chat.leadGateCancel}</button>
        <button type="button" className="btn-primary" disabled={busy || (needsNote && !note.trim())} onClick={() => send(picked.id)}>
          {busy ? t.chat.leadGateSending : t.chat.leadGateSend}
        </button>
      </>
    );
  } else {
    actions = choices.map((c) => (
      // 否定类挂 is-deny：键盘 Tab 进卡的首个落点是它（见 ApprovalCard），它只展开意见框，空格、回车都不会误提交。
      <button key={c.id} type="button" className={c.negative ? "is-deny" : c.primary ? "btn-primary" : undefined} disabled={busy}
        onClick={() => (c.negative ? setChoice(c.id) : send(c.id))}>
        {sending === c.id ? t.chat.leadGateSending : c.label}
      </button>
    ));
  }
  return (
    <ApprovalCard className={"lead-gate-card" + (direct ? " is-direct" : "")} title={`${kindLabel} · ${gate.title}`} actions={actions}>
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
      {!direct && (
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
      )}
      {showNote ? (
        <>
          <textarea ref={noteRef} className="lead-gate-note" rows={2} placeholder={placeholder} aria-label={placeholder}
            value={note} onChange={(e) => setNote(e.target.value)} />
          {/* 自己点开的可选意见可以收回（收回即清空，免得藏着的字被发出去）。 */}
          {noteOpen && !needsNote && !writing && (
            <button type="button" className="lead-gate-more lead-gate-add-note" onClick={() => { setNoteOpen(false); setNote(""); }}>{t.chat.leadGateRemoveNote}</button>
          )}
        </>
      ) : !direct && (
        <button type="button" className="lead-gate-more lead-gate-add-note" onClick={() => setNoteOpen(true)}>{t.chat.leadGateAddNote}</button>
      )}
      {error && <div className="lead-gate-error" role="alert">{error}</div>}
    </ApprovalCard>
  );
}
