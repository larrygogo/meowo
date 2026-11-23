// 建议回复（调研 landscape.md 第 4 项）：Claude 回合结束后会在终端输入框里用暗色字画一句「预测的下一句」
// （prompt suggestions，交互模式默认开）。对话页看不到终端，这句话就白生成了。这里从终端画面把它读出来，
// 交给输入框上方的一颗按钮：点了只填进输入框，用户看过再发（误发是这类功能最大的坑，见调研）。
//
// 不额外调模型：建议是 CLI 自己生成的，Meowo 只读屏（统一用同一个 provider 的规矩）。
import { useCallback, useEffect, useRef, useState } from "react";
import { composerSuggestion, readComposerFrame, terminalComposerIo, type ComposerIo, type ComposerSpec } from "./composer";

/// 回合结束后多久内去读：实拍约 5~10 秒出现（2.1.330），短对话或缓存冷时可能一直没有。
export const SUGGESTION_POLL_MS = 1500;
export const SUGGESTION_POLL_TRIES = 30;
/// 读到以后低频复核：用户在终端页打字、或终端里建议没了，对话页跟着收（独立审查第 2 条）。
export const SUGGESTION_RECHECK_MS = 5000;
export const SUGGESTION_RECHECK_TRIES = 120;

/// 主回合停下（不在跑）且这一家会画建议时，隔一会儿读一次终端输入框。读到后转为低频复核。
/// 回合又跑起来、换会话、这一家不画建议时清掉。`dismiss` 在发送时调用：这一句用过了，
/// 终端那边还没反应过来之前不再拿出来（独立审查第 1 条：刚发出又冒出来，Tab 又填回去）。
export function useComposerSuggestion(
  sessionId: number,
  spec: ComposerSpec | null | undefined,
  idle: boolean,
  io?: ComposerIo,
): [string | null, () => void] {
  const [suggestion, setSuggestion] = useState<string | null>(null);
  const usedRef = useRef<string | null>(null);
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
    let rechecks = 0;
    let shown: string | null = null;
    let missed = false;
    let timer: number | undefined;
    const tick = async () => {
      const frame = await terminal.frame().catch(() => null);
      if (cancelled) return;
      let found = composerSuggestion(readComposerFrame(frame, spec), spec);
      if (found && found === usedRef.current) found = null;
      // 已显示的建议一次没读到先不收：Claude 每次重画先藏光标、画完再显示（2.1.330 原始字节经 vt100
      // 重放核对），恰好读在重画中间会认成草稿。隔 1.5 秒再读一次仍没有才收，免得按钮闪一下。
      if (shown && !found && !missed && usedRef.current !== shown) {
        missed = true;
        timer = window.setTimeout(() => void tick(), SUGGESTION_POLL_MS);
        return;
      }
      missed = false;
      if (found !== shown) {
        shown = found;
        setSuggestion(found);
      }
      if (shown) {
        if ((rechecks += 1) < SUGGESTION_RECHECK_TRIES) timer = window.setTimeout(() => void tick(), SUGGESTION_RECHECK_MS);
      } else if ((tries += 1) < SUGGESTION_POLL_TRIES) {
        timer = window.setTimeout(() => void tick(), SUGGESTION_POLL_MS);
      }
    };
    timer = window.setTimeout(() => void tick(), SUGGESTION_POLL_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [enabled, sessionId, io]);
  // 新一轮跑起来以后，上一句「用过的」作废：下一轮 Claude 可能又猜出同一句，那时该照常显示。
  useEffect(() => {
    if (!idle) usedRef.current = null;
  }, [idle]);
  const dismiss = useCallback(() => {
    setSuggestion((current) => {
      if (current) usedRef.current = current;
      return null;
    });
  }, []);
  return [suggestion, dismiss];
}
