/// 提问表单的第二识别源：读后端按 PTY 实际尺寸维护的屏幕（managedTerminalScreen），
/// 认 AskUserQuestion 的终端表单。
///
/// 为什么需要（提问卡「答不了、关不掉」，调研 real-usage #3，11-02、11-03 实拍）：题面卡
/// 只是展示形态时（broker 放行了挂起：老版本 300s 超时、窗口没及时就位、远程租约过期、
/// 「去终端作答」），点选的答案要等屏幕识别确认表单在屏才落键。原先唯一的识别源是对话窗里
/// xterm 的可视区，它会和 CLI 的真实画面对不上：
/// - 网格漂移：xterm 的行列和 PTY 不一致时，TUI 按 PTY 尺寸重绘，xterm 里同一块区域重画成
///   好几份，最底下的表单被挤出可视区（ManagedTerminal resyncGridIfDrifted 的注释原文；
///   11-03 10:15 截图里终端正是这种错位，表单 10 分钟没被认出，卡一直写「表单就绪后自动作答」）。
/// - 可视区不在最底部、终端还没挂载（手机上没有终端页）时同样看不见表单。
/// 后端的屏幕由同一条字节流按 PTY 尺寸重建，不受这些影响。识别规则仍是同一个
/// terminalAttention（编号选择器），这里只换输入源。
///
/// 只在「展示形态的题面卡在等表单」期间轮询；认出后继续盯着，跟进下一题与焦点变化，
/// 表单从屏上消失（连续几次认不出）时撤回自己报过的识别结果。

import { useEffect, useRef, useState } from "react";
import { managedTerminalScreen } from "../../api";
import { terminalAttention, type AttentionGrammar, type TerminalAttention } from "../../terminalAttention";

export const QUESTION_PROBE_MS = 800;
/// 题面卡出现多久还没认出表单，就照实说「没认出」并给出路（与 ChatWindow 的诊断日志同一个 8s）。
export const QUESTION_FORM_MISSING_MS = 8_000;
const CLEAR_STREAK = 3;

export type QuestionFormProbe = {
  /// 等够 QUESTION_FORM_MISSING_MS 仍没在后端屏幕上认出表单（卡上要说明原因、给出路）。
  missing: boolean;
};

export function useQuestionFormProbe({ sessionId, active, othersSeeForm, grammar, onAttention }: {
  sessionId: number;
  /// 展示形态的题面卡在场且会话由 Meowo 托管（写得进按键）。
  active: boolean;
  /// xterm 那条识别源已经认出了表单。它由输出事件驱动、总在 CLI 重画之后扫，焦点最新；
  /// 这时本探针让位，不拿轮询读到的、可能早于重画的一帧去覆盖它（多选连点时焦点会被拨回）。
  othersSeeForm: boolean;
  grammar: () => AttentionGrammar;
  onAttention: (attention: TerminalAttention | null) => void;
}): QuestionFormProbe {
  const [missing, setMissing] = useState(false);
  const onAttentionRef = useRef(onAttention);
  onAttentionRef.current = onAttention;
  const grammarRef = useRef(grammar);
  grammarRef.current = grammar;
  const othersRef = useRef(othersSeeForm);
  othersRef.current = othersSeeForm;
  useEffect(() => {
    setMissing(false);
    if (!active || sessionId === 0) return;
    let cancelled = false;
    let timer = 0;
    let reported: string | null = null;
    let misses = 0;
    let seen = false;
    const startedAt = Date.now();
    const tick = async () => {
      const screen = await managedTerminalScreen(sessionId).catch(() => null);
      if (cancelled) return;
      const text = (screen ?? []).join("\n");
      // 只认提问表单：其它提示（审批框、信任页）各有自己的通道，这里不越俎代庖。
      // 按种类认，不按规则 id：claude 的编号选择器、gemini 的框线表单、kimi 的方括号表单都是提问。
      // 此前只认编号选择器的 id，kimi 的表单被丢掉，手机上（没有 xterm，这里是唯一的识别源）一直认不出。
      const attention = text ? terminalAttention(text, [], true, false, grammarRef.current(), false) : null;
      if (attention?.kind === "question") {
        seen = true;
        misses = 0;
        setMissing(false);
        const signature = `${attention.text}\0${JSON.stringify(attention.options)}`;
        // 别的识别源先认出来的，交给它跟进；本探针报过的才由本探针继续更新。
        if (signature !== reported && (reported != null || !othersRef.current)) {
          reported = signature;
          onAttentionRef.current(attention);
        }
      } else {
        if (reported != null && ++misses >= CLEAR_STREAK) {
          reported = null;
          misses = 0;
          onAttentionRef.current(null);
        }
        if (!seen && Date.now() - startedAt >= QUESTION_FORM_MISSING_MS) setMissing(true);
      }
      timer = window.setTimeout(() => void tick(), QUESTION_PROBE_MS);
    };
    void tick();
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [active, sessionId]);
  return { missing };
}
