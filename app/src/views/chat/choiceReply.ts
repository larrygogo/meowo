// 方案卡（offer_choices）替用户发出的那条消息：怎么写、怎么认。
//
// 卡片点选后以用户身份发一条普通消息给 agent（ChatActionsContext.sendText），不走额外通道：
// 五家都能读懂一句人话，回放旧会话时也能从这条消息认出当时选了哪个，卡片据此显示「已选」。
// 写法中英各一套（按界面语言发），认的时候两套都认：切过语言的旧会话照样认得出。
//
// 认的时候要对上这张卡（做空第一轮）：同一轮里有两张卡时，它们之后的第一条用户消息是同一条；
// 只看「方案 N」会把布局卡的选择记到配色卡上，用户手打的「选了方案 2：其实我想说…」也会被当成点选。
// 所以选了哪个要连方案名一起对上，「都不要」要连题面一起对上。

export type ChoiceAnswer =
  | { kind: "picked"; index: number; note: string | null }
  | { kind: "none"; note: string | null };

export type ChoiceCardText = { question: string; labels: readonly string[] };

const PICKED = [/^选了方案 (\d+)：(.*)$/, /^Picked option (\d+): (.*)$/];
const NONE = [/^方案都不要(?:：(.*))?$/, /^None of these options(?:: (.*))?$/];
const NOTE = [/^补充：([\s\S]*)$/, /^Note: ([\s\S]*)$/];

export function formatChoiceAnswer(answer: ChoiceAnswer, card: ChoiceCardText, locale: string): string {
  const zh = !locale.startsWith("en");
  const question = card.question.trim();
  const head = answer.kind === "picked"
    ? (zh ? `选了方案 ${answer.index + 1}：${card.labels[answer.index] ?? ""}` : `Picked option ${answer.index + 1}: ${card.labels[answer.index] ?? ""}`)
    : zh ? `方案都不要${question ? `：${question}` : ""}` : `None of these options${question ? `: ${question}` : ""}`;
  const note = answer.note?.trim();
  return note ? `${head}\n${zh ? "补充：" : "Note: "}${note}` : head;
}

/** 认出这张卡发出的消息；不是（或是别的卡的）返回 null。index 从 0 起。 */
export function parseChoiceAnswer(text: string, card: ChoiceCardText): ChoiceAnswer | null {
  const [head = "", ...rest] = text.trim().split("\n");
  const tail = rest.join("\n");
  const note = tail ? NOTE.map((re) => re.exec(tail)?.[1]?.trim()).find((value) => value) ?? null : null;
  // 首行之后若有内容却不是「补充」，说明是用户自己写的长消息，不当成卡片发的。
  if (tail.trim() && note === null) return null;
  for (const re of PICKED) {
    const match = re.exec(head.trim());
    if (!match) continue;
    const index = Number(match[1]) - 1;
    return index >= 0 && index < card.labels.length && card.labels[index] === match[2].trim() ? { kind: "picked", index, note } : null;
  }
  for (const re of NONE) {
    const match = re.exec(head.trim());
    if (!match) continue;
    // 题面必须是这张卡的（卡片没有题面时，消息里也不带）。
    return (match[1] ?? "").trim() === card.question.trim() ? { kind: "none", note } : null;
  }
  return null;
}

/** 看起来是某张方案卡发的消息（不管是哪张卡）：同一轮有几张卡时，别的卡的回复不算「用户直接打字回复」。 */
export function looksLikeChoiceReply(text: string): boolean {
  const [head = "", ...rest] = text.trim().split("\n");
  const tail = rest.join("\n");
  if (tail.trim() && !NOTE.some((re) => re.test(tail))) return false;
  return [...PICKED, ...NONE].some((re) => re.test(head.trim()));
}
