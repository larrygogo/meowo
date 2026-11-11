// 名单行内的决定单：轮到你的那一行点开后，批准 / 拒绝 / 拍板就在原位做，不用先进那个会话。
// 卡片本体复用对话窗里的同一套（BrokerApprovalCard、LeadGateCard），行为一处维护；
// 这里只管「不打开会话也拿到那条待处理」和「答完之后怎么收」。
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { pendingInteraction, resolvePendingApproval, type FocusSessionResult, type PendingApproval } from "../../api";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import { useVisibleInterval } from "../../hooks/useVisibleInterval";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { BrokerApprovalCard } from "../chat/BrokerApprovalCard";
import { LeadGateCard, useLeadTask } from "../chat/LeadPanel";
import { RemoteTerminalKeys } from "../chat/RemoteTerminalKeys";
import type { Item } from "../sticker/types";

/** 展开期间多久再问一次后端。事件是主路径（pending-approval 全会话广播），轮询只兜事件错过的情况。 */
const POLL_MS = 1000;

/**
 * 待处理的行展开后的内容。能原位答的给卡片；答不了的（审批已经回到终端、CLI 自己画的提问）
 * 如实说明，由调用方给「进入现场」。返回 null 的 fallback 由 children 承担。
 */
export function RosterDecision({ item, fallback }: { item: Item; fallback: React.ReactNode }) {
  if (item.pending_review === "gate") return <GateDecision item={item} fallback={fallback} />;
  return <ApprovalDecision item={item} fallback={fallback} />;
}

function ApprovalDecision({ item, fallback }: { item: Item; fallback: React.ReactNode }) {
  const t = useT();
  const id = item.session.id;
  const [approval, setApproval] = useState<PendingApproval | null>(null);
  const [asked, setAsked] = useState(false);
  const [resolving, setResolving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const aliveRef = useRef(true);
  useEffect(() => () => { aliveRef.current = false; }, []);

  const refresh = useCallback(() => {
    pendingInteraction(id)
      .then((dto) => {
        if (!aliveRef.current) return;
        setApproval(dto.approval);
        setAsked(true);
      })
      .catch(() => { if (aliveRef.current) setAsked(true); });
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
      .then(() => { if (aliveRef.current) setApproval(null); })
      .catch((raw) => { if (aliveRef.current) setError(formatBackendError(raw, t.locale)); })
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
        <ExternalTerminalJump item={item} />
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

function ExternalTerminalJump({ item }: { item: Item }) {
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
