import { memo, useContext } from "react";
import { useT } from "../../../i18n";
import { ToolDisclosure } from "../ToolDisclosure";
import { toolShortName } from "../ToolIcon";
import { ToolResultLine } from "../ToolResultLine";
import { ChatCwdContext, toolSummary } from "../ToolActivity";
import { OutputBlock } from "../terminalText";
import { outputLines, stripAnsi, type ToolResultItem, type ToolUseItem } from "../shared";
import { languageForPath } from "../highlight";

/// Read 工具（对照 Kimi 的 ReadTool）：摘要「读取 + 文件 + 行号范围」，展开区是文件内容。
export const ReadTool = memo(function ReadTool({ item, result }: { item: ToolUseItem; result?: ToolResultItem }) {
  const t = useT();
  const cwd = useContext(ChatCwdContext);
  const detail = item.detail?.kind === "read" ? item.detail : null;
  const path = detail?.path ?? item.summary;
  const range = detail?.offset != null ? `:${detail.offset}${detail.limit != null ? `-${detail.offset + detail.limit}` : ""}` : "";
  const lines = result ? outputLines(result.text).length : 0;
  return (
    <ToolDisclosure
      item={item}
      result={result}
      summary={
        <>
          <span className="chat-tool-verb" data-kind="read">{toolShortName(item.name, "read", t)}</span>
          <span className="chat-tool-summary">{toolSummary(item, cwd)}{range}</span>
          {/* 失败时回执是错误信息，按行数算会写成「读取了 1 行」，把失败说成成功（UI 调研 chat #11）。 */}
          {result && <span className="chat-tool-count">{result.is_error ? t.chat.toolReadFailed : t.chat.toolReadLines(lines)}</span>}
        </>
      }
      body={
        <>
          <div className="chat-tool-result">
            <span className="chat-tool-elbow" aria-hidden="true">⎿</span>
            <ToolResultLine item={item} result={result} kind="read" detail={detail} patch={null} />
          </div>
          <OutputBlock text={result ? stripAnsi(result.text) || t.chat.toolNoOutput : t.chat.toolRunning} lang={languageForPath(path)} />
        </>
      }
    />
  );
});
