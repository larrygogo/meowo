// 名单底部的常驻输入框。没有行展开时用来交代一个新结果（带着这句话去新建页挑项目和 CLI）；
// 展开了某一行时指向那个 agent，写明它的任务名，话直接发进它的主会话，不用先钻进去。
import { useRef, useState } from "react";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { Icon } from "../../icons/Icon";
import { icArrowUp } from "../../icons/glyphs.chat";
import { submitToTerminal } from "../chat/terminalSubmit";
import { cardTone } from "../sticker/helpers";
import type { Item } from "../sticker/types";

/**
 * 能不能从名单直接对它说话。要 Meowo 持有它的终端，且它没停在审批 / 提问上：那时终端里的输入框
 * 不在前台，写进去的字会落到选择器上（对话窗里同一条守卫，见 submitToTerminal 的 abortIf）。
 */
export function canTalkTo(item: Item): boolean {
  if (!item.pty_managed || !item.connected || item.background || item.foreign) return false;
  const tone = cardTone(item);
  return tone === "running" || tone === "waiting" || tone === "on";
}

export function RosterComposer({ target, targetTitle, latest, onNewSession }: {
  /** 展开的那一行；null = 没有行展开。 */
  target: Item | null;
  targetTitle: string;
  /** 取这个会话此刻最新的那份数据（发回车前复核用：它可能刚好停到审批上了）。 */
  latest: (id: number) => Item | undefined;
  /** 没有行展开时提交：带着这句话去新建页。 */
  onNewSession: (message: string) => void;
}) {
  const t = useT();
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const talk = target ? canTalkTo(target) : false;
  const blocked = target != null && !talk;

  const submit = () => {
    const message = text.trim();
    if (busy) return;
    if (!target) {
      // 空着提交也去新建页：就是「我要派一个 agent」。
      onNewSession(message);
      setText("");
      return;
    }
    if (blocked) return;
    if (!message) return;
    const id = target.session.id;
    setBusy(true);
    setError(null);
    submitToTerminal(
      id,
      message,
      // 正文落下后、回车发出前再看一眼：它要是停到审批 / 提问上了，这个回车会替你答掉它。
      () => { const now = latest(id); return now && canTalkTo(now) ? null : t.roster.sayBlocked; },
      // 写完先确认正文真的出现在它的输入框里，再发回车（没回显 = 没进输入框，不发）。
      { message: t.roster.sayNotLanded },
    )
      .then(() => setText(""))
      .catch((raw) => setError(raw instanceof Error ? raw.message : formatBackendError(raw, t.locale)))
      .finally(() => { setBusy(false); inputRef.current?.focus(); });
  };

  const placeholder = !target ? t.roster.composeNew : blocked ? t.roster.composeBlocked(targetTitle) : t.roster.composeTo(targetTitle);
  return (
    <form className="roster-composer" onSubmit={(event) => { event.preventDefault(); submit(); }}>
      <div className="roster-composer-box">
        <textarea
          ref={inputRef}
          rows={1}
          value={text}
          disabled={busy || blocked}
          placeholder={placeholder}
          aria-label={placeholder}
          onChange={(event) => { setText(event.target.value); setError(null); }}
          onKeyDown={(event) => {
            // 回车发送，Shift+回车换行；输入法正在组字时的回车不算。
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); submit(); }
          }}
        />
        {/* 不能直接说话的行不放发送钮：进它现场的按钮就在上面那一行里，这里再放一个是重复。 */}
        {!blocked && (
          <button type="submit" className="roster-send" disabled={busy || (target != null && !text.trim())} aria-label={target ? t.roster.send : t.roster.sendNew}>
            <Icon glyph={icArrowUp} size={16} />
          </button>
        )}
      </div>
      {error && <span className="roster-composer-error" role="alert">{error}</span>}
    </form>
  );
}
