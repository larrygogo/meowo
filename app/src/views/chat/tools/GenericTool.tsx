import { memo, useContext } from "react";
import { useT } from "../../../i18n";
import { ToolDisclosure } from "../ToolDisclosure";
import { toolKind, toolShortName } from "../ToolIcon";
import { ToolResultLine } from "../ToolResultLine";
import { ChatCwdContext, toolSummary } from "../ToolActivity";
import { OutputBlock } from "../terminalText";
import { outputLines, stripAnsi, type ToolResultItem, type ToolUseItem } from "../shared";
import { languageForPath } from "../highlight";

/// 其他工具（对照 Kimi 的 GenericTool）：搜索/网络/待办/子任务/未识别工具的统一形态。
export const GenericTool = memo(function GenericTool({ item, result }: { item: ToolUseItem; result?: ToolResultItem }) {
  const t = useT();
  const cwd = useContext(ChatCwdContext);
  const kind = toolKind(item.name);
  const summary = toolSummary(item, cwd);
  const count = kind === "search" && result ? outputLines(result.text).length : null;
  const resultText = result ? stripAnsi(result.text) || t.chat.toolNoOutput : t.chat.toolRunning;
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
          {count !== null && <span className="chat-tool-count">{t.chat.toolFoundLines(count)}</span>}
        </>
      }
      body={
        <>
          <div className="chat-tool-result">
            <span className="chat-tool-elbow" aria-hidden="true">⎿</span>
            <ToolResultLine item={item} result={result} kind={kind} detail={item.detail ?? null} patch={null} />
          </div>
          <OutputBlock text={resultText} lang={lang} />
        </>
      }
    />
  );
});
