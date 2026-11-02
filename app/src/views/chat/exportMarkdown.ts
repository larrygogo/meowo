/// 把对话导出成 Markdown（对照 Kimi 会话菜单的「复制全部对话为 Markdown / 仅复制最终总结」）。
///
/// 只留「人说的话」与「agent 的正文回复」：工具调用、思考过程、终端回显、后台任务通知都不进——
/// 贴进文档 / Issue 时那些是噪音，要看过程回 Meowo 里看。用户消息的取舍与右侧消息目录同口径
/// （userNavEntries）：只剩免责声明的本地命令、后台任务通知、跨会话消息不是用户写的，跳过。
import type { ChatItem } from "../../api";
import { splitUserText } from "./Message";
import { parseUserText } from "./localCommand";

/** 用户消息的正文：剥掉本地命令外壳与粘贴图片的占位符；不是用户亲手写的返回 null。 */
function userBody(raw: string): string | null {
  const parts = parseUserText(raw);
  if (parts.local && !parts.text.trim()) {
    // 纯本地命令（/model 之类）：命令本身是用户敲的，留一行；通知 / 跨会话消息不算。
    const command = parts.commands[0];
    if (!command) return null;
    const name = command.name.startsWith("/") ? command.name : `/${command.name}`;
    return command.args ? `${name} ${command.args}` : name;
  }
  const { body } = splitUserText(parts.text);
  const text = body.replace(/\[Image #\d+\]/g, "").trim();
  return text || null;
}

export function transcriptMarkdown(
  items: readonly ChatItem[],
  labels: { title: string; user: string; agent: string },
): string {
  const blocks: string[] = [`# ${labels.title}`];
  for (const item of items) {
    if (item.type === "user_text") {
      const body = userBody(item.text);
      if (body) blocks.push(`**${labels.user}**\n\n${body}`);
    } else if (item.type === "assistant_text") {
      const text = item.text.trim();
      if (text) blocks.push(`**${labels.agent}**\n\n${text}`);
    }
  }
  return `${blocks.join("\n\n---\n\n")}\n`;
}

/** 最后一条 agent 正文回复（「仅复制最终总结」）；还没有回复时 null。 */
export function finalAnswer(items: readonly ChatItem[]): string | null {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (item.type === "assistant_text" && item.text.trim()) return item.text.trim();
  }
  return null;
}
