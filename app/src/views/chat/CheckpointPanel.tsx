import { useEffect, useState, type MutableRefObject } from "react";
import { checkpointFileDiff, checkpointRestore, checkpointTurnDetail, checkpointTurns, checkpointUndo, type CheckpointRestoreDto, type CheckpointTurnDto, type GitDiffSummaryDto } from "../../api";
import { formatBackendError } from "../../i18n/errors";
import { busyProps } from "../../busyButton";
import { useT } from "../../i18n";
import { AppModal } from "../AppModal";
import { LazyChangesView as ChangesView } from "./lazyDiff";
import type { ReviewComment } from "./diffComments";

export function CheckpointPanel({ sessionId, cwd, summary, comments, onAddComment, onRemoveComment, escOverrideRef, onChanged }: {
  sessionId: number;
  cwd: string;
  summary: GitDiffSummaryDto;
  comments: readonly ReviewComment[];
  onAddComment: (comment: Omit<ReviewComment, "id" | "dir">) => void;
  onRemoveComment: (id: string) => void;
  escOverrideRef: MutableRefObject<(() => void) | null>;
  onChanged: () => void;
}) {
  const t = useT();
  const [turns, setTurns] = useState<CheckpointTurnDto[]>([]);
  const [turn, setTurn] = useState<number | null>(null);
  const [detail, setDetail] = useState<CheckpointTurnDto | null>(null);
  const [repoRoot, setRepoRoot] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [confirm, setConfirm] = useState<"start" | "end" | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<CheckpointRestoreDto | null>(null);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    checkpointTurns(sessionId).then((items) => {
      if (!alive) return;
      setTurns(items);
      setTurn((current) => current && items.some((item) => item.turn === current) ? current : items[0]?.turn ?? null);
      setLoading(false);
    }).catch((cause) => {
      if (!alive) return;
      setError(formatBackendError(cause, t.locale));
      setLoading(false);
    });
    return () => { alive = false; };
  }, [sessionId, t.locale]);

  useEffect(() => {
    if (turn == null) {
      setDetail(null);
      return;
    }
    let alive = true;
    setDetail(null);
    setRepoRoot(null);
    checkpointTurnDetail(sessionId, turn).then((value) => {
      if (alive) setDetail(value);
    }).catch((cause) => {
      if (alive) setError(formatBackendError(cause, t.locale));
    });
    return () => { alive = false; };
  }, [sessionId, turn, t.locale]);

  const index = turns.findIndex((item) => item.turn === turn);
  const repo = detail?.repos.find((item) => item.root === repoRoot) ?? detail?.repos[0] ?? null;
  const ready = !!repo?.startRef && !!repo.endRef && !repo.error;
  const apply = async (phase: "start" | "end") => {
    if (turn == null) return;
    setBusy(true);
    setError("");
    try {
      const value = await checkpointRestore(sessionId, turn, phase);
      setResult(value);
      setConfirm(null);
      onChanged();
    } catch (cause) {
      setError(formatBackendError(cause, t.locale));
    } finally {
      setBusy(false);
    }
  };
  const undo = async (id: number) => {
    setBusy(true);
    try {
      setResult(await checkpointUndo(sessionId, id));
      onChanged();
    } catch (cause) {
      setError(formatBackendError(cause, t.locale));
    } finally {
      setBusy(false);
    }
  };

  return <section className="checkpoint-panel" aria-label={t.chat.checkpointScope}>
    <div className="checkpoint-toolbar">
      <button type="button" className="scm-icon-btn" aria-label={t.chat.checkpointPrevious} disabled={index < 0 || index === 0} onClick={() => setTurn(turns[index - 1]?.turn ?? turn)}>‹</button>
      <strong>{turn == null ? t.chat.checkpointScope : t.chat.checkpointTurn(turn)}</strong>
      <button type="button" className="scm-icon-btn" aria-label={t.chat.checkpointNext} disabled={index < 0 || index >= turns.length - 1} onClick={() => setTurn(turns[index + 1]?.turn ?? turn)}>›</button>
      {ready && <span className="checkpoint-actions">
        <button type="button" className="chat-user-action" {...busyProps(busy, () => setConfirm("start"))}>{t.chat.checkpointRestoreStart}</button>
        <button type="button" className="chat-user-action" {...busyProps(busy, () => setConfirm("end"))}>{t.chat.checkpointRestoreEnd}</button>
      </span>}
    </div>
    {/* 只读预览要说出来：下面的文件列表和「当前改动」长得一样，不说就像代码已经被切过去了（2026-11-16 用户反馈）。 */}
    {turn != null && <div className="checkpoint-preview-note">{t.chat.checkpointPreviewNote}</div>}
    {detail && detail.repos.length > 1 && <div className="checkpoint-repos">{detail.repos.map((item) => <button type="button" key={item.root} className={repo?.root === item.root ? "is-selected" : ""} onClick={() => setRepoRoot(item.root)}>{item.name}</button>)}</div>}
    {error && <div className="scm-notice is-error" role="alert">{error}</div>}
    {result && <div className="scm-notice" role="status">
      {result.results.every((item) => item.restored) ? t.chat.checkpointRestored : t.chat.checkpointRestorePartial}
      {result.results.filter((item) => !item.restored).map((item) => <span key={item.root}> {item.root}: {item.error}</span>)}
      {result.undoId > 0 && <button type="button" className="chat-user-action" {...busyProps(busy, () => void undo(result.undoId))}>{t.chat.scmUndo}</button>}
    </div>}
    {loading || (turn != null && !detail) ? <div className="scm-empty">{t.chat.checkpointLoading}</div>
      : turn == null ? <div className="scm-empty">{t.chat.checkpointEmpty}</div>
      : !ready ? <div className="scm-empty">{repo?.error ?? t.chat.checkpointNotReady}</div>
      : repo.files.length === 0 ? <div className="scm-empty">{t.chat.checkpointNoFiles}</div>
      : <ChangesView
          key={`${turn}:${repo.root}`}
          cwd={repo.root}
          // 不显示分支名：这是某一回合当时的改动，不是当前分支的状态，挂着分支名会被当成工作区已经变了。
          summary={{ ...summary, branch: null, files: repo.files, ahead: null, behind: null }}
          comments={comments}
          touchedPaths={[]}
          turnTouchedPaths={[]}
          onAddComment={(comment) => onAddComment(comment)}
          onRemoveComment={onRemoveComment}
          escOverrideRef={escOverrideRef}
          live={false}
          fetchDiff={(path) => checkpointFileDiff(sessionId, turn, repo.root, path)}
          sessionCwd={cwd}
        />}
    {confirm && turn != null && <AppModal label={t.chat.checkpointRestoreTitle(turn)} onClose={() => setConfirm(null)}>
      <div className="chat-modal-title">{t.chat.checkpointRestoreTitle(turn)}</div>
      <p>{t.chat.checkpointRestoreNote}</p>
      <div className="chat-modal-actions">
        <button type="button" className="ns-btn" autoFocus onClick={() => setConfirm(null)}>{t.newSession.cancel}</button>
        <button type="button" className="ns-btn is-danger" {...busyProps(busy, () => void apply(confirm))}>{confirm === "start" ? t.chat.checkpointRestoreStart : t.chat.checkpointRestoreEnd}</button>
      </div>
    </AppModal>}
  </section>;
}
