/// 对话窗底部错误条上的「出路」按钮：重试、看终端画面、收起。
///
/// 为什么要有（走查 C-4）：切模型、切思考程度、撤回、读不到模型清单这几类失败，原先只有一句
/// 「请到终端检查」，错误条上既没有按钮也没有关闭钮（只能靠打字清掉），手机上更是死路——远程
/// 页根本没有终端页可去（用户 2026-11-01：「我在手机上远程，你让我去 agent 回答问题不是扯淡吗」）。
/// 口径（第三轮共同说明）：能在 Meowo 里做的给按钮，做不到的把原因写进文案，不写「去终端」。
///
/// - retry：Meowo 能自己再做一遍的（切模型、切思考程度、撤回前清终端输入框）。
/// - terminal：结论只在终端画面上（CLI 有没有真的切过去、残图在不在）。桌面给「去终端页」，
///   手机就地展开终端画面 + 按键（RemoteTerminalKeys，同软拦横幅与降级审批卡）。
///
/// 出路跟着那一条错误走：setSendError 换了文案或清空时一并作废（见 ChatWindow 的 setSendError 包装），
/// 不会把上一条错误的「重试」挂到下一条错误上。
import { useState } from "react";
import { useT } from "../../i18n";
import { remoteUi } from "../../remoteMode";
import { RemoteTerminalKeys } from "./RemoteTerminalKeys";

export type SendErrorExit = {
  /** 就地再做一遍。按下时焦点先交给输入框：错误条随之清空卸载，焦点不能掉到 body。 */
  retry?: () => void;
  /** 重试钮的文案（缺省「重试」）：撤回后清终端输入框那条说「清空终端输入框」，做什么写在按钮上。 */
  retryLabel?: string;
  /** 结论在终端画面上：桌面去终端页，手机就地看画面、按键。 */
  terminal?: boolean;
};

export function SendErrorExits({ exit, sessionId, onGoTerminal, onDismiss, dismissLabel, focusComposer }: {
  exit: SendErrorExit;
  sessionId: number;
  onGoTerminal: () => void;
  onDismiss: () => void;
  /** 收起钮的文案；残图拦截用它说「已清理，可以重试」。 */
  dismissLabel?: string;
  focusComposer: () => void;
}) {
  const t = useT();
  const remote = remoteUi();
  const [keysOpen, setKeysOpen] = useState(false);
  return (
    <>
      {exit.retry && (
        <button type="button" className="chat-send-takeover" onClick={() => { if (!remote) focusComposer(); exit.retry?.(); }}>
          {exit.retryLabel ?? t.chat.errorRetry}
        </button>
      )}
      {exit.terminal && (remote
        ? <button type="button" className="chat-send-takeover" aria-expanded={keysOpen} onClick={() => setKeysOpen((open) => !open)}>{t.chat.remoteOpenKeys}</button>
        : <button type="button" className="chat-send-takeover" onClick={onGoTerminal}>{t.chat.goTerminal}</button>)}
      <button type="button" className="chat-send-takeover" onClick={() => { if (!remote) focusComposer(); onDismiss(); }}>{dismissLabel ?? t.chat.slashMenuDismiss}</button>
      {remote && exit.terminal && keysOpen && <RemoteTerminalKeys sessionId={sessionId} />}
    </>
  );
}
