// 网络图片的「点了才加载」：正文 Markdown、README 预览、Lead 与计划面板、更新说明里的 http(s) 图片不自动下载。
// 先显示一个占位（写出域名和「点击加载图片」），用户点了才交给后端 resolve_chat_image 去下。
// 同一会话里某个域名点过一次，这个域名后面的图（包括已经摆在屏幕上的占位）自动加载。
//
// 为什么不自动下：图片网址是 agent 或仓库作者写的，一打开就下载等于替第三方确认「这个人正在看」（追踪像素），
// 也可能拿内网或大文件试探。邮件客户端（Gmail、Thunderbird、Apple Mail）的做法是一样的：外部图片默认挡住，
// 用户点「显示图片」或把发件人加进信任后才下。这里的「发件人」换成域名，范围限在当前会话。
//
// 放行名单只存在内存里，不持久：重开 Meowo 之后重新要点，免得一次点击变成永久授权（约定见任务目录
// research/contract.md「2026-11-21 做空第一轮后的修订」）。本地路径不受影响，照常自动显示。
import { createContext, useCallback, useContext, useSyncExternalStore } from "react";

/// 当前会话的 id：网络图片的放行按会话分开记；手机页解析本地路径时也要带上它（后端只在该会话的工作目录内解析）。
/// 不在任何会话里的地方（更新说明窗）是 null，这些地方共用一份放行名单。
export const ChatSessionContext = createContext<number | null>(null);

const allowed = new Map<string, Set<string>>();
const listeners = new Set<() => void>();
let version = 0;

function scopeKey(session: number | null): string {
  return session == null ? "global" : `s:${session}`;
}

/** http(s) 网址的域名（小写）；不是网址（本地路径、相对路径）返回 null。 */
export function remoteImageHost(src: string): string | null {
  if (!/^https?:\/\//i.test(src.trim())) return null;
  try {
    return new URL(src.trim()).hostname.toLowerCase() || null;
  } catch {
    return null;
  }
}

export function isImageHostAllowed(session: number | null, host: string): boolean {
  return allowed.get(scopeKey(session))?.has(host) ?? false;
}

export function allowImageHost(session: number | null, host: string): void {
  const key = scopeKey(session);
  const set = allowed.get(key) ?? new Set<string>();
  if (set.has(host)) return;
  set.add(host);
  allowed.set(key, set);
  version += 1;
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/// 这张图现在能不能去拿：本地路径恒为 true；网址看当前会话是否放行过它的域名。
/// 返回 [能否加载, 域名, 放行函数]。放行后同会话同域名的其他占位一起刷新。
export function useRemoteImageGate(src: string | undefined | null): { ready: boolean; host: string | null; allow: () => void } {
  const session = useContext(ChatSessionContext);
  const host = src ? remoteImageHost(src) : null;
  useSyncExternalStore(subscribe, () => version, () => version);
  const allow = useCallback(() => { if (host) allowImageHost(session, host); }, [session, host]);
  return { ready: host == null || isImageHostAllowed(session, host), host, allow };
}

/** 测试用：清空放行名单。 */
export function resetImageHostGate(): void {
  allowed.clear();
  version += 1;
  listeners.forEach((listener) => listener());
}
