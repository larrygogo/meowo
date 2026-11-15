import { useCallback, useEffect, useState } from "react";
import { prComment, prLineComment, prMonitorSet, prMonitorWake, prReview, type PrReviewDto } from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { AppModal } from "../AppModal";
import { diffLineAnchors, type DiffLineAnchor } from "./diffComments";
import { diffLineClass } from "./gitDiff";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import { busyProps } from "../../busyButton";

type Tab = "discussion" | "files" | "monitor";
type Submission = { kind: "comment" | "review" | "line"; anchor?: DiffLineAnchor };

export function PrReviewPanel({ cwd, sessionCwd, sessionId, number, active }: {
  cwd: string;
  sessionCwd: string | null;
  sessionId: number;
  number: number;
  active: boolean;
}) {
  const t = useT();
  const [data, setData] = useState<PrReviewDto | null>(null);
  const [tab, setTab] = useState<Tab>("discussion");
  const [path, setPath] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [submission, setSubmission] = useState<Submission | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const load = useCallback(() => {
    if (!active) return;
    prReview(cwd, sessionId, sessionCwd).then((value) => {
      setData(value);
      setPath((current) => current ?? value?.files[0]?.path ?? null);
    }).catch((cause) => setError(formatBackendError(cause, t.locale)));
  }, [active, cwd, sessionId, sessionCwd, t.locale]);
  useEffect(() => { load(); }, [load]);
  useTauriEvent<string>("pr-monitor-changed", (event) => {
    if (active && event.payload === data?.monitor?.key) load();
  });

  const submit = async () => {
    if (!submission || !draft.trim()) return;
    setBusy(true);
    setError("");
    try {
      if (submission.kind === "line" && submission.anchor) {
        await prLineComment(cwd, number, path ?? "", submission.anchor.line, submission.anchor.side === "old" ? "OLD" : "NEW", draft, sessionCwd);
      } else {
        await prComment(cwd, number, draft, submission.kind === "review", sessionCwd);
      }
      setDraft("");
      setSubmission(null);
      setConfirming(false);
      load();
    } catch (cause) {
      setError(formatBackendError(cause, t.locale));
      setConfirming(false);
    } finally {
      setBusy(false);
    }
  };
  const file = data?.files.find((item) => item.path === path) ?? null;
  const anchors = file?.patch ? diffLineAnchors(file.patch) : [];
  const monitor = data?.monitor;
  const wake = async (id: string) => {
    try {
      const state = await prMonitorWake(sessionId, monitor?.key ?? "", id);
      if (!["queued", "delivered"].includes(state)) setError(t.forge.monitorFailed);
      load();
    } catch (cause) {
      setError(formatBackendError(cause, t.locale));
    }
  };

  return <section className="pr-review" aria-label={t.forge.reviewOpen}>
    <div className="chat-diff-scope" role="tablist" aria-label={t.forge.reviewOpen}>
      <button type="button" role="tab" aria-selected={tab === "discussion"} onClick={() => setTab("discussion")}>{t.forge.reviewDiscussion}</button>
      <button type="button" role="tab" aria-selected={tab === "files"} onClick={() => setTab("files")}>{t.forge.reviewFiles}</button>
      <button type="button" role="tab" aria-selected={tab === "monitor"} onClick={() => setTab("monitor")}>{t.forge.reviewMonitor}</button>
    </div>
    {error && <div className="scm-notice is-error" role="alert">{error}</div>}
    {!data ? <div className="scm-empty">{t.forge.reviewLoading}</div> : <>
      {tab === "discussion" && <div className="pr-comments">
        {data.reviews.map((review) => <article key={review.id} className="pr-comment">
          <header>{review.author} · review {review.state ?? ""}</header><p>{review.body}</p>
        </article>)}
        {data.comments.map((comment) => <article key={comment.id} className="pr-comment">
          <header>{comment.author}{comment.path ? ` · ${comment.path}:${comment.line ?? ""}` : ""}</header><p>{comment.body}</p>
        </article>)}
        {data.comments.length + data.reviews.length === 0 && <div className="scm-empty">{t.forge.reviewEmpty}</div>}
      </div>}
      {tab === "files" && <div className="pr-files">
        <div className="pr-file-list">{data.files.map((item) => <button type="button" key={item.path} className={item.path === path ? "is-selected" : ""} onClick={() => setPath(item.path)}>
          {item.path} <span>+{item.additions} −{item.deletions}</span>
        </button>)}</div>
        {file?.patch && <pre className="pr-patch">{file.patch.split("\n").map((line, index) => {
          const anchor = anchors[index];
          return <div key={index} className={diffLineClass(line) || undefined}>
            {anchor && <button type="button" className="chat-diff-cmt-add" aria-label={t.forge.lineCommentSubmit} onClick={() => { setSubmission({ kind: "line", anchor }); setTab("files"); }}>+</button>}
            {line}
          </div>;
        })}</pre>}
        {file && !file.patch && <div className="scm-empty">{t.chat.diffBinary}</div>}
      </div>}
      {tab === "monitor" && <div className="pr-monitor">
        <button type="button" className="scm-forge-btn is-ghost" {...busyProps(busy, () => {
          setBusy(true);
          prMonitorSet(cwd, sessionId, !monitor?.enabled, sessionCwd, monitor?.key).then(load).catch((cause) => setError(formatBackendError(cause, t.locale))).finally(() => setBusy(false));
        })}>{monitor?.enabled ? t.forge.monitorStop : t.forge.monitorStart}</button>
        {monitor?.lastError && <div className="scm-notice is-error" role="alert">{monitor.lastError}</div>}
        {monitor?.events.map((event) => <article key={event.id} className="pr-event">
          <p>{event.summary}</p>
          {event.wakeState === "pending" ? <button type="button" className="chat-user-action" onClick={() => void wake(event.id)}>{t.forge.monitorWake}</button>
            : <span>{event.wakeState === "queued" ? t.forge.monitorQueued : event.wakeState === "delivered" ? t.forge.monitorDelivered : t.forge.monitorFailed}</span>}
        </article>)}
      </div>}
      {tab !== "monitor" && <div className="pr-compose">
        <textarea value={draft} aria-label={t.forge.commentPlaceholder} placeholder={t.forge.commentPlaceholder} onChange={(event) => setDraft(event.target.value)} />
        <div className="checkpoint-actions">
          <button type="button" className="chat-user-action" disabled={!draft.trim()} onClick={() => { setSubmission({ kind: "comment" }); setConfirming(true); }}>{t.forge.commentSubmit}</button>
          <button type="button" className="chat-user-action" disabled={!draft.trim()} onClick={() => { setSubmission({ kind: "review" }); setConfirming(true); }}>{t.forge.reviewSubmit}</button>
          {submission?.kind === "line" && <button type="button" className="chat-user-action" disabled={!draft.trim()} onClick={() => setConfirming(true)}>{t.forge.lineCommentSubmit}</button>}
        </div>
      </div>}
    </>}
    {confirming && submission && <AppModal label={t.forge.commentConfirm} onClose={() => { if (!busy) setConfirming(false); }}>
      <div className="chat-modal-title">{t.forge.commentConfirm}</div>
      <p>{submission.kind === "line" ? `${path}:${submission.anchor?.line ?? ""}` : `#${number}`}</p>
      <p className="pr-confirm-body">{draft}</p>
      <div className="chat-modal-actions">
        <button type="button" className="ns-btn" {...busyProps(busy, () => setConfirming(false))}>{t.newSession.cancel}</button>
        <button type="button" className="ns-btn btn-primary" {...busyProps(busy, () => void submit())}>{t.forge.commentConfirm}</button>
      </div>
    </AppModal>}
  </section>;
}
