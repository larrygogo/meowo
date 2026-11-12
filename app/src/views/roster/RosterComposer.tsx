// 名单底部的常驻输入框。没有行展开时用来交代一个新结果（带着这句话去新建页挑项目和 CLI）；
// 展开了某一行时指向那个 agent，写明它的任务名，回车就发给它，人留在名单上。
//
// 名单自己不往终端写：对话窗的发送有一整套守卫——终端停在子代理视图时先切回主会话、按实时屏幕
// 判定它是不是停在审批 / 提问上、暂存终端输入行里的残留草稿、运行中各家 CLI 用各自的排队键、
// 发送队列防插键、交互式命令走菜单识别。名单上另写一条发送路会绕开全部这些（做空审查 2026-11-12
// H3）。所以是名单底下的对话窗切到那条会话、用它自己的发送发出去；守卫拦下时现场亮出来说明原因。
import { useEffect, useRef, useState } from "react";
import { useT } from "../../i18n";
import { Icon } from "../../icons/Icon";
import { icArrowUp } from "../../icons/glyphs.chat";
import type { Item } from "../sticker/types";

export function RosterComposer({ target, targetTitle, sentTo, onNewSession, onSay }: {
  /** 刚发出去的那一句给了谁（seq 每次发送加一）。 */
  sentTo?: { seq: number; title: string } | null;
  /** 展开的那一行；null = 没有行展开。 */
  target: Item | null;
  targetTitle: string;
  /** 没有行展开时提交：带着这句话去新建页。 */
  onNewSession: (message: string) => void;
  /** 有行展开时提交：发给它。 */
  onSay: (id: number, message: string) => void;
}) {
  const t = useT();
  const [text, setText] = useState("");
  const inputRef = useRef<HTMLTextAreaElement>(null);
  // 「已发给…」报一声就收：几秒后、或人又开始打字时，上面那行回到去向。
  const [shownSent, setShownSent] = useState<number | null>(null);
  useEffect(() => {
    if (!sentTo) return;
    setShownSent(sentTo.seq);
    const timer = window.setTimeout(() => setShownSent(null), 4000);
    return () => window.clearTimeout(timer);
  }, [sentTo]);

  const submit = () => {
    const message = text.trim();
    if (target) {
      if (!message) return;
      onSay(target.session.id, message);
    } else {
      // 空着提交也去新建页：就是「我要派一个 agent」。
      onNewSession(message);
    }
    setText("");
  };

  // 去向写在上面那一行，placeholder 不重复它：说清楚回车之后会发生什么。
  const label = target ? t.roster.composeTo(targetTitle) : t.roster.composeNew;
  const placeholder = target ? t.roster.composeToHint : t.roster.composeNew;
  return (
    <form className="roster-composer" onSubmit={(event) => { event.preventDefault(); submit(); }}>
      {/* 这句话是给谁的，一直写在输入框上面：placeholder 一打字就看不见了，原先为派新 agent 写的字
          会在点开一行之后悄悄换了去向（做空审查 M4）。 */}
      <span className="roster-composer-to" role="status">{sentTo && shownSent === sentTo.seq && !text ? t.roster.sentTo(sentTo.title) : target ? t.roster.composeTo(targetTitle) : t.roster.composeNewLabel}</span>
      <div className="roster-composer-box">
        <textarea
          ref={inputRef}
          rows={1}
          value={text}
          placeholder={placeholder}
          aria-label={label}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            // 回车提交，Shift+回车换行；输入法正在组字时的回车不算。
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); submit(); }
          }}
        />
        <button type="submit" className="roster-send" disabled={target != null && !text.trim()} aria-label={target ? t.roster.sayInScene : t.roster.sendNew}>
          <Icon glyph={icArrowUp} size={16} />
        </button>
      </div>
    </form>
  );
}
