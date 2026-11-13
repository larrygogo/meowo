import { useEffect, useRef, useState } from "react";
import { managedConversationRecoveries } from "../../api";
import type { ManagedNativeRecoveryChunkDto } from "../../generated/contracts/ManagedNativeRecoveryChunkDto";
import type { ManagedNativeRecoveryCursorDto } from "../../generated/contracts/ManagedNativeRecoveryCursorDto";
import { formatBackendError } from "../../i18n/errors";
import { useT } from "../../i18n";

export type RecoveryMessage = {
  sequence: number; createdAt: number; messageIndex: number; role: string;
  text: string; bytes: number; complete: boolean; parts: Map<number, string>;
};

export function mergeRecoveryChunks(previous: RecoveryMessage[], chunks: ManagedNativeRecoveryChunkDto[]): RecoveryMessage[] {
  const messages = new Map(previous.map((message) => [`${message.sequence}:${message.messageIndex}`, { ...message, parts: new Map(message.parts) }]));
  for (const chunk of chunks) {
    const key = `${chunk.sequence}:${chunk.messageIndex}`;
    const message = messages.get(key) ?? { sequence: chunk.sequence, createdAt: chunk.createdAt, messageIndex: chunk.messageIndex, role: chunk.role, text: "", bytes: 0, complete: false, parts: new Map<number, string>() };
    // byte offset 不能当JS字符串下标；重复页只接受同offset同内容，缺块或冲突时保留已有记录并报错。
    if (message.parts.has(chunk.textByteOffset)) {
      if (message.parts.get(chunk.textByteOffset) !== chunk.text) throw new Error("conversation/recovery_payload_invalid");
      continue;
    }
    if (message.bytes !== chunk.textByteOffset || message.role !== chunk.role || message.complete) throw new Error("conversation/recovery_payload_invalid");
    message.parts.set(chunk.textByteOffset, chunk.text);
    message.text += chunk.text;
    message.bytes += new TextEncoder().encode(chunk.text).length;
    message.complete = chunk.complete;
    messages.set(key, message);
  }
  return [...messages.values()].sort((left, right) => right.sequence - left.sequence || left.messageIndex - right.messageIndex);
}

export function ManagedRecoveries({ sessionId }: { sessionId: number }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [messages, setMessages] = useState<RecoveryMessage[]>([]);
  const [cursor, setCursor] = useState<ManagedNativeRecoveryCursorDto | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pending = useRef(false);
  const mounted = useRef(false);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  // 恢复记录是排障入口、极少用到：先悄悄查一条，没有就整个不画（此前常驻一颗大按钮，
  // 点开还用正文大字写「暂无原生恢复记录」，leader 截图嫌丑，2026-11-13）。查询失败同样不画。
  const [hasAny, setHasAny] = useState(false);
  useEffect(() => {
    let live = true;
    managedConversationRecoveries(sessionId, null, 1)
      .then((page) => { if (live) setHasAny(page.chunks.length > 0); })
      .catch(() => {});
    return () => { live = false; };
  }, [sessionId]);
  const load = async (restart = false) => {
    if (pending.current) return;
    pending.current = true; setBusy(true); setError(null);
    try {
      const page = await managedConversationRecoveries(sessionId, restart ? null : cursor, 50);
      if (!mounted.current) return;
      // 只追加独立的原生恢复记录，永远不交给主时间线的ChatItem reducer猜重或改写live正文。
      setMessages(mergeRecoveryChunks(restart ? [] : messages, page.chunks));
      setCursor(page.nextCursor); setLoaded(true);
    } catch (reason) { if (mounted.current) setError(formatBackendError(reason, t.locale)); }
    finally { pending.current = false; if (mounted.current) setBusy(false); }
  };
  if (!hasAny && !open) return null;
  return <section className="chat-engine-recoveries">
    <div className="chat-engine-gate">
      <span data-tip={t.chat.engineRecoveries}>{t.chat.engineRecoveriesFound}</span>
      <button type="button" className="chat-queued-interrupt" aria-expanded={open} onClick={() => { setOpen((value) => !value); if (!open) void load(true); }}>{open ? t.chat.close : t.chat.engineOpenRecoveries}</button>
    </div>
    {open && <div className="chat-engine-recovery-list" role="region" aria-label={t.chat.engineRecoveries}>
      {error && <div role="alert" className="chat-send-error">{error}<button type="button" className="chat-queued-interrupt" disabled={busy} onClick={() => void load()}>{t.chat.retryNow}</button></div>}
      {loaded && !messages.length && <p className="chat-engine-recovery-meta">{t.chat.engineNoRecoveries}</p>}
      {messages.map((message, index) => <article key={`${message.sequence}:${message.messageIndex}`}>
        {(index === 0 || messages[index - 1].sequence !== message.sequence) && <p className="chat-engine-recovery-meta">{new Date(message.createdAt).toLocaleString(t.locale)}</p>}
        <p className="chat-engine-recovery-role">{t.chat.engineRecoveryRole(message.role)}</p>
        <pre className="chat-approval-input">{message.text}</pre>
        {!message.complete && <p className="chat-engine-recovery-meta">{t.chat.engineRecoveryIncomplete}</p>}
      </article>)}
      {cursor !== null && <button type="button" className="chat-queued-interrupt" disabled={busy} onClick={() => void load()}>{t.chat.engineMoreRecoveries}</button>}
      {busy && <span className="chat-engine-recovery-meta" role="status">{t.chat.engineLoadingRecoveries}</span>}
    </div>}
  </section>;
}
