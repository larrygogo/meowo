/// 往托管终端写「特殊输入」的两段固定流程：中断键、原生贴图。
///
/// 为什么单独成模块：两者都是纯粹的 I/O 时序（写什么、等什么、何时算成功），与对话窗的
/// 会话/门控状态无关，却一直写在 ChatWindow 的发送路径里，只能渲染整个窗口来测。这里把
/// I/O 以参数注入，时序规则（按几次、隔多久、等哪个占位符）可以直接单测。
/// 发送守卫、草稿暂存、提交正文仍归调用方——它们依赖会话身份与交互门控。

import { useEffect, useState } from "react";
import type { ManagedTerminalSnapshot } from "../../api";
import { appendTerminalText, visibleTerminalText } from "../../terminalAttention";

const sleep = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

/// 按插件声明的次数**分次**写中断键:opencode 首按 Esc 只提示 "esc again to interrupt",
/// 两个 Esc 合成一次写入又会被当成 Alt+Esc(真机取证见其插件)。
export async function writeInterruptKeys(write: (data: string) => Promise<void>, input: string, presses: number): Promise<void> {
  const count = Math.max(1, presses);
  for (let press = 0; press < count; press += 1) {
    if (press > 0) await sleep(150);
    await write(input);
  }
}

/// 只发中断键、停掉当前回合（composer 上的停止键与排队栈的「中断并处理」）。
///
/// interrupting:写完中断键后 tone 要等下一轮历史轮询才变,期间按钮若维持可点形态,
/// 用户以为没点上会连点——多个中断键连打进 PTY(claude 的双 Esc 另有历史回退语义)。
/// 置忙态直到 tone 离开 running 或超时兜底,期间按钮禁用并转圈。
export function useInterrupt({ sessionId, running, write, input, presses, blocked, onError }: {
  sessionId: number;
  running: boolean;
  write: (data: string) => Promise<void>;
  /** 插件声明的中断键；null = 未取证，不提供中断。 */
  input: string | null | undefined;
  presses: number;
  /** 此刻不该中断（发送/中断在途、交互提示在屏）。 */
  blocked: () => boolean;
  onError: (error: unknown) => void;
}) {
  const [interrupting, setInterrupting] = useState(false);
  const interrupt = () => {
    if (!input || interrupting || blocked()) return;
    setInterrupting(true);
    void writeInterruptKeys(write, input, presses).catch((error) => {
      onError(error);
      setInterrupting(false);
    });
  };
  useEffect(() => {
    if (!interrupting) return;
    if (!running) { setInterrupting(false); return; }
    // 兜底:中断被 TUI 吞掉(如恰逢重绘)时 4s 后解锁,允许再按。
    const timer = window.setTimeout(() => setInterrupting(false), 4_000);
    return () => window.clearTimeout(timer);
  }, [interrupting, running]);
  // 换会话清忙态:旧会话的中断在途与新会话无关。
  useEffect(() => { setInterrupting(false); }, [sessionId]);
  return { interrupting, interrupt };
}

export type NativePasteIo = {
  setClipboardImage: (path: string) => Promise<void>;
  write: (data: string) => Promise<void>;
  snapshot: (since?: number) => Promise<ManagedTerminalSnapshot>;
};

/// 原生图片附加：逐张把图片写进系统剪贴板 → 发插件声明的粘贴键让 TUI 自己读 → 等第 N 个
/// 占位符（claude 的 `[Image #N]`、kimi 的 `[image #N (W×H)]`）出现再贴下一张。
///
/// 返回 "landed" = 全部落地，调用方接着提交正文；"fallback" = 没贴成但可以安全改走文本路径
/// （首张还没粘贴，或贴了一半的残图已由 abandon 确认清干净）。清不掉的残图抛 unconfirmed——
/// 带着残图降级会重复附图。abort 返回非空即中止（回车前交互提示恰好弹出、会话已切走）。
export async function pasteImagesNatively({ images, marker, pasteInput, io, abort, abandon, onPasteStarted, landingPolls, unconfirmed }: {
  images: readonly string[];
  /** 占位符的匹配正则（插件声明）。 */
  marker: string;
  pasteInput: string;
  io: NativePasteIo;
  abort: () => string | null;
  /** 放弃原生贴图：清掉输入框里贴了一半的残图，清干净返回 true。 */
  abandon: () => Promise<boolean>;
  /** 第一次真的向终端发粘贴键：从此刻起输入框里可能留有残图。 */
  onPasteStarted: () => void;
  /** 每张图等占位符落出的轮询次数（每次 250ms）。 */
  landingPolls: number;
  unconfirmed: string;
}): Promise<"landed" | "fallback"> {
  const before = await io.snapshot();
  const markerPattern = new RegExp(marker, "gi");
  const decoder = new TextDecoder();
  let tail = "";
  let since = before.endOffset;
  let pasteStarted = false;
  const check = () => { const cancelled = abort(); if (cancelled) throw new Error(cancelled); };
  for (let index = 0; index < images.length; index += 1) {
    check();
    try { await io.setClipboardImage(images[index]); }
    catch {
      // 只有尚未向 PTY 粘贴任何图片时才可自动降级，否则第一张会随文本路径重复发送。
      if (!pasteStarted || await abandon()) return "fallback";
      throw new Error(unconfirmed);
    }
    check();
    if (!pasteStarted) { pasteStarted = true; onPasteStarted(); }
    await io.write(pasteInput);
    let landed = false;
    for (let attempt = 0; attempt < landingPolls && !landed; attempt += 1) {
      await sleep(250);
      check();
      const snapshot = await io.snapshot(since);
      if (!snapshot.active || snapshot.endOffset < since) break;
      since = snapshot.endOffset;
      tail = appendTerminalText(tail, snapshot.data, decoder);
      landed = (visibleTerminalText(tail).match(markerPattern) ?? []).length >= index + 1;
    }
    if (!landed) {
      if (await abandon()) return "fallback";
      throw new Error(unconfirmed);
    }
  }
  return "landed";
}
