// 灵动岛「打开」一条会话（提醒上的「打开」、面板里点会话行）的唯一出口。
//
// I-1（走查 2026-11-12）：原来 `invoke("open_chat_window").catch(console.error)` 只打日志，
// 随后无条件收起提醒或面板。后端 window.rs 专门写了「创建/切换失败必须传播回前端：静默
// 失败时用户点了没反应，再点会重复起会话」，前端又把错误吞了，提醒收起后也回不来。
// 现在失败如实返回，由 App 留住提醒/面板并就地显示原因与「重试」。
//
// I-2（同日走查）：轻量模式（chat_enabled=false）下后端 open_chat_window_target 会改开设置窗，
// 用户想看那条会话却来到设置页，也不知道为什么。轻量模式下会话跑在外部终端里，改为
// focus_session 把那个终端拉到前台；拉不过去（进程已结束、终端不支持）才说明「轻量模式
// 没有对话窗」，并给「打开设置」。
import { invoke } from "@tauri-apps/api/core";
import { focusSessionTerminal, type FocusSessionResult } from "../../api";
import { formatBackendError } from "../../i18n/errors";
import type { Item } from "../sticker/types";

/** 打开失败的两种结局：failed = 对话窗没开成（可重试）；lite = 轻量模式下跳不到外部终端。 */
export type IslandOpenError =
  | { sessionId: number; kind: "failed"; reason: string }
  | { sessionId: number; kind: "lite" };

/** focus_session 算「跳过去了」的两种结果：精确切到标签，或至少把宿主窗口置前。 */
const FOCUSED: readonly FocusSessionResult[] = ["focused", "host_focused"];

export async function openIslandSession(args: {
  sessionId: number;
  chatEnabled: boolean;
  /** 轻量模式下用来定位外部终端（pid/标题/目录）；找不到这条会话时为 undefined。 */
  item: Item | undefined;
  locale: string;
}): Promise<IslandOpenError | null> {
  const { sessionId, chatEnabled, item, locale } = args;
  if (chatEnabled) {
    try {
      await invoke("open_chat_window", { sessionId });
      return null;
    } catch (err) {
      console.error("[island] 打开会话失败：", err);
      return { sessionId, kind: "failed", reason: formatBackendError(err, locale) };
    }
  }
  if (item?.pid != null && item.pid > 0) {
    const result = await focusSessionTerminal(item).catch((err: unknown) => {
      console.error("[island] 跳到外部终端失败：", err);
      return null;
    });
    if (result && FOCUSED.includes(result)) return null;
  }
  return { sessionId, kind: "lite" };
}
