/**
 * 状态词表：会话状态、待你处理的请求、消息送达状态，每个状态只有一个词。
 *
 * 词本身在 statusVocab.json（中英各一份）。三处从它取：
 * - zh.ts / en.ts 把它挂成 `t.vocab`，界面一律经本文件的取词函数或 `t.vocab` 读。
 * - 后端通知标题（src-tauri/src/settings.rs 的 `tr`）编译期读同一份 JSON。
 * - statusVocab.test.ts 扫描字典与渲染代码：词表里的词在别处再写一遍、或出现被收敛掉的
 *   旧说法（出错了、待审批、等待审批、已收到…）即失败。
 *
 * 为什么不放在各个渲染点：同一个状态此前在侧栏、灵动岛、通知、气泡、排队栈各写各的，
 * 于是「出错 / 出错了」「待批准 / 待审批 / 等待审批」「已排队 / 已收到」并存（走查输入 B4、显示 B7）。
 */
import type { SessionTone } from "../api";
import type { Dict } from "./zh";
import vocab from "./statusVocab.json";

export const vocabZh = vocab.zh;
export const vocabEn = vocab.en;
export type StatusVocab = typeof vocab.zh;

/** 会话状态：SessionTone 六态，加上显式终态「已中断」与手机连接用的「在线」。 */
export type SessionStatusKey = keyof StatusVocab["session"];
/** 待你处理的请求种类，加上卡片的两个终态（已过期、已取消）。 */
export type RequestStatusKey = keyof StatusVocab["request"];
/** 消息送达状态，与 outgoingPrompts 的 OutgoingPhase 对应（received 是瞬态，不显示）。 */
export type DeliveryStatusKey = keyof StatusVocab["delivery"];

/** 会话状态词。未知状态返回空串（调用方按「不显示」处理），不把内部标识露给用户。 */
export function sessionStatusLabel(t: Dict, status: SessionTone | SessionStatusKey | string): string {
  return (t.vocab.session as Record<string, string>)[status] ?? "";
}

/** 请求状态词。种类未知（屏幕上停着却说不出是哪一种）按「待处理」说。 */
export function requestStatusLabel(t: Dict, kind: RequestStatusKey | string | null | undefined): string {
  return (kind ? (t.vocab.request as Record<string, string>)[kind] : undefined) ?? t.vocab.request.blocked;
}

/** 送达状态词。没有对应词的瞬态（received）返回空串。 */
export function deliveryStatusLabel(t: Dict, phase: DeliveryStatusKey | string): string {
  return (t.vocab.delivery as Record<string, string>)[phase] ?? "";
}

/** 灵动岛按 tone 查词的表：岛上把「在线」记作 on。 */
export function sessionToneLabels(t: Dict): Record<string, string> {
  return { ...t.vocab.session, on: t.vocab.session.online };
}

/** 侧栏会话行的状态词：有待批准的审批时说「待批准」，其余按会话状态说。 */
export function sessionRowLabel(t: Dict, tone: SessionTone, awaitingApproval: boolean): string {
  return awaitingApproval ? t.vocab.request.approval : sessionStatusLabel(t, tone);
}
