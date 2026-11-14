import { memo, useEffect, useState } from "react";
import { useT } from "../../i18n";
import type { DiffHunk } from "../../generated/contracts/DiffHunk";
import type { ToolDetail } from "../../generated/contracts/ToolDetail";
import { fmtElapsed } from "./cliStatus";
import { dedent } from "./terminalText";
import { lineDelta } from "./ToolActivity";
import { outputLines, type ToolResultItem, type ToolUseItem } from "./shared";
import type { ToolKind } from "./ToolIcon";
import { useToolRunState } from "./toolState";

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
  const state = useToolRunState(result);
  // 只有真在跑才走表：回合已经不在进行的调用不会再有结果，计时只会越走越离谱。
  useSecondTick(state === "running");
  if (state === "running") {
    const startedAt = Date.parse(item.timestamp ?? "");
    const since = Number.isNaN(startedAt) ? mountedAt : startedAt;
    return <span className="chat-tool-out is-running">{t.chat.toolRunningFor(fmtElapsed(Date.now() - since))}</span>;
  }
  // 回合停在审批或提问上：这次调用还没开始跑，说的是会话状态同一个词。
  if (state === "waiting") return <span className="chat-tool-out is-running">{t.chat.status.pending}</span>;
  if (state === "interrupted" || !result) return <span className="chat-tool-out">{t.chat.interrupted}</span>;
  const lines = outputLines(result.text);
  if (result.is_error) {
    // 退出码是「为什么算失败」最直接的事实，放在最前；正文首行可能只是命令的普通输出。
    // 有的 CLI 回执首行本来就是「Exit code N」，不重复一遍。
    if (result.exit_code) {
      const detail = lines.find((line) => !/^exit code\b/i.test(line.trim())) ?? "";
      return <span className="chat-tool-out is-error">{t.chat.toolExitCode(result.exit_code, detail)}</span>;
    }
    return <span className="chat-tool-out is-error">{lines[0] ?? t.chat.toolNoOutput}</span>;
  }
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
  // 行数、条数优先用后端给的真实数字：回执正文会被截断，按正文去数会少报。
  const count = result.count ?? lines.length;
  if (kind === "read") return <span className="chat-tool-out">{t.chat.toolReadLines(count)}</span>;
  if (kind === "search") return <span className="chat-tool-out">{count ? t.chat.toolFoundLines(count) : t.chat.toolNoOutput}</span>;
  if (lines.length === 0) return <span className="chat-tool-out">{t.chat.toolNoOutput}</span>;
  const head = dedent(lines.slice(0, OUTPUT_HEAD_LINES));
  return (
    <span className="chat-tool-out is-output">
      {head.join("\n")}
      {lines.length > head.length && <span className="chat-tool-out-more">{`\n… ${t.chat.toolMoreLines(lines.length - head.length)}`}</span>}
    </span>
  );
});
