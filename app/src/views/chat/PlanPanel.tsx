import { memo } from "react";
import type { ChatItem } from "../../api";
import { useT } from "../../i18n";
import { ChatMarkdown } from "../ChatMarkdown";

/// 最近一份计划正文：transcript 里最后一次 ExitPlanMode 提交的优先，历史窗口外的由后端
/// 保留的 fallback 兜底。入口显隐与面板正文共用这一份判定。
export function latestPlan(items: ChatItem[], fallbackPlan?: string | null): string | null {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (item.type === "tool_use" && item.plan?.trim()) return item.plan;
  }
  return fallbackPlan?.trim() ? fallbackPlan : null;
}

/// 计划是 ExitPlanMode 提交的完整 Markdown 文档，与 TodoWrite 的执行待办是两套数据。
/// 入口只在「有计划或开着计划模式」时出现（对照 Kimi 的 ChatDock），所以没有计划时
/// 只会是计划模式刚开、计划还没写出来。
export const PlanPanel = memo(function PlanPanel({ plan, pending, onClose }: { plan: string | null; pending: boolean; onClose: () => void }) {
  const t = useT();
  return (
    <div className="dock-work-panel panel-plan">
      <div className="dock-work-head">
        <span className="dock-work-title">
          <svg className="dock-work-title-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" />
          </svg>
          {t.chat.plan}
          {plan && <span className="dock-work-meta">{pending ? t.chat.planPending : t.chat.planApproved}</span>}
        </span>
        <button type="button" className="dock-work-close" aria-label={t.chat.close} onClick={onClose}>×</button>
      </div>
      <div className="dock-work-body">
        {plan ? <div className="chat-plan-doc chat-md"><ChatMarkdown text={plan} /></div> : <div className="chat-tasks-empty">{t.chat.planEmpty}</div>}
      </div>
    </div>
  );
});
