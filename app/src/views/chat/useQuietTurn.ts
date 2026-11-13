import { useEffect, useState } from "react";

/** 运行中回合多久没有新内容算「安静」。 */
export const QUIET_TURN_MS = 45_000;

/**
 * 托管回合运行中、却长时间没有任何新内容时返回 true。
 *
 * 为什么要有：Gemini 0.65/0.68 遇到服务端 503（high demand）会 retryWithBackoff 静默重试，
 * ACP 期间不发任何事件（2026-11-13 本机 gemini -p 实测原文），界面看起来像 Meowo 卡死。
 * 只提示、不自动中断：重试可能成功，停不停交给用户（已有停止按钮）。
 *
 * activity 是「最近内容」的指纹（条数、末条 id 等），变了就重新计时。
 */
export function useQuietTurn(running: boolean, activity: string, quietMs = QUIET_TURN_MS): boolean {
  const [quiet, setQuiet] = useState(false);
  useEffect(() => {
    setQuiet(false);
    if (!running) return;
    const timer = window.setTimeout(() => setQuiet(true), quietMs);
    return () => window.clearTimeout(timer);
  }, [running, activity, quietMs]);
  return running && quiet;
}
