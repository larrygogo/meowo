// 灵动岛「打开」一条会话（提醒上的「打开」、面板里点会话行）的唯一出口。
//
// I-1（走查 2026-11-12）：原来 `invoke("open_chat_window").catch(console.error)` 只打日志，
// 随后无条件收起提醒或面板。后端 window.rs 专门写了「创建/切换失败必须传播回前端：静默
// 失败时用户点了没反应，再点会重复起会话」，前端又把错误吞了，提醒收起后也回不来。
// 现在失败如实返回，由 App 留住提醒/面板并就地显示原因与「重试」。
//
// 原 I-2（轻量模式下改为把外部终端拉到前台、失败时提示「打开设置」）随 2026-11-20 去掉轻量
// 模式删除（leader 决定）：对话窗恒可用，这里只剩打开对话窗一条路。
import { invoke } from "@tauri-apps/api/core";
import { formatBackendError } from "../../i18n/errors";

/** 打开失败：对话窗没开成（可重试）。 */
export type IslandOpenError = { sessionId: number; kind: "failed"; reason: string };

export async function openIslandSession(args: {
  sessionId: number;
  locale: string;
}): Promise<IslandOpenError | null> {
  const { sessionId, locale } = args;
  try {
    await invoke("open_chat_window", { sessionId });
    return null;
  } catch (err) {
    console.error("[island] 打开会话失败：", err);
    return { sessionId, kind: "failed", reason: formatBackendError(err, locale) };
  }
}
