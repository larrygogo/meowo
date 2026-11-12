/// broker 审批的「回应」侧：结算锁、按请求绑定的失败原因、数字键映射、审批卡本体。
///
/// 为什么单独成模块（对照 Kimi 的 kimi.approvals 仓库）：接收侧早已抽到 useApprovalChannel，
/// 回应侧却散在 ChatWindow 里——审批与结构化提问靠约定共用一个 resolvingApproval，「说明并
/// 拒绝」的表单状态靠 effect 在换请求时手动清零。这里把结算收成一个 hook，卡片按 requestId
/// 挂载（key），卡内状态随请求自然重置，不再需要「记得清」。

import { useEffect, useRef, useState, type RefObject } from "react";
import { resolvePendingApproval, type PendingApproval } from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { remoteUi } from "../../remoteMode";
import { ApprovalCard, ApprovalCommandDetail, isRiskyCommand } from "./ApprovalCard";
import { ApprovalScopeTags } from "./ApprovalScopeTags";
import { commandOfInput } from "./approvalScope";

type Dict = ReturnType<typeof useT>;

export type ResolveResult = { ok: true } | { ok: false; message: string };

/// broker 挂起请求（审批与结构化提问同一条 resolvePendingApproval 通道）的结算。
/// 一个会话同一时刻只结算一件：连点、键盘与按钮同时触发都只发一次。
/// 失败原因按 requestId 记：轮询换上新请求后，旧请求的错误不污染新卡。
export function useBrokerResolver(sessionId: number) {
  const t = useT();
  const [resolving, setResolving] = useState(false);
  const [error, setError] = useState<{ requestId: string; message: string } | null>(null);
  const resolve = async (requestId: string, choice: string): Promise<ResolveResult | null> => {
    if (resolving) return null;
    setResolving(true);
    try {
      await resolvePendingApproval(sessionId, requestId, choice);
      setError((current) => (current?.requestId === requestId ? null : current));
      return { ok: true };
    } catch (raw) {
      const message = formatBackendError(raw, t.locale);
      setError({ requestId, message });
      return { ok: false, message };
    } finally {
      setResolving(false);
    }
  };
  const errorFor = (requestId: string) => (error?.requestId === requestId ? error.message : null);
  return { resolving, resolve, errorFor };
}

/// 数字键 → 审批选择（对照 Kimi 审批卡的 1/2/3）：1 = 允许一次，2.. = 依次的持久放行。
/// 超出持久放行数量的数字不对应任何按钮，返回 null（不吞键）。Enter 刻意不映射——
/// 输入框外随手一个回车就放行一条命令，换不来那点快捷。
export function approvalChoiceForKey(key: string, suggestionCount: number): string | null {
  if (!/^[1-9]$/.test(key)) return null;
  const digit = Number(key);
  if (digit === 1) return "allow_once";
  return digit - 2 < suggestionCount ? `suggestion:${digit - 2}` : null;
}

/// 焦点所在处另有数字键的主人：输入框 / 文本框（数字是要打的字）、按钮与链接（键归它自己）。
/// 窗口级的数字键监听据此让开；卡上的「1」「2」徽标也按同一判据显隐——走查 C-5：默认焦点就在
/// 输入框，徽标却一直亮着，按下去只把数字打进输入框（p2-digit-in-ta），徽标说了假话。
/// 数字键刻意不像 Esc 那样在空输入框里放行：Esc 拒绝有两段式确认，放行一条命令没有，空框里
/// 随手敲个「1」开头的消息就替用户放行了（ChatWindow.session.test「焦点在输入框：数字键属于输入」）。
export function digitKeysOwnedElsewhere(active: Element | null): boolean {
  if (!(active instanceof HTMLElement)) return false;
  if (active.tagName === "TEXTAREA" || active.tagName === "INPUT" || active.isContentEditable) return true;
  return !!active.closest("button, a, [role='button'], summary");
}

/// 数字键此刻是否归审批卡（徽标显隐用）：跟着焦点走，焦点进出输入框、按钮时重算。
function useDigitKeysLive(enabled: boolean): boolean {
  const [live, setLive] = useState(() => enabled && !digitKeysOwnedElsewhere(document.activeElement));
  useEffect(() => {
    if (!enabled) return;
    // focusout 时 activeElement 还是旧元素，等这一轮焦点转移落定再读。
    const update = () => window.setTimeout(() => setLive(!digitKeysOwnedElsewhere(document.activeElement)), 0);
    update();
    document.addEventListener("focusin", update);
    document.addEventListener("focusout", update);
    return () => {
      document.removeEventListener("focusin", update);
      document.removeEventListener("focusout", update);
    };
  }, [enabled]);
  return enabled && live;
}

function suggestionParts(suggestion: unknown, index: number, t: Dict): { base: string; detail: string } {
  if (!suggestion || typeof suggestion !== "object" || Array.isArray(suggestion)) {
    return { base: t.chat.allowSuggested(index + 1), detail: "" };
  }
  const entry = suggestion as Record<string, unknown>;
  const base = (() => { switch (entry.destination) {
    case "session": return t.chat.allowSession;
    case "localSettings": return t.chat.allowLocalProject;
    case "projectSettings": return t.chat.allowProject;
    case "userSettings": return t.chat.allowUser;
    default: return t.chat.allowSuggested(index + 1);
  } })();
  const firstRule = Array.isArray(entry.rules) ? entry.rules[0] : null;
  if (!firstRule || typeof firstRule !== "object" || Array.isArray(firstRule)) return { base, detail: "" };
  const rule = firstRule as Record<string, unknown>;
  const tool = typeof rule.toolName === "string" ? rule.toolName : "";
  const content = typeof rule.ruleContent === "string" ? rule.ruleContent : "";
  return { base, detail: content || tool };
}

export function approvalSuggestionLabel(suggestion: unknown, index: number, t: Dict): string {
  const { base, detail } = suggestionParts(suggestion, index, t);
  if (!detail) return base;
  const short = detail.length > 42 ? detail.slice(0, 41) + "…" : detail;
  return `${base} · ${short}`;
}

/// 悬浮提示与按钮文案同源，但不截断：按钮上 42 字截断的长命令在这里能看全。
/// 此前直接 JSON.stringify 整个建议——把协议内部结构甩给用户，谁也读不动。
export function approvalSuggestionTip(suggestion: unknown, index: number, t: Dict): string {
  const { base, detail } = suggestionParts(suggestion, index, t);
  return detail ? `${base} · ${detail}` : base;
}

/// claude hook 劫走的审批请求的卡片。调用方以 `key={approval.requestId}` 挂载：换请求即
/// 新卡，「说明并拒绝」的展开与草稿随之归零，理由不会串到下一张卡。
/// 注意它**没有**「仅收起」：收起等于让 hook 干等到 300s 超时，只能 allow/deny/持久放行。
export function BrokerApprovalCard({ approval, countdown, timedOut, resolving, error, escArmed, dirs, returnFocusTo, onDecide }: {
  approval: PendingApproval;
  countdown: string | null;
  timedOut: boolean;
  resolving: boolean;
  error: string | null;
  /** Esc 两段式拒绝已武装（第一下 Esc 点亮，状态归窗口级键盘处理所有）。 */
  escArmed: boolean;
  dirs: readonly string[];
  returnFocusTo: RefObject<HTMLElement | null>;
  onDecide: (choice: string) => void;
}) {
  const t = useT();
  // 只有命令可看时摆命令本身（判定见 commandOfInput），否则照旧摆完整参数。
  const command = commandOfInput(approval.input);
  const [reasonOpen, setReasonOpen] = useState(false);
  const reasonButtonRef = useRef<HTMLButtonElement>(null);
  const [reason, setReason] = useState("");
  // 远程没有键盘快捷键可言，kbd 徽章只在桌面挂。
  const kbd = !remoteUi();
  // 数字徽标只在按下去真生效时显示（Esc 徽标在空输入框里也生效，照常常驻）。
  const digits = useDigitKeysLive(kbd);
  // 结算在途：按钮用 aria-disabled 而不是 disabled（走查 C-3，p1-broker-fail）。disabled 会把刚
  // 按下、正聚焦的按钮的焦点踢到 body，失败时用户找不回刚才的位置，只能从窗口顶上重新 Tab。
  // 现在焦点留在原按钮上：失败可原地重按或换个选项；成功则卡片卸载，焦点由 returnFocusTo 交回输入框。
  // 重复提交由 useBrokerResolver 的结算锁挡住，这里的 decide 只是不让点击发出去。
  const busy = resolving || undefined;
  const decide = (choice: string) => { if (!resolving) onDecide(choice); };
  // 「说明并拒绝」空着提交多半是手快（走查 C-7：p3-reason-empty 截获 deny: 空理由），不算数。
  // 想直接拒绝有旁边的「拒绝」。
  const reasonReady = reason.trim().length > 0;
  return (
    <ApprovalCard
      returnFocusTo={returnFocusTo}
      title={t.chat.approvalTitle}
      // 300s 超时回落终端处理——徽章带剩余时间，让「卡片会过期」这件事可见；
      // 归零后切「已超时」态，事件到达收卡前不留定格的 0:00。
      badge={timedOut ? t.chat.approvalTimedOut : countdown ? `${t.chat.approvalPending} · ${countdown}` : t.chat.approvalPending}
      // `?? []`：类型上字段恒在（DTO 保证），但旧后端/新前端错配时负载可能缺它——
      // 一个可选按钮组不值得让整个对话窗白屏。持久放行归左侧组：它的作用域比「这一次」
      // 大得多，不该和本次决定并排争主位。
      sideActions={(approval.permissionSuggestions ?? []).map((suggestion, index) => (
        <button
          type="button"
          className="is-persistent"
          key={index}
          data-tip={approvalSuggestionTip(suggestion, index, t)}
          aria-disabled={busy}
          onClick={() => decide(`suggestion:${index}`)}
        >{approvalSuggestionLabel(suggestion, index, t)}{digits && index < 8 && <kbd aria-hidden="true">{index + 2}</kbd>}</button>
      ))}
      actions={<>
        {/* 右端两颗是「就这一次」的决定：拒绝（中性，也是 Esc 的落点）、允许一次（主按钮）。
            危险命令时允许钮降级为红色警示主按钮(is-danger,判定见 isRiskyCommand)。 */}
        <button ref={reasonButtonRef} type="button" className="is-deny-reason" aria-disabled={busy} aria-expanded={reasonOpen} onClick={() => { if (!resolving) setReasonOpen((open) => !open); }}>
          {t.chat.denyWithReason}
        </button>
        <button type="button" className={"is-deny" + (escArmed ? " is-esc-armed" : "")} aria-disabled={busy} onClick={() => decide("deny")}>
          {escArmed ? t.chat.denyConfirmEsc : t.chat.deny}{kbd && <kbd aria-hidden="true">Esc</kbd>}
        </button>
        <button type="button" className={"btn-primary" + (isRiskyCommand(approval.input) ? " is-danger" : "")} aria-disabled={busy} onClick={() => decide("allow_once")}>
          {t.chat.allowOnce}{digits && <kbd aria-hidden="true">1</kbd>}
        </button>
      </>}
    >
      {/* 次要信息（请求工具、影响范围、说明）收在一组：平时工具与标签同一行，矮窗里整组压成一行，
          把高度让给命令（做空审查：620×480 下命令被挤到折叠线下）。 */}
      <div className="chat-approval-meta">
        <div className="chat-approval-tool"><span>{t.chat.approvalTool}</span><code>{approval.toolName}</code></div>
        <ApprovalScopeTags tool={approval.toolName} input={approval.input} dirs={dirs} />
        {approval.description && <span className="chat-approval-desc">{approval.description}</span>}
      </div>
      {approval.input && <ApprovalCommandDetail
        label={command !== null ? t.chat.approvalCommand : t.chat.approvalInput}
        command={command ?? approval.input}
        expandLabel={t.chat.approvalExpand}
        collapseLabel={t.chat.approvalCollapse}
        copyLabel={t.chat.copyCommand}
        copiedLabel={t.chat.codeCopied}
      />}
      {/* 附理由拒绝：理由作为拒绝说明回给 Agent（后端 deny:<理由> → DenyWith）。
          Enter 提交、Esc 收起（只收起表单，不触发卡片的 Esc 拒绝）。 */}
      {reasonOpen && (
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
