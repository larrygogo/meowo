import { memo, useContext, useState } from "react";
import type { ChatItem } from "../../api";
import { useT } from "../../i18n";
import { toolKind } from "./ToolIcon";
import { ChatCwdContext, editOutcome, relativize } from "./ToolActivity";
import type { ToolResultItem, ToolUseItem } from "./shared";
import { Icon } from "../../icons/Icon";
import { icPencil } from "../../icons/glyphs";

/// 折叠时露出的行数（Kimi 实拍：3 行 + 「还有 N 个文件」）。
const PREVIEW_ROWS = 3;

type FileChange = { path: string; added: number; removed: number };

/// 按文件汇总一段回复里的 Edit/Write：同一文件多次编辑合并成一行、增删累加，顺序按首次出现。
/// 失败的调用没落盘，不计。
export function collectFileChanges(items: ChatItem[], cwd: string | null): FileChange[] {
  const results = new Map<string, ToolResultItem>();
  for (const item of items) {
    if (item.type === "tool_result" && item.tool_use_id) results.set(item.tool_use_id, item);
  }
  const byPath = new Map<string, FileChange>();
  for (const item of items) {
    if (item.type !== "tool_use") continue;
    const kind = toolKind(item.name);
    if (kind !== "edit" && kind !== "write") continue;
    const result = results.get(item.id);
    if (result?.is_error) continue;
    const { detail, delta } = editOutcome(item as ToolUseItem, result);
    if (!detail) continue;
    const path = relativize(detail.path, cwd).replace(/\\/g, "/");
    const key = path.toLowerCase();
    const entry = byPath.get(key) ?? { path, added: 0, removed: 0 };
    entry.added += delta?.added ?? 0;
    entry.removed += delta?.removed ?? 0;
    byPath.set(key, entry);
  }
  return [...byPath.values()];
}

/// 回复末尾的「N 个文件已修改」卡片（对照 Kimi 的回合改动汇总）：目录弱化、文件名加粗，
/// 增删数靠右；超过 3 个文件时折叠，点「还有 N 个文件」展开。
export const TurnChanges = memo(function TurnChanges({ items }: { items: ChatItem[] }) {
  const t = useT();
  const cwd = useContext(ChatCwdContext);
  const [expanded, setExpanded] = useState(false);
  const files = collectFileChanges(items, cwd);
  if (files.length === 0) return null;
  const shown = expanded ? files : files.slice(0, PREVIEW_ROWS);
  const hidden = files.length - PREVIEW_ROWS;
  return (
    <div className="chat-turn-changes">
      <div className="chat-turn-changes-head">
        <Icon glyph={icPencil} size={14} />
        <span>{t.chat.turnChangedFiles(files.length)}</span>
      </div>
      <ul className="chat-turn-changes-list">
        {shown.map((file) => {
          const cut = file.path.lastIndexOf("/");
          return (
            <li key={file.path} className="chat-turn-changes-row" title={file.path}>
              <span className="chat-turn-changes-path">
                {cut >= 0 && <span className="chat-turn-changes-dir">{file.path.slice(0, cut + 1)}</span>}
                <span className="chat-turn-changes-name">{file.path.slice(cut + 1)}</span>
              </span>
              {file.added > 0 && <span className="is-add">+{file.added}</span>}
              {file.removed > 0 && <span className="is-del">-{file.removed}</span>}
            </li>
          );
        })}
      </ul>
      {hidden > 0 && (
        <button type="button" className="chat-turn-changes-more" aria-expanded={expanded} onClick={() => setExpanded((value) => !value)}>
          {expanded ? t.chat.turnChangedLess : t.chat.turnChangedMore(hidden)}
          <span className="chat-turn-changes-caret" aria-hidden="true">›</span>
        </button>
      )}
    </div>
  );
});
