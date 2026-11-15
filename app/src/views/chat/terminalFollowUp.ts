/// 屏幕审批的二次确认页：用户在 Meowo 的卡片上点了持久放行，CLI 却还要再确认一次时，
/// Meowo 认出确认页后替用户按下确认，对话页上一次点击就完成。
///
/// 起因（opencode 1.19.5 真机取证，2026-11-14）：权限框里选 Allow always 不直接生效，先进
/// 「△ Always allow」确认页（`Confirm   Cancel`）。五家的持久放行在对话页都应该是点一下。
/// 不盲按：只有屏幕上真的认出了那条规则声明的确认页（规则的 `confirms` 指向刚才那张卡），
/// 而且是同一个会话、离点击不超过时限，才替用户按。用户自己在终端里走到确认页的，照常出卡让他选。

import type { TerminalAttention } from "../../terminalAttention";

/// 用户在哪个会话、点了哪张卡上的持久放行、什么时候点的。
export type PendingFollowUp = { session: number; rule: string; at: number };

/// 点击之后多久内出现的确认页算是这次点击带出来的。opencode 实测一秒内出现，留足余量。
export const FOLLOW_UP_WINDOW_MS = 8_000;

/// 认出的这一屏是不是刚才那次点击带出来的确认页；是就返回确认要写的按键，否则 null。
/// 确认项恒为确认页的第一项（焦点初始就在它上面）。
export function followUpInput(
  pending: PendingFollowUp | null,
  attention: Pick<TerminalAttention, "confirms" | "options">,
  session: number,
  now: number,
): string | null {
  if (!pending || pending.session !== session) return null;
  if (!attention.confirms || attention.confirms !== pending.rule) return null;
  if (now - pending.at > FOLLOW_UP_WINDOW_MS || now < pending.at) return null;
  return attention.options?.[0]?.input || null;
}
