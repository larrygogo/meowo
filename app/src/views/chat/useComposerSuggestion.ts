// 建议回复（调研 landscape.md 第 4 项）：Claude 回合结束后会在终端输入框里用暗色字画一句「预测的下一句」
// （prompt suggestions，交互模式默认开）。对话页看不到终端，这句话就白生成了。这里从终端画面把它读出来，
// 交给输入框上方的一颗按钮：点了只填进输入框，用户看过再发（误发是这类功能最大的坑，见调研）。
//
// 不额外调模型：建议是 CLI 自己生成的，Meowo 只读屏（统一用同一个 provider 的规矩）。
import { useEffect, useRef, useState } from "react";
import { composerSuggestion, readComposerFrame, terminalComposerIo, type ComposerIo, type ComposerSpec } from "./composer";

/// 回合结束后多久内去读：实拍约 5~10 秒出现（2.1.330），短对话或缓存冷时可能一直没有。
export const SUGGESTION_POLL_MS = 1500;
export const SUGGESTION_POLL_TRIES = 30;

/// 主回合停下（不在跑）且这一家会画建议时，隔一会儿读一次终端输入框，读到就停。
/// 回合又跑起来、换会话、这一家不画建议时清掉。
export function useComposerSuggestion(
  sessionId: number,
  spec: ComposerSpec | null | undefined,
  idle: boolean,
  io?: ComposerIo,
): string | null {
  const [suggestion, setSuggestion] = useState<string | null>(null);
  const enabled = idle && sessionId !== 0 && !!spec?.suggestion_placeholder;
  // 规格对象可能随会话信息刷新换引用：经 ref 取，换引用不该把已读到的建议清掉、重新读一遍。
  const specRef = useRef(spec);
  specRef.current = spec;
  useEffect(() => {
    setSuggestion(null);
    const spec = specRef.current;
    if (!enabled || !spec) return;
    const terminal = io ?? terminalComposerIo(sessionId);
    let cancelled = false;
    let tries = 0;
    let timer: number | undefined;
    const tick = async () => {
      const frame = await terminal.frame().catch(() => null);
      if (cancelled) return;
      const found = composerSuggestion(readComposerFrame(frame, spec), spec);
      if (found) {
        setSuggestion(found);
        return;
      }
      if ((tries += 1) < SUGGESTION_POLL_TRIES) timer = window.setTimeout(() => void tick(), SUGGESTION_POLL_MS);
    };
    timer = window.setTimeout(() => void tick(), SUGGESTION_POLL_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [enabled, sessionId, io]);
  return suggestion;
}
