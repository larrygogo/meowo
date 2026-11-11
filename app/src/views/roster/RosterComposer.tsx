// 名单底部的常驻输入框。没有行展开时用来交代一个新结果（带着这句话去新建页挑项目和 CLI）；
// 展开了某一行时指向那个 agent，写明它的任务名，提交后带着这句话进它的现场，在那里发出。
//
// 为什么不在名单上直接把话写进它的终端：对话窗的发送有一整套守卫——终端停在子代理视图时先切回
// 主会话、按实时屏幕判定它是不是停在审批 / 提问上、暂存终端输入行里的残留草稿、运行中各家 CLI
// 用各自的排队键、发送队列防插键、交互式命令走菜单识别。名单上另写一条发送路绕开了全部这些
// （做空审查 2026-11-12 H3）。话带过去由对话窗发，守卫只有一份。
import { useRef, useState } from "react";
import { useT } from "../../i18n";
import { Icon } from "../../icons/Icon";
import { icArrowUp } from "../../icons/glyphs.chat";
import type { Item } from "../sticker/types";

export function RosterComposer({ target, targetTitle, onNewSession, onSay }: {
  /** 展开的那一行；null = 没有行展开。 */
  target: Item | null;
  targetTitle: string;
  /** 没有行展开时提交：带着这句话去新建页。 */
  onNewSession: (message: string) => void;
  /** 有行展开时提交：带着这句话进它的现场。 */
  onSay: (id: number, message: string) => void;
}) {
  const t = useT();
  const [text, setText] = useState("");
  const inputRef = useRef<HTMLTextAreaElement>(null);

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

  const placeholder = target ? t.roster.composeTo(targetTitle) : t.roster.composeNew;
  return (
    <form className="roster-composer" onSubmit={(event) => { event.preventDefault(); submit(); }}>
      {/* 这句话是给谁的，一直写在输入框上面：placeholder 一打字就看不见了，原先为派新 agent 写的字
          会在点开一行之后悄悄换了去向（做空审查 M4）。 */}
      <span className="roster-composer-to">{target ? t.roster.composeTo(targetTitle) : t.roster.composeNewLabel}</span>
      <div className="roster-composer-box">
        <textarea
          ref={inputRef}
          rows={1}
          value={text}
          placeholder={placeholder}
          aria-label={placeholder}
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
