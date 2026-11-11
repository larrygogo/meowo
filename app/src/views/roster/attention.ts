// 眼睛该站到谁旁边、是什么神态。黑轨上的眼睛只认这一个函数（规格 .lead/20261111-roster）。
// 状态本身不在这里判：每个会话是哪一档由 cardTone 给（它源于 activityTone，全应用一处）。
import { cardTone } from "../sticker/helpers";
import type { Item } from "../sticker/types";
import type { IslandTone } from "../island/islandParts";

export type Attention = {
  /** 眼睛站在哪个会话那一行；null = 没人需要你，回轨顶。 */
  id: number | null;
  /** 眼睛的神态。站在某一行旁边时说的就是那一行，不会出现「站在这行、神态说的是那行」。 */
  tone: IslandTone;
};

/** 需要你的三档，数字越小越要紧：待处理 > 出错 > 跑完了你还没看。其余不叫眼睛。
 *  待处理压过出错与 cardTone 同序：出错只是上一轮断了，审批挂着时 agent 正卡着等你。 */
function rank(item: Item): number {
  const tone = cardTone(item);
  if (tone === "pending") return 0;
  if (tone === "error") return 1;
  if (tone === "waiting" && item.unread) return 2;
  return 9;
}

/**
 * items 按名单顺序传入。stickyId 是眼睛此刻站着的会话：它还需要你，就站住不动，同样要紧的
 * 新来的不会把眼睛抢走（leader 2026-11-11 确认的取舍 2）；它不需要你了，才按要紧程度、再按
 * 名单从上到下找下一个。
 *
 * 站住不跨档：比它更要紧的来了要让。出错不会因为看过就消失，不让的话一个没处理的出错会把
 * 眼睛钉死，之后所有审批都没人指（做空审查 M1）。
 */
export function attentionFocus(items: readonly Item[], stickyId: number | null): Attention {
  let best: Item | null = null;
  let bestRank = 9;
  for (const item of items) {
    const r = rank(item);
    if (r === 9) continue;
    // 同档里站着的那个优先：r <= bestRank 让它压过排在前面的同档会话。
    if (r < bestRank || (r === bestRank && item.session.id === stickyId)) { best = item; bestRank = r; }
  }
  if (best) {
    return { id: best.session.id, tone: bestRank === 0 ? "pending" : bestRank === 1 ? "error" : "waiting" };
  }
  // 没人需要你：有人在跑就是「在跑」的神态，否则发呆。跑完且看过的不算事。
  const running = items.some((item) => cardTone(item) === "running");
  return { id: null, tone: running ? "running" : "idle" };
}
