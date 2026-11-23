// 小表单（ask_form）替用户发出的那条消息：怎么写、怎么认。与方案卡（choiceReply.ts）同一套做法：
// 一条人读得懂的普通消息，五家都能读；回放旧会话时也能从这条消息认出当时填了什么，卡片据此收起。
// 中英各一套（按界面语言发），认的时候两套都认。
//
//   填好了表单「部署配置」
//   - 环境：预发
//   - 备注：
//     第一行
//     第二行
//   - 版本号：（没填）
//
// 认的时候要对上这张卡：标题一致、字段按顺序逐个对上标签；多出来的行（用户手改过）不算。
// 回答里带「- 别的标签：」样式的文字也不会串：多行回答逐行缩进两格，只有顶格的「- 」才是新字段。
import { looksLikeChoiceReply } from "./choiceReply";

export type FormFieldType = "text" | "long_text" | "choice" | "multi_choice" | "yes_no";

export type FormFieldSpec = { label: string; type: FormFieldType };
export type FormCardText = { title: string; fields: readonly FormFieldSpec[] };

/** 一项回答：已经格式化成显示用的文字；null 表示没填。 */
export type FormAnswer =
  | { kind: "filled"; values: (string | null)[] }
  | { kind: "declined"; note: string | null };

const COPY = {
  zh: { filled: (t: string) => `填好了表单「${t}」`, declined: (t: string) => `不填表单「${t}」`, sep: "：", blank: "（没填）", note: "补充：" },
  en: { filled: (t: string) => `Filled in form "${t}"`, declined: (t: string) => `Declined form "${t}"`, sep: ": ", blank: "(left blank)", note: "Note: " },
} as const;
const LOCALES = [COPY.zh, COPY.en];

const FILLED_HEAD = [/^填好了表单「(.*)」$/, /^Filled in form "(.*)"$/];
const DECLINED_HEAD = [/^不填表单「(.*)」$/, /^Declined form "(.*)"$/];

export function formatFormAnswer(answer: FormAnswer, card: FormCardText, locale: string): string {
  const copy = locale.startsWith("en") ? COPY.en : COPY.zh;
  if (answer.kind === "declined") {
    const note = answer.note?.trim();
    return note ? `${copy.declined(card.title)}\n${copy.note}${note}` : copy.declined(card.title);
  }
  const lines = [copy.filled(card.title)];
  card.fields.forEach((field, index) => {
    const value = answer.values[index]?.trim() ?? "";
    if (!value) {
      lines.push(`- ${field.label}${copy.sep}${copy.blank}`);
    } else if (value.includes("\n")) {
      lines.push(`- ${field.label}${copy.sep.trimEnd()}`);
      for (const line of value.split(/\r?\n/)) lines.push(`  ${line}`);
    } else {
      lines.push(`- ${field.label}${copy.sep}${value}`);
    }
  });
  return lines.join("\n");
}

/** 认出这张卡发出的消息；不是（或是别的卡的、被用户改过的）返回 null。 */
export function parseFormAnswer(text: string, card: FormCardText): FormAnswer | null {
  const lines = text.trim().split(/\r?\n/);
  const head = lines[0]?.trim() ?? "";
  for (const re of DECLINED_HEAD) {
    const match = re.exec(head);
    if (!match) continue;
    if (match[1] !== card.title) return null;
    const tail = lines.slice(1).join("\n");
    if (!tail.trim()) return { kind: "declined", note: null };
    for (const copy of LOCALES) {
      if (tail.startsWith(copy.note)) return { kind: "declined", note: tail.slice(copy.note.length).trim() || null };
    }
    return null;
  }
  for (const [at, re] of FILLED_HEAD.entries()) {
    const match = re.exec(head);
    if (!match) continue;
    if (match[1] !== card.title) return null;
    const copy = LOCALES[at];
    const values: (string | null)[] = [];
    let row = 1;
    for (const field of card.fields) {
      const line = lines[row] ?? "";
      const prefix = `- ${field.label}${copy.sep}`;
      const bare = `- ${field.label}${copy.sep.trimEnd()}`;
      if (line === bare && (lines[row + 1] ?? "").startsWith("  ")) {
        // 先认多行：中文的分隔符「：」后面没有空格，多行的头一行也以单行的前缀开头。
        row += 1;
        const block: string[] = [];
        // 回答里的空行发出去是两个空格，记录里可能被去掉行尾空白成了空行：只要空行之后（隔几行都行）
        // 还接着缩进行，就照算这一项的（审查 2026-11-23：连续两个空行时曾断开，卡片又变回可填）。
        const continues = (at: number) => {
          let next = at;
          while (next < lines.length && !lines[next].trim()) next += 1;
          return next < lines.length && lines[next].startsWith("  ");
        };
        while (row < lines.length && (lines[row].startsWith("  ") || (!lines[row].trim() && continues(row)))) {
          block.push(lines[row].slice(2));
          row += 1;
        }
        if (block.length === 0) return null;
        values.push(block.join("\n"));
      } else if (line.startsWith(prefix)) {
        const value = line.slice(prefix.length);
        values.push(value === copy.blank ? null : value);
        row += 1;
      } else {
        return null;
      }
    }
    // 后面还有别的内容：用户在发出前改过，或是自己写的长消息，不当成卡片发的。
    return lines.slice(row).some((line) => line.trim()) ? null : { kind: "filled", values };
  }
  return null;
}

/** 看起来是某张表单发的消息（不管是哪张）：同一轮有几张卡时，别的卡的回复不算「用户直接打字回复」。 */
export function looksLikeFormReply(text: string): boolean {
  const lines = text.trim().split(/\r?\n/);
  const head = lines[0]?.trim() ?? "";
  if (DECLINED_HEAD.some((re) => re.test(head))) {
    const tail = lines.slice(1).join("\n");
    return !tail.trim() || LOCALES.some((copy) => tail.startsWith(copy.note));
  }
  if (!FILLED_HEAD.some((re) => re.test(head))) return false;
  // 表单至少一个字段：只有一行表头的是用户手打的，算「直接回复」（审查 2026-11-23：曾让卡片一直悬着）。
  const body = lines.slice(1);
  return body.some((line) => line.startsWith("- ")) && body.every((line) => line.startsWith("- ") || line.startsWith("  ") || !line.trim());
}

/** 卡片之后的用户消息里，像不像某张卡（方案卡或表单）替用户发的：同一轮几张卡的回复互不挡，
 *  只有用户自己打的话才让卡片收成「没在卡上选 / 没在表单里填」。 */
export function looksLikeCardReply(text: string): boolean {
  return looksLikeChoiceReply(text) || looksLikeFormReply(text);
}
