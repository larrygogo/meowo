// 「更改」面板（SourceControl / ForgeBar）的提示行：红字失败或灰字结果，后面跟能点的下一步和关闭。
//
// 统一口径（第三轮设计准则，walkthrough P-1 / P-14 2026-11-12）：失败要说原因、有下一步就给按钮；带按钮或带原因
// 的提示不自动消失，停到用户关掉或做下一次操作。以前红字只能等下一次操作才清，用户以为还锁着（P-14）；
// 「先拉取再推」「去终端运行 git config」也没有任何能点的出路（P-1）。
//
// 焦点（做空第一轮 #3，2026-11-12）：提示条上的「撤销」「×」「重试」「拉取并重试」点完，调用方几乎都会先清掉
// 提示（run / begin 开头 setNotice(null)），按钮随提示条卸载，焦点掉到 body，键盘用户得从窗口顶上重新找。
// 统一在这里接：卸载时（或拿着焦点的按钮被换掉时）焦点还在条里，就交给 focusAfter（「更改」面板给提交框），
// 没给就交给条前面最近的控件（ForgeBar 里是推送按钮 / PR 那一栏）。
import { useLayoutEffect, useRef } from "react";
import { useT } from "../../i18n";
import { Icon } from "../../icons/Icon";
import { icClose } from "../../icons/glyphs";
import { focusBefore } from "../../focusRestore";

export type PanelAction = { label: string; onClick: () => void; testId?: string };

export function PanelMessage({ tone, text, actions = [], onDismiss, testId, focusAfter }: {
  tone: "error" | "notice";
  text: string;
  actions?: readonly PanelAction[];
  /** 给了就出一个 ×：红字一律给，结果提示带按钮时也给。 */
  onDismiss?: () => void;
  testId?: string;
  /** 提示条消失时焦点去哪；返回 false（或不给）就落到条前面最近的控件。 */
  focusAfter?: () => boolean;
}) {
  const t = useT();
  const boxRef = useRef<HTMLDivElement>(null);
  const handoff = useRef(focusAfter);
  // 拿着焦点的那颗按钮：提示条还在、只是按钮被换掉（换成另一条没有按钮的提示）时也要接住。
  const held = useRef<HTMLElement | null>(null);
  useLayoutEffect(() => {
    handoff.current = focusAfter;
    const box = boxRef.current;
    if (box && held.current && !held.current.isConnected && document.activeElement === document.body) {
      held.current = null;
      if (!handoff.current?.()) focusBefore(box);
    }
  });
  useLayoutEffect(() => {
    const box = boxRef.current;
    // 卸载时 React 先跑这里、再摘 DOM：此刻焦点还在条里的按钮上，挪走它就不会掉到 body。
    return () => {
      if (!box || !box.contains(document.activeElement)) return;
      if (!handoff.current?.()) focusBefore(box);
    };
  }, []);
  return (
    <div ref={boxRef} className={(tone === "error" ? "scm-error" : "scm-notice") + " scm-msg"} role={tone === "error" ? "alert" : "status"} data-testid={testId}
      onFocus={(event) => { held.current = event.target as HTMLElement; }}>
      <span className="scm-msg-text">{text}</span>
      {actions.map((action) => (
        <button key={action.label} type="button" className="scm-link-btn" data-testid={action.testId} onClick={action.onClick}>{action.label}</button>
      ))}
      {onDismiss && (
        <button type="button" className="scm-msg-close" aria-label={t.chat.scmDismiss} data-tip={t.chat.scmDismiss} onClick={onDismiss}>
          <Icon glyph={icClose} size={12} />
        </button>
      )}
    </div>
  );
}

/** 后端错误的原因码（`git/locked`、`git/push_rejected` 之类）：冒号前那段，没有冒号就是全文。 */
export function errorCode(e: unknown): string {
  const raw = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
  const colon = raw.indexOf(":");
  return (colon > 0 ? raw.slice(0, colon) : raw).trim();
}
