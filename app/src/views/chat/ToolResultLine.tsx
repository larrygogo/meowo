import { memo, useEffect, useState } from "react";
import { useT } from "../../i18n";
import type { DiffHunk } from "../../generated/contracts/DiffHunk";
import type { ToolDetail } from "../../generated/contracts/ToolDetail";
import { fmtElapsed } from "./cliStatus";
import { dedent } from "./terminalText";
import { lineDelta } from "./ToolActivity";
import { outputLines, type ToolResultItem, type ToolUseItem } from "./shared";
import type { ToolKind } from "./ToolIcon";

/// 结果摘要行（对照 Kimi 各工具的 meta/chip）：运行中计时、编辑增删、读取行数、
/// 搜索命中数、命令输出头几行。
function useSecondTick(active: boolean) {
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => setTick((value) => value + 1), 1000);
    return () => window.clearInterval(timer);
  }, [active]);
}

const OUTPUT_HEAD_LINES = 3;

export const ToolResultLine = memo(function ToolResultLine({ item, result, kind, detail, patch }: {
  item: ToolUseItem;
  result?: ToolResultItem;
  kind: ToolKind;
  detail: ToolDetail | null;
  patch: DiffHunk[] | null;
}) {
  const t = useT();
  const [mountedAt] = useState(() => Date.now());
  useSecondTick(!result);
  if (!result) {
    const startedAt = Date.parse(item.timestamp ?? "");
    const since = Number.isNaN(startedAt) ? mountedAt : startedAt;
    return <span className="chat-tool-out is-running">{t.chat.toolRunningFor(fmtElapsed(Date.now() - since))}</span>;
  }
  const lines = outputLines(result.text);
  if (result.is_error) return <span className="chat-tool-out is-error">{lines[0] ?? t.chat.toolNoOutput}</span>;
  if (kind === "edit" || kind === "write") {
    if (patch) {
      const added = patch.reduce((n, hunk) => n + hunk.lines.filter((line) => line[0] === "+").length, 0);
      const removed = patch.reduce((n, hunk) => n + hunk.lines.filter((line) => line[0] === "-").length, 0);
      return <span className="chat-tool-out">{t.chat.toolEditSummary(added, removed)}</span>;
    }
    if (detail?.kind === "write") return <span className="chat-tool-out">{t.chat.toolWroteLines(detail.lines)}</span>;
    const delta = lineDelta(detail);
    if (delta) return <span className="chat-tool-out">{t.chat.toolEditSummary(delta.added, delta.removed)}</span>;
  }
  if (kind === "read") return <span className="chat-tool-out">{t.chat.toolReadLines(lines.length)}</span>;
  if (kind === "search") return <span className="chat-tool-out">{lines.length ? t.chat.toolFoundLines(lines.length) : t.chat.toolNoOutput}</span>;
  if (lines.length === 0) return <span className="chat-tool-out">{t.chat.toolNoOutput}</span>;
  const head = dedent(lines.slice(0, OUTPUT_HEAD_LINES));
  return (
    <span className="chat-tool-out is-output">
      {head.join("\n")}
      {lines.length > head.length && <span className="chat-tool-out-more">{`\n… ${t.chat.toolMoreLines(lines.length - head.length)}`}</span>}
    </span>
  );
});
