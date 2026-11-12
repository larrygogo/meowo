// 名单行内的决定单：轮到你的那一行点开后，批准 / 拒绝 / 拍板就在原位做，不用先进那个会话。
// 卡片本体复用对话窗里的同一套（BrokerApprovalCard、LeadGateCard），行为一处维护；
// 这里只管「不打开会话也拿到那条待处理」和「答完之后怎么收」。
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { pendingInteraction, resolvePendingApproval, type FocusSessionResult, type PendingApproval } from "../../api";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import { useVisibleInterval } from "../../hooks/useVisibleInterval";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { ApprovalCard } from "../chat/ApprovalCard";
import { answersRepresentable, composeAnswers, parseAskUserQuestions, type QuestionAnswerDraft } from "../chat/askUserQuestion";
import { BrokerApprovalCard } from "../chat/BrokerApprovalCard";
import { QuestionPanels } from "../chat/QuestionPanels";
import { LeadGateCard, useLeadTask } from "../chat/LeadPanel";
import { RemoteTerminalKeys } from "../chat/RemoteTerminalKeys";
import type { Item } from "../sticker/types";

/** 展开期间多久再问一次后端。事件是主路径（pending-approval 全会话广播），轮询只兜事件错过的情况。 */
const POLL_MS = 1000;

/**
 * 待处理的行展开后的内容。能原位答的给卡片；答不了的（审批已经回到终端、CLI 自己画的提问）
 * 如实说明，由调用方给「进入现场」。返回 null 的 fallback 由 children 承担。
 */
export function RosterDecision({ item, fallback, onEnter }: { item: Item; fallback: React.ReactNode; onEnter?: (id: number, takeover?: boolean) => void }) {
  if (item.pending_review === "gate") return <GateDecision item={item} fallback={fallback} />;
  return <ApprovalDecision item={item} fallback={fallback} onEnter={onEnter} />;
}

function ApprovalDecision({ item, fallback, onEnter }: { item: Item; fallback: React.ReactNode; onEnter?: (id: number, takeover?: boolean) => void }) {
  const t = useT();
  const id = item.session.id;
  const [approval, setApproval] = useState<PendingApproval | null>(null);
  // broker 手里的提问（Claude 的 AskUserQuestion）：终端里此刻没有表单，题面在 Meowo 这边。
  const [question, setQuestion] = useState<PendingApproval | null>(null);
  const [asked, setAsked] = useState(false);
  // 刚在这里答掉了一条：名单要等下一次刷新才知道它不再待处理，这段时间不能把「已回到终端」
  // 和一排终端按键摆出来——agent 已经在跑，那排 Esc 按下去是打断它（做空审查 M3）。
  const [answered, setAnswered] = useState(false);
  const [elsewhere, setElsewhere] = useState(false);
  const [resolving, setResolving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const aliveRef = useRef(true);
  useEffect(() => () => { aliveRef.current = false; }, []);

  const refresh = useCallback(() => {
    pendingInteraction(id)
      .then((dto) => {
        if (!aliveRef.current) return;
        setApproval(dto.approval);
        // 题面内容没变就留着原来那份：换引用会把人正在填的答案清掉。
        setQuestion((prev) => (prev && dto.question && prev.requestId === dto.question.requestId && prev.answerable === dto.question.answerable ? prev : dto.question));
        if (dto.approval || dto.question) { setAnswered(false); setElsewhere(false); }
        setAsked(true);
      })
      // 没问到就不下结论：当成「问过了、表里没有」会把还在 Meowo 手里的审批说成「已回到终端」，
      // 还摆出一排终端按键（做空审查第二轮，问题 4）。下一拍轮询再问。
      .catch(() => {});
  }, [id]);
  useEffect(refresh, [refresh]);
  useVisibleInterval(refresh, POLL_MS);
  useTauriEvent("pending-approval", refresh);
  useTauriEvent("pending-approval-cleared", refresh);

  const decide = useCallback((choice: string) => {
    if (!approval || resolving) return;
    setResolving(true);
    setError(null);
    resolvePendingApproval(id, approval.requestId, choice)
      .then(() => { if (aliveRef.current) { setApproval(null); setAnswered(true); } })
      .catch((raw) => {
        // 没答成。先看这条请求还在不在：不在了就是另一端（手机 / 桌面 / 终端）先答掉了——两端都能领、
        // 先答者生效，后答的这一端如实说「已在别处处理」，不当成出错。还在才是真的失败。
        const requestId = approval.requestId;
        return pendingInteraction(id)
          .then((dto) => {
            if (!aliveRef.current) return;
            if (dto.approval?.requestId === requestId) { setError(formatBackendError(raw, t.locale)); return; }
            setApproval(dto.approval);
            setElsewhere(true);
            setAnswered(true);
          })
          .catch(() => { if (aliveRef.current) setError(formatBackendError(raw, t.locale)); });
      })
      .finally(() => { if (aliveRef.current) setResolving(false); });
  }, [approval, resolving, id, t.locale]);

  const noFocus = useRef<HTMLElement | null>(null);
  if (approval) {
    return (
      <div className="roster-decision is-approval">
        <BrokerApprovalCard
          key={approval.requestId}
          approval={approval}
          countdown={null}
          timedOut={false}
          resolving={resolving}
          error={error}
          // 行内没有「Esc 两下拒绝」：Esc 在名单上只收起这一行，拒绝必须点。
          escArmed={false}
          dirs={item.cwd ? [item.cwd] : []}
          returnFocusTo={noFocus}
          onDecide={decide}
        />
      </div>
    );
  }
  // 问过后端、表里没有这条：审批不在 Meowo 手里（等过了时限回到终端，或这家 CLI 的提问是它自己画的）。
  if (!asked) return <>{fallback}</>;
  if (answered) return <><p className="roster-note" role="status">{elsewhere ? t.roster.answeredElsewhere : t.roster.answered}</p>{fallback}</>;
  // 题面在 Meowo 这边、终端里没有表单：给终端按键是错的（键会落进它的普通输入框，做空审查 M5）。
  // 能在卡里答的（broker 挂着等代答、会话归 Meowo 托管、每题都有不重复的题面）就在行内答；
  // 答不了的如实说进现场作答。
  if (question && question.answerable && item.pty_managed !== false) {
    return (
      <QuestionDecision
        key={question.requestId}
        question={question}
        fallback={<><p className="roster-note">{t.roster.answerInScene}</p>{fallback}</>}
        onAnswered={(elsewhereAnswered) => { setQuestion(null); setElsewhere(elsewhereAnswered); setAnswered(true); }}
      />
    );
  }
  if (question) return <><p className="roster-note">{t.roster.answerInScene}</p>{fallback}</>;
  // Meowo 持有它的终端：把终端此刻的画面和一排按键摆在行内（手机页同一个兜底），不让人自己去找终端。
  if (item.pty_managed) {
    return (
      <div className="roster-decision is-keys">
        <p className="roster-note">{item.pending_review === "approval" ? t.roster.approvalBackWithKeys : t.roster.answerWithKeys}</p>
        <RemoteTerminalKeys sessionId={id} checkScreen />
      </div>
    );
  }
  // 在外部终端里跑、进程还在：Meowo 没有它的终端，看不到画面也按不了键。如实说，给一条直达那个
  // 终端的路；想留在 Meowo 里处理就进现场接管（接管会结束外部那个进程，确认在现场里做）。
  // 这是「不让用户自己去终端」的例外（leader 2026-11-11 关卡 A 取舍 6）。
  if (item.connected && item.pid && !item.background && !item.foreign) {
    return (
      <>
        <p className="roster-note">{t.roster.externalTerminal}</p>
        <ExternalTerminalJump item={item} onTakeover={onEnter} />
        {fallback}
      </>
    );
  }
  return (
    <>
      <p className="roster-note">{item.pending_review === "approval" ? t.roster.approvalInTerminal : t.roster.answerInScene}</p>
      {fallback}
    </>
  );
}

/** Claude 的提问（AskUserQuestion）在行内作答：和对话窗里那张作答卡同一个题面组件、同一条提交路
 *  （答案映射 → broker 回给 hook，agent 直接继续）。这里没有「去终端作答」：要走终端就进现场。 */
function QuestionDecision({ question, fallback, onAnswered }: {
  question: PendingApproval;
  fallback: React.ReactNode;
  onAnswered: (elsewhere: boolean) => void;
}) {
  const t = useT();
  const questions = useMemo(() => parseAskUserQuestions(question.input), [question.input]);
  const [answers, setAnswers] = useState<ReadonlyMap<number, QuestionAnswerDraft>>(new Map());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const aliveRef = useRef(true);
  useEffect(() => () => { aliveRef.current = false; }, []);
  const edit = (index: number, change: (draft: QuestionAnswerDraft) => QuestionAnswerDraft) => {
    setAnswers((current) => new Map(current).set(index, change(current.get(index) ?? { selected: [], custom: "" })));
  };
  const select = (index: number, label: string) => edit(index, (draft) => ({
    ...draft,
    // 单选点即换（再点取消），多选勾选切换：同对话窗的作答卡。
    selected: draft.selected.includes(label) ? draft.selected.filter((entry) => entry !== label) : questions[index]?.multiSelect ? [...draft.selected, label] : [label],
  }));
  // 题面表达不成「问题原文 → 答案」的（空题面、同文重题）不给提交：送出去会错位。
  if (!answersRepresentable(questions)) return <>{fallback}</>;
  const answerMap = composeAnswers(questions, answers);
  const unanswered = questions.filter((_, index) => { const draft = answers.get(index); return !(draft && (draft.selected.length > 0 || draft.custom.trim())); }).length;
  const submit = () => {
    if (busy || !answerMap) return;
    setBusy(true);
    setError(null);
    resolvePendingApproval(question.sessionId, question.requestId, `answers:${JSON.stringify(answerMap)}`)
      .then(() => { if (aliveRef.current) onAnswered(false); })
      .catch((raw) => pendingInteraction(question.sessionId)
        .then((dto) => {
          if (!aliveRef.current) return;
          // 这道题还挂着才是真的没答成；不在了就是别处（另一端、终端、超时）先了结了。
          if (dto.question?.requestId === question.requestId) setError(formatBackendError(raw, t.locale));
          else onAnswered(true);
        })
        .catch(() => { if (aliveRef.current) setError(formatBackendError(raw, t.locale)); }))
      .finally(() => { if (aliveRef.current) setBusy(false); });
  };
  return (
    <div className="roster-decision is-question">
      <ApprovalCard
        title={t.chat.questionTitle}
        badge={t.chat.questionPending}
        actions={<button type="button" className="btn-primary" disabled={busy || !answerMap} onClick={submit}>{t.chat.submitAnswer}</button>}
      >
        <QuestionPanels
          mode="answer"
          items={questions}
          answers={answers}
          onSelect={select}
          onCustom={(index, text) => edit(index, (draft) => ({ ...draft, custom: text }))}
          onSubmit={submit}
        />
        <span>{answerMap ? t.chat.questionAnswerReady : t.chat.questionAnswerIncomplete(unanswered)}</span>
        {error && <span role="alert">{error}</span>}
      </ApprovalCard>
    </div>
  );
}

function ExternalTerminalJump({ item, onTakeover }: { item: Item; onTakeover?: (id: number, takeover?: boolean) => void }) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [missed, setMissed] = useState(false);
  const jump = () => {
    if (busy) return;
    setBusy(true);
    setMissed(false);
    invoke<FocusSessionResult | undefined>("focus_session", {
      pid: item.pid,
      title: item.task_title,
      cwd: item.cwd,
      sessionId: item.session.cc_session_id,
      provider: item.provider,
    })
      // 只有真切到了那个终端（或至少把它的宿主窗口提到前面）才算数；其余都说没找到。
      .then((result) => { if (result && result !== "focused" && result !== "host_focused") setMissed(true); })
      .catch(() => setMissed(true))
      .finally(() => setBusy(false));
  };
  return (
    <div className="roster-jump">
      <button type="button" className="roster-enter is-quiet" disabled={busy} onClick={jump}>{t.roster.jumpToTerminal}</button>
      {/* 接管会结束外部那个进程：这里把人带到现场并把接管确认摆出来，确认在那里点，不在名单上一键杀进程。 */}
      {onTakeover && <button type="button" className="roster-enter is-quiet" onClick={() => onTakeover(item.session.id, true)}>{t.roster.takeover}</button>}
      {missed && <span className="roster-note" role="status">{t.roster.jumpMissed}</span>}
    </div>
  );
}

function GateDecision({ item, fallback }: { item: Item; fallback: React.ReactNode }) {
  const [task, refresh] = useLeadTask(item.cwd, item.session.cc_session_id);
  const gate = task?.gate;
  if (item.cwd && gate?.status === "pending") {
    return (
      <div className="roster-decision">
        <LeadGateCard key={gate.id} cwd={item.cwd} gate={gate} onAnswered={refresh} />
      </div>
    );
  }
  return <>{fallback}</>;
}
