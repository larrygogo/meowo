import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type { ChatItem } from "../api";

/// 五家回放样本的读取（测试用）。样本是后端回放测试产出的快照
/// （`src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/<家>.<会话>.items.json`）：每份取自同一场
/// 真实会话、保持原顺序，一家有若干份（见 `meowo-agent/src/replay_tests.rs`）。
///
/// 此前每家一份、由几场会话拼成，前端测试把它当一条时间线渲染，耗时算出来是几天（做空 ⑨）。
/// 现在每份各自渲染；只有「这一家有没有某类条目」这种不看顺序的断言才把各份摊平。

const FIXTURES = join(__dirname, "../../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay");

export const REPLAY_AGENTS = ["claude", "kimi", "codex", "gemini", "opencode"] as const;
export type ReplayAgent = typeof REPLAY_AGENTS[number];
export type ReplaySample = { name: string; items: ChatItem[] };

/** 一份样本（名字如 `codex.01a1cea4`）解析出的原始条目。 */
export function replaySample(name: string): ChatItem[] {
  return JSON.parse(readFileSync(join(FIXTURES, `${name}.items.json`), "utf8")) as ChatItem[];
}

/** 一家的全部样本，按名字排序。 */
export function replaySamples(agent: ReplayAgent): ReplaySample[] {
  const suffix = ".items.json";
  return readdirSync(FIXTURES)
    .filter((file) => file.startsWith(`${agent}.`) && file.endsWith(suffix))
    .sort()
    .map((file) => {
      const name = file.slice(0, -suffix.length);
      return { name, items: replaySample(name) };
    });
}

/** 这一家里第一份含有满足条件的条目的样本，以及那条的位置。 */
export function findInSamples(agent: ReplayAgent, pick: (item: ChatItem, index: number, items: ChatItem[]) => boolean): { name: string; items: ChatItem[]; at: number } {
  for (const sample of replaySamples(agent)) {
    const at = sample.items.findIndex(pick);
    if (at >= 0) return { ...sample, at };
  }
  throw new Error(`${agent} 的样本里没有要找的条目`);
}
