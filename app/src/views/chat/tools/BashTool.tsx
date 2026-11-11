import { memo } from "react";
import { useT } from "../../../i18n";
import { ToolDisclosure } from "../ToolDisclosure";
import { toolShortName } from "../ToolIcon";
import { ToolResultLine } from "../ToolResultLine";
import { CommandLines, OutputBlock } from "../terminalText";
import { friendlyToolName } from "../ToolActivity";
import { stripAnsi, type ToolResultItem, type ToolUseItem } from "../shared";

/// Bash 工具（对照 Kimi 的 BashTool）：摘要「运行 + 命令」，展开区是命令与完整输出。
export const BashTool = memo(function BashTool({ item, result }: { item: ToolUseItem; result?: ToolResultItem }) {
  const t = useT();
  const detail = item.detail?.kind === "command" ? item.detail : null;
  const command = detail?.command ?? item.summary;
  return (
    <ToolDisclosure
      item={item}
      result={result}
      summary={
        <>
          <span className="chat-tool-verb" data-kind="run">{toolShortName(item.name, "run", t)}</span>
          <span className="chat-tool-name chat-sronly">{friendlyToolName(item.name, t)}</span>
          <span className="chat-tool-summary" data-tip={detail?.description ?? undefined}>{command}</span>
        </>
      }
      body={
        <>
          {/* 成功回执不再放预览行（头 3 行 +「还有 N 行」）：下面的 OutputBlock 就是全文，再放预览就是
              前 3 行显示两遍，还自相矛盾地写「还有 57 行」（UI 调研 chat #10）。跑着时的计时、失败时的
              首行报错只有一行，照旧留着当「⎿ 结果摘要」。 */}
          {(!result || result.is_error) && (
            <div className="chat-tool-result">
              <span className="chat-tool-elbow" aria-hidden="true">⎿</span>
              <ToolResultLine item={item} result={result} kind="run" detail={detail} patch={null} />
            </div>
          )}
          {detail && <CommandLines command={detail.command} />}
          <OutputBlock text={result ? stripAnsi(result.text) || t.chat.toolNoOutput : t.chat.toolRunning} />
        </>
      }
    />
  );
});
