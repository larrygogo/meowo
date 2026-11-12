import { useT } from "../../i18n";
import { busyProps } from "../../busyButton";

/// 终端停在子代理视图、自动切不回主会话时的横幅（切回逻辑见 subagentView.ts）。
/// 说清楚此刻再发会交给谁，并给一键切回；不让用户去终端手动处理。
/// role=status 只罩文字：按钮放在通报区外，读屏不会把按钮当通报内容播报。
export function SubagentViewNotice({ name, busy, onBack }: { name: string | null; busy: boolean; onBack: () => void }) {
  const t = useT();
  return (
    <div className="chat-send-error">
      <span role="status">{t.chat.subagentViewNotice(name ?? t.chat.subagentViewUnnamed)}</span>
      <button type="button" className="chat-send-takeover" {...busyProps(busy, onBack)}>{t.chat.subagentViewBack}</button>
    </div>
  );
}
