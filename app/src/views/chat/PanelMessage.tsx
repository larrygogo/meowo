// 「更改」面板（SourceControl / ForgeBar）的提示行：红字失败或灰字结果，后面跟能点的下一步和关闭。
//
// 统一口径（第三轮设计准则，walkthrough P-1 / P-14 2026-11-12）：失败要说原因、有下一步就给按钮；带按钮或带原因
// 的提示不自动消失，停到用户关掉或做下一次操作。以前红字只能等下一次操作才清，用户以为还锁着（P-14）；
// 「先拉取再推」「去终端运行 git config」也没有任何能点的出路（P-1）。
import { useT } from "../../i18n";
import { Icon } from "../../icons/Icon";
import { icClose } from "../../icons/glyphs";

export type PanelAction = { label: string; onClick: () => void; testId?: string };

export function PanelMessage({ tone, text, actions = [], onDismiss, testId }: {
  tone: "error" | "notice";
  text: string;
  actions?: readonly PanelAction[];
  /** 给了就出一个 ×：红字一律给，结果提示带按钮时也给。 */
  onDismiss?: () => void;
  testId?: string;
}) {
  const t = useT();
  return (
    <div className={(tone === "error" ? "scm-error" : "scm-notice") + " scm-msg"} role={tone === "error" ? "alert" : "status"} data-testid={testId}>
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
