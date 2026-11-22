// 方案卡（offer_choices）替用户发出的那条消息：怎么写、怎么认。
//
// 卡片点选后以用户身份发一条普通消息给 agent（ChatActionsContext.sendText），不走额外通道：
// 五家都能读懂一句人话，回放旧会话时也能从这条消息认出当时选了哪个，卡片据此显示「已选」。
// 写法中英各一套（按界面语言发），认的时候两套都认：切过语言的旧会话照样认得出。

export type ChoiceAnswer =
  | { kind: "picked"; index: number; note: string | null }
  | { kind: "none"; note: string | null };

const PICKED = [/^选了方案 (\d+)[：:]/, /^Picked option (\d+):/];
const NONE = [/^方案都不要/, /^None of these options/];
const NOTE = [/\n补充[：:]\s*([\s\S]*)$/, /\nNote:\s*([\s\S]*)$/];

export function formatChoiceAnswer(answer: ChoiceAnswer, labels: readonly string[], locale: string): string {
  const zh = !locale.startsWith("en");
  const head = answer.kind === "picked"
    ? (zh ? `选了方案 ${answer.index + 1}：${labels[answer.index] ?? ""}` : `Picked option ${answer.index + 1}: ${labels[answer.index] ?? ""}`)
    : (zh ? "方案都不要" : "None of these options");
  const note = answer.note?.trim();
  return note ? `${head}\n${zh ? "补充：" : "Note: "}${note}` : head;
}

/** 认出方案卡发出的消息；不是就返回 null。index 从 0 起；超出选项个数的不算。 */
export function parseChoiceAnswer(text: string, count: number): ChoiceAnswer | null {
  const trimmed = text.trim();
  const note = NOTE.map((re) => re.exec(trimmed)?.[1]?.trim()).find((value) => value) ?? null;
  for (const re of PICKED) {
    const match = re.exec(trimmed);
    if (!match) continue;
    const index = Number(match[1]) - 1;
    return index >= 0 && index < count ? { kind: "picked", index, note } : null;
  }
  return NONE.some((re) => re.test(trimmed)) ? { kind: "none", note } : null;
}
