// 对话页的「最近看过的会话」快照缓存（对照 Kimi 的会话池：切回看过的会话直接从内存出画面，
// 再在后台补增量）。此前每次换会话都清空重读整段 transcript——手机上「列表 → 会话 → 返回
// → 再进同一条」次次骨架屏，走 Tailscale 的长会话要等好几秒。
//
// 快照只是「上次看到的样子」，不是真值：恢复后照常从快照的 offset 拉增量，文件被重建时
// 后端回 reset 整读纠正。模块级 Map 即可——每个窗口是独立的 JS 上下文。
import type { ChatHistory, ChatItem } from "../../api";

export type HistorySnapshot = {
  history: ChatHistory;
  items: ChatItem[];
  /** transcript 已读到的字节偏移（增量轮询从这里续）。 */
  offset: number;
  /** 已加载窗口的首行偏移（「加载更早」从这里往上翻）。 */
  earliest: number;
  hasEarlier: boolean;
};

/** 只留最近这么多条：长时间挂着来回切几十个会话，内存不该跟着无限涨。 */
const LIMIT = 8;
const cache = new Map<number, HistorySnapshot>();

export function rememberHistory(sessionId: number, snapshot: HistorySnapshot): void {
  if (sessionId <= 0) return;
  // 删了再插：Map 按插入序迭代，最近写入的排在最后，淘汰从最前面开始。
  cache.delete(sessionId);
  cache.set(sessionId, snapshot);
  while (cache.size > LIMIT) {
    const oldest = cache.keys().next().value;
    if (oldest === undefined) break;
    cache.delete(oldest);
  }
}

export function recallHistory(sessionId: number): HistorySnapshot | undefined {
  return cache.get(sessionId);
}

export function forgetHistory(sessionId: number): void {
  cache.delete(sessionId);
}

// 预取（对照 Kimi 的 prefetchMainTranscript）：手指按下/鼠标悬停在会话列表项上就开读首屏。
// 在途请求按会话记下：点下去时首读直接接过这一发（takePrefetch），不重复读一遍。
const inflight = new Map<number, Promise<ChatHistory>>();

export function trackPrefetch(sessionId: number, request: Promise<ChatHistory>): void {
  inflight.set(sessionId, request);
  const done = () => { if (inflight.get(sessionId) === request) inflight.delete(sessionId); };
  request.then(done, done);
}

export function prefetchPending(sessionId: number): boolean {
  return inflight.has(sessionId);
}

/** 取走在途的首读（只取一次：之后的轮询各走各的增量）。 */
export function takePrefetch(sessionId: number): Promise<ChatHistory> | undefined {
  const request = inflight.get(sessionId);
  inflight.delete(sessionId);
  return request;
}

// 会话列表已知的标题/agent/目录：点进会话、首读还没回来时，顶栏先用它出真实标题
// （对照 Kimi：点开即见标题，不是一个通用的「会话对话」占位等几秒再换）。
export type SessionMeta = { title: string; provider: string; cwd: string | null };
const metas = new Map<number, SessionMeta>();

export function noteSessionMeta(sessionId: number, meta: SessionMeta): void {
  metas.set(sessionId, meta);
}

export function peekSessionMeta(sessionId: number): SessionMeta | undefined {
  return metas.get(sessionId);
}

/** 测试用：用例之间清空。 */
export function clearHistoryCache(): void {
  cache.clear();
  inflight.clear();
  metas.clear();
}
