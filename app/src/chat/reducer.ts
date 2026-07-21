import type { ChatItem } from "../api";

/**
 * 把 Provider 无关的 transcript 事件折叠成可直接渲染的消息序列。
 *
 * 这里是 delta 合并与边界去重的唯一位置。React 组件只渲染结果；各 Provider parser 只负责
 * 准确描述日志事实。没有实际变化时返回旧引用，让稳定轮询跳过整棵消息树重渲染。
 */
export function reduceChatEvents(
  previous: ChatItem[],
  incoming: ChatItem[],
  reset: boolean,
): ChatItem[] {
  let next = reset ? [] : previous;
  let changed = reset;
  const writable = () => {
    if (!changed) {
      next = [...previous];
      changed = true;
    }
    return next;
  };

  for (const item of incoming) {
    const last = next[next.length - 1];
    if (item.type !== "assistant_delta" && item.type !== "reasoning_delta") {
      // 同一语义事件可能被 Provider 同时写进两个兼容日志入口；只消除相邻且完全等价的记录，
      // 不跨工具活动或其它消息猜测重复，避免吞掉用户确实连续发送的相同文本。
      if (item.type === "user_text" && last?.type === "user_text" && last.text === item.text) continue;
      if (item.type === "reasoning" && last?.type === "reasoning" && last.text === item.text) continue;
      writable().push(item);
      continue;
    }

    const target = item.type === "assistant_delta" ? "assistant_text" : "reasoning";
    if (last?.type === target) {
      const items = writable();
      items[items.length - 1] = { ...last, text: last.text + item.text };
    } else {
      writable().push({ type: target, id: item.id, timestamp: item.timestamp, text: item.text });
    }
  }
  return next;
}
