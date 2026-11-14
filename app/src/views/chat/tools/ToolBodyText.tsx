import { memo } from "react";
import { useT } from "../../../i18n";
import { OutputBlock } from "../terminalText";
import { stripAnsi, type ToolResultItem } from "../shared";
import { useToolRunState } from "../toolState";

/// 工具行展开区的正文：有回执就是回执全文，没有就照实说它此刻的状态。
/// 四个渲染器（命令、读取、编辑、其他）共用，状态的说法只在这一处。
export const ToolBodyText = memo(function ToolBodyText({ result, lang }: { result?: ToolResultItem; lang?: string | null }) {
  const t = useT();
  const state = useToolRunState(result);
  const text = state === "running" ? t.chat.toolRunning
    : state === "waiting" ? t.chat.status.pending
    : state === "interrupted" ? stripAnsi(result?.text ?? "") || t.chat.interrupted
    : stripAnsi(result?.text ?? "") || t.chat.toolNoOutput;
  return <OutputBlock text={text} lang={result ? lang : null} />;
});
