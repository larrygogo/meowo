import { memo, useContext } from "react";
import { useT } from "../../../i18n";
import { ToolDisclosure } from "../ToolDisclosure";
import { toolShortName } from "../ToolIcon";
import { ToolResultLine } from "../ToolResultLine";
import { ChatCwdContext, todoSummary, toolSummary } from "../ToolActivity";
import { outputLines, type ToolResultItem, type ToolUseItem } from "../shared";
import { ToolBodyText } from "./ToolBodyText";
import { languageForPath } from "../highlight";

/// 其他工具（对照 Kimi 的 GenericTool）：搜索/网络/待办/子任务/未识别工具的统一形态。
export const GenericTool = memo(function GenericTool({ item, result }: { item: ToolUseItem; result?: ToolResultItem }) {
  const t = useT();
  const cwd = useContext(ChatCwdContext);
  const kind = item.kind;
  const summary = toolSummary(item, cwd);
  // 命中条数优先用后端给的真实数字：回执可能被截断，或整包是一行 JSON（数正文恒为 1）。
  const count = kind === "search" && result && !result.is_error && !result.interrupted
    ? result.count ?? outputLines(result.text).length
    : null;
  const more = item.detail?.kind === "web" ? item.detail.more : 0;
  // 待办：行尾写完成进度（已完成 / 总数），与标题栏进度胶囊同一种写法。
  const todo = kind === "todo" ? todoSummary(item.summary) : null;
  const detailPath = item.detail && "path" in item.detail ? item.detail.path : null;
  const lang = languageForPath(detailPath ?? "") ?? (result && /^\s*(?:\{|\[)/.test(result.text) ? "json" : null);
  return (
    <ToolDisclosure
      item={item}
      result={result}
      summary={
        <>
          <span className="chat-tool-verb" data-kind={kind}>{toolShortName(item.name, kind, t)}</span>
          <span className="chat-tool-summary">{summary}</span>
          {more > 0 && <span className="chat-tool-count">{t.chat.toolMoreTargets(more)}</span>}
          {count !== null && <span className="chat-tool-count">{t.chat.toolFoundLines(count)}</span>}
          {todo && <span className="chat-tool-count">{todo.done}/{todo.total}</span>}
        </>
      }
      body={
        <>
          <div className="chat-tool-result">
            <span className="chat-tool-elbow" aria-hidden="true">⎿</span>
            <ToolResultLine item={item} result={result} kind={kind} detail={item.detail ?? null} patch={null} />
          </div>
          <ToolBodyText result={result} lang={lang} />
        </>
      }
    />
  );
});
