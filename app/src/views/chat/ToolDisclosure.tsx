import { memo, type ReactNode } from "react";
import { useT } from "../../i18n";
import { useRememberedDetails } from "./foldMemory";
import { fmtSpan, type ToolResultItem, type ToolUseItem } from "./shared";
import { ToolIcon } from "./ToolIcon";
import { useToolRunState } from "./toolState";

/// 工具行的公共折叠外壳（对照 Kimi 的 ToolDisclosure）：图标、状态点、耗时、展开区。
/// 具体工具只提供 summary 与 body；复制集中在回合页脚，不在每行重复放按钮。
export const ToolDisclosure = memo(function ToolDisclosure({ item, result, summary, body, defaultOpen = false }: {
  item: ToolUseItem;
  result?: ToolResultItem;
  summary: ReactNode;
  body: ReactNode;
  defaultOpen?: boolean;
}) {
  const t = useT();
  const details = useRememberedDetails(`tool:${item.id}`, defaultOpen);
  const state = useToolRunState(result);
  const startedAt = Date.parse(item.timestamp ?? "");
  const endedAt = Date.parse(result?.timestamp ?? "");
  const span = result && Number.isFinite(startedAt) && Number.isFinite(endedAt) && endedAt - startedAt >= 1000
    ? fmtSpan(endedAt - startedAt)
    : null;
  return (
    <details
      className={"chat-tool" + (state === "error" ? " is-error" : "")}
      data-state={state}
      {...details}
    >
      <summary>
        <ToolIcon kind={item.kind} />
        {summary}
        {/* 等人、已中断写在行上：这两个词原先只在展开区里，而行默认收起，等审批时只看到一行没有任何标记的
            命令（2026-11-14 截图自验）。与会话状态、中断分隔用的是同一个词。 */}
        {state === "waiting" && <span className="chat-tool-status is-waiting">{t.vocab.request.approval}</span>}
        {state === "interrupted" && <span className="chat-tool-status">{t.chat.interrupted}</span>}
        {span && <span className="chat-tool-time">{span}</span>}
        {/* 状态点只给「在跑」和「失败」：等人、已中断是静止的事实，不需要一个亮点去抢眼。 */}
        {(state === "running" || state === "error") && <span className="chat-tool-state" aria-hidden="true" />}
        <span className="chat-tool-chevron">›</span>
      </summary>
      <div className="chat-tool-body">{body}</div>
    </details>
  );
});
