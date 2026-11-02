/// 会话菜单的导出三件事（从 ChatWindow.tsx 拆出，行为不变；对照 Kimi 会话菜单的「复制全部对话为
/// Markdown / 仅复制最终总结 / 导出会话」）。内容规则见 exportMarkdown。
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { agentName, exportTextFile, type AgentDescriptor, type ChatItem } from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { copyPlainText } from "./clipboard";
import { finalAnswer, transcriptMarkdown } from "./exportMarkdown";

export function useConversationExport({ items, title, provider, agents, onNotice, onError }: {
  /** 已渲染的时间线（含接续的前序段）。 */
  items: readonly ChatItem[];
  title: string | null | undefined;
  provider: string | null | undefined;
  agents: readonly AgentDescriptor[] | null;
  onNotice: (text: string) => void;
  onError: (message: string) => void;
}) {
  const t = useT();
  const markdown = () => transcriptMarkdown(items, {
    title: title || t.chat.title,
    user: t.chat.exportYou,
    agent: provider ? agentName([...(agents ?? [])], provider) : t.chat.title,
  });
  const copyConversation = () => {
    void copyPlainText(markdown()).then((ok) => onNotice(ok ? t.chat.conversationCopied : t.chat.copyFailed));
  };
  const copyFinalAnswer = () => {
    const text = finalAnswer(items);
    if (!text) return;
    void copyPlainText(text).then((ok) => onNotice(ok ? t.chat.finalAnswerCopied : t.chat.copyFailed));
  };
  // 导出会话为 .md：系统保存框选路径，文件名默认取会话标题（去掉文件名里不能用的字符）。
  // 取消保存框不报错。
  const exportConversation = async () => {
    const base = (title || t.chat.title).replace(/[\\/:*?"<>|]+/g, " ").trim().slice(0, 80) || "conversation";
    const path = await saveDialog({ defaultPath: `${base}.md`, filters: [{ name: "Markdown", extensions: ["md"] }] }).catch(() => null);
    if (!path) return;
    try {
      await exportTextFile(path, markdown());
      onNotice(t.chat.conversationExported);
    } catch (error) {
      onError(formatBackendError(error, t.locale));
    }
  };
  return {
    /** 有内容可复制 / 导出（菜单项据此显隐）。 */
    hasContent: items.length > 0,
    hasFinalAnswer: finalAnswer(items) !== null,
    copyConversation,
    copyFinalAnswer,
    exportConversation,
  };
}
