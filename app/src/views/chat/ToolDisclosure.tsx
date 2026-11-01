import { memo, type ReactNode } from "react";
import { useRememberedDetails } from "./foldMemory";
import { fmtSpan, type ToolResultItem, type ToolUseItem } from "./shared";
import { ToolIcon } from "./ToolIcon";

/// 工具行的公共折叠外壳（对照 Kimi 的 ToolDisclosure）：图标、状态点、耗时、展开区。
/// 具体工具只提供 summary 与 body；复制集中在回合页脚，不在每行重复放按钮。
export const ToolDisclosure = memo(function ToolDisclosure({ item, result, summary, body, defaultOpen = false }: {
  item: ToolUseItem;
  result?: ToolResultItem;
  summary: ReactNode;
  body: ReactNode;
  defaultOpen?: boolean;
}) {
  const details = useRememberedDetails(`tool:${item.id}`, defaultOpen);
  const startedAt = Date.parse(item.timestamp ?? "");
  const endedAt = Date.parse(result?.timestamp ?? "");
  const span = result && Number.isFinite(startedAt) && Number.isFinite(endedAt) && endedAt - startedAt >= 1000
    ? fmtSpan(endedAt - startedAt)
    : null;
  return (
    <details
      className={"chat-tool" + (result?.is_error ? " is-error" : "")}
      data-state={!result ? "running" : result.is_error ? "error" : "done"}
      {...details}
    >
      <summary>
        <ToolIcon name={item.name} />
        {summary}
        {span && <span className="chat-tool-time">{span}</span>}
        {(!result || result.is_error) && <span className="chat-tool-state" aria-hidden="true" />}
        <span className="chat-tool-chevron">›</span>
      </summary>
      <div className="chat-tool-body">{body}</div>
    </details>
  );
});
