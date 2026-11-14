/// 审批卡：broker 挂起的请求与屏幕上认出的审批框渲染同一张卡。
///
/// 卡片只认 ApprovalCardModel（见 requestCard.ts）：按钮的名称、顺序、键盘徽标五家相同，
/// 「说明并拒绝」「持久放行」「仅收起」按模型里的能力出现。决定通过 onDecide 交回调用方，
/// 怎么送达（broker 应答还是往终端写按键）由调用方按通道处理，卡片不关心。
///
/// 调用方以 `key={model.requestId}` 挂载：换请求即新卡，「说明并拒绝」的展开与草稿随之归零，
/// 理由不会串到下一张卡。

import { useRef, useState, type RefObject } from "react";
import { useT } from "../../i18n";
import { remoteUi } from "../../remoteMode";
import { ApprovalCard, ApprovalCommandDetail } from "./ApprovalCard";
import { ApprovalScopeTags } from "./ApprovalScopeTags";
import { useDigitKeysLive } from "./BrokerApprovalCard";
import { cardBadge, cardTitle, type ApprovalCardModel, type ApprovalChoice } from "./requestCard";

export function ApprovalRequestCard({ model, countdown, resolving, error, escArmed, dirs, returnFocusTo, onDecide, onCollapse }: {
  model: ApprovalCardModel;
  /** 剩余时间（broker 请求 300 秒后交回终端）；没有时限的请求传 null。 */
  countdown: string | null;
  resolving: boolean;
  error: string | null;
  /** Esc 两段式拒绝已武装（第一下 Esc 点亮，状态归窗口级键盘处理所有）。 */
  escArmed: boolean;
  dirs: readonly string[];
  returnFocusTo: RefObject<HTMLElement | null>;
  onDecide: (choice: ApprovalChoice) => void;
  /** 仅收起（模型声明了 collapse 能力才会用到）。 */
  onCollapse?: () => void;
}) {
  const t = useT();
  const [reasonOpen, setReasonOpen] = useState(false);
  const reasonButtonRef = useRef<HTMLButtonElement>(null);
  const [reason, setReason] = useState("");
  const { actions } = model;
  // 过期、外部终端里跑的会话：卡片照常显示内容，按钮不再可用。
  const answerable = model.live && model.state === "pending";
  // 远程没有键盘快捷键可言，kbd 徽章只在桌面挂。
  const kbd = !remoteUi() && answerable;
  // 数字徽标只在按下去真生效时显示（Esc 徽标在空输入框里也生效，照常常驻）。
  const digits = useDigitKeysLive(kbd);
  // 不可作答、结算在途：按钮用 aria-disabled 而不是 disabled（走查 C-3，p1-broker-fail）。disabled
  // 会把刚按下、正聚焦的按钮的焦点踢到 body，失败时用户找不回刚才的位置，只能从窗口顶上重新 Tab。
  // 现在焦点留在原按钮上：失败可原地重按或换个选项；成功则卡片卸载，焦点由 returnFocusTo 交回输入框。
  // 重复提交由调用方的结算锁挡住，这里的 decide 只是不让点击发出去。
  const blocked = resolving || !answerable;
  const busy = blocked || undefined;
  const decide = (choice: ApprovalChoice) => { if (!blocked) onDecide(choice); };
  // 「说明并拒绝」空着提交多半是手快（走查 C-7：p3-reason-empty 截获 deny: 空理由），不算数。
  // 想直接拒绝有旁边的「拒绝」。
  const reasonReady = reason.trim().length > 0;
  return (
    <ApprovalCard
      returnFocusTo={returnFocusTo}
      // 截图脚本与测试用这个类名找屏幕通道的卡；没有对应样式，两条通道长相相同。
      className={model.channel === "terminal" ? "chat-screen-approval" : undefined}
      title={cardTitle("approval", t)}
      badge={cardBadge("approval", model.state, t, countdown)}
      // 左侧组放「不作本次决定」的动作：零副作用的仅收起、范围比这一次大的持久放行。
      sideActions={(actions.collapse || actions.persistent.length > 0) ? <>
        {actions.collapse && onCollapse && <button type="button" className="chat-attention-dismiss is-inline" data-tip={t.chat.attentionDismissTip} onClick={onCollapse}>{t.chat.attentionDismiss}</button>}
        {actions.persistent.map((entry, index) => (
          <button
            type="button"
            className="is-persistent"
            key={index}
            data-tip={entry.tip}
            aria-disabled={busy}
            onClick={() => decide(`suggestion:${index}`)}
          >
            {/* 放行范围可能很长（规则原文、英文说明）：标签自己省略，全文在悬停提示里，不把按钮撑出卡片。 */}
            <span className="chat-approval-persist-label">{entry.label}</span>
            {digits && index < 8 && <kbd aria-hidden="true">{index + 2}</kbd>}
          </button>
        ))}
      </> : undefined}
      actions={<>
        {/* 右端是「就这一次」的决定：说明并拒绝、拒绝（中性，也是 Esc 的落点）、允许一次（主按钮）。
            危险命令时允许钮降级为红色警示主按钮（is-danger，判定见 isRiskyCommand）。 */}
        {actions.denyWithReason && <button ref={reasonButtonRef} type="button" className="is-deny-reason" aria-disabled={busy} aria-expanded={reasonOpen} onClick={() => { if (!blocked) setReasonOpen((open) => !open); }}>
          {t.chat.denyWithReason}
        </button>}
        {actions.deny && <button type="button" className={"is-deny" + (escArmed ? " is-esc-armed" : "")} aria-disabled={busy} onClick={() => decide("deny")}>
          {escArmed ? t.chat.denyConfirmEsc : t.chat.deny}{kbd && <kbd aria-hidden="true">Esc</kbd>}
        </button>}
        {actions.allowOnce && <button type="button" className={"btn-primary" + (model.risky ? " is-danger" : "")} aria-disabled={busy} onClick={() => decide("allow_once")}>
          {t.chat.allowOnce}{digits && <kbd aria-hidden="true">1</kbd>}
        </button>}
      </>}
    >
      {!model.live && model.state === "pending" && <span>{t.chat.questionExternalHint}</span>}
      {model.state === "expired" && <span>{t.chat.approvalTimedOut}</span>}
      {/* 次要信息（请求工具、影响范围、说明）收在一组：平时工具与标签同一行，矮窗里整组压成一行，
          把高度让给命令（做空审查：620×480 下命令被挤到折叠线下）。 */}
      <div className="chat-approval-meta">
        {model.tool && <div className="chat-approval-tool"><span>{t.chat.approvalTool}</span><code>{model.tool}</code></div>}
        {model.scopeInput && <ApprovalScopeTags tool={model.tool} input={model.scopeInput} dirs={dirs} />}
        {model.description && <span className="chat-approval-desc">{model.description}</span>}
      </div>
      {model.question && <span>{model.question}</span>}
      {model.detail && <ApprovalCommandDetail
        label={model.detail.label === "command" ? t.chat.approvalCommand : t.chat.approvalInput}
        command={model.detail.text}
        expandLabel={t.chat.approvalExpand}
        collapseLabel={t.chat.approvalCollapse}
        copyLabel={t.chat.copyCommand}
        copiedLabel={t.chat.codeCopied}
      />}
      {/* 附理由拒绝：理由作为拒绝说明回给 Agent（后端 deny:<理由> → DenyWith）。
          Enter 提交、Esc 收起（只收起表单，不触发卡片的 Esc 拒绝）。 */}
      {actions.denyWithReason && reasonOpen && (
        <form
          className="chat-approval-reason"
          onSubmit={(event) => {
            event.preventDefault();
            if (reasonReady) decide(`deny:${reason.trim()}`);
          }}
        >
          <input
            autoFocus
            value={reason}
            placeholder={t.chat.denyReasonPlaceholder}
            aria-label={t.chat.denyReasonPlaceholder}
            onChange={(event) => setReason(event.target.value)}
            onKeyDown={(event) => {
              // 收起时把焦点还给「说明并拒绝」：输入框随表单卸载，焦点会掉回 body，之后的按键
              // 没有落点（UI 优化 c13，chat s13 实测 after esc → BODY）。
              if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setReasonOpen(false); reasonButtonRef.current?.focus(); }
            }}
          />
          {/* 理由为空时提交钮不可用：此时焦点在理由框里，用 disabled 不会踢走焦点。 */}
          <button type="submit" disabled={!reasonReady} aria-disabled={busy}>{t.chat.denyReasonSubmit}</button>
        </form>
      )}
      {/* 提交失败的原因写进卡内（此前空 catch 完全静默，用户点了没反应）。 */}
      {error && <span className="chat-approval-error" role="alert">{error}</span>}
    </ApprovalCard>
  );
}
