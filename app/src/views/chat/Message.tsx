import { useT } from "../../i18n";
import { type ChatItem } from "../../api";
import { ChatMarkdown } from "../ChatMarkdown";

export function Message({ item }: { item: ChatItem }) {
  const t = useT();
  if (item.type === "user_text" || item.type === "assistant_text" || item.type === "assistant_delta") {
    const user = item.type === "user_text";
    return (
      <article className={"chat-message " + (user ? "is-user" : "is-assistant")}>
        {/* 用户消息保持原文（用户不是在写 markdown，行首 # 变大标题只会失真）；
            模型输出按 markdown 渲染。 */}
        {user
          ? <div className="chat-text">{item.text}</div>
          : <div className="chat-text chat-md"><ChatMarkdown text={item.text} /></div>}
      </article>
    );
  }
  if (item.type === "reasoning" || item.type === "reasoning_delta") {
    return (
      <details className="chat-reasoning" open>
        <summary><span className="chat-timeline-dot" />{t.chat.reasoning}</summary>
        <div className="chat-md"><ChatMarkdown text={item.text} /></div>
      </details>
    );
  }
  if (item.type === "tool_use") {
    return (
      <details className="chat-tool">
        <summary>
          <span className="chat-tool-icon"><svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><path d="M4 17l6-6-6-6M12 19h8" /></svg></span>
          <span className="chat-tool-name">{item.name}</span><span className="chat-tool-summary">{item.summary}</span><span className="chat-tool-chevron">›</span>
        </summary>
        <pre>{item.summary}</pre>
      </details>
    );
  }
  if (item.type === "tool_result") {
    return (
      <details className={"chat-tool chat-result" + (item.is_error ? " is-error" : "")}>
        <summary>
          <span className="chat-tool-icon is-file"><svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><path d="M6 3h8l4 4v14H6zM14 3v5h5" /></svg></span>
          <span className="chat-tool-name">{t.chat.toolResult}</span><span className="chat-tool-summary">{item.text}</span><span className="chat-tool-chevron">›</span>
        </summary>
        <pre>{item.text}</pre>
      </details>
    );
  }
  return <div className="chat-meta"><span />{t.chat.compact}<span /></div>;
}
