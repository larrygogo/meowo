import { memo, useContext, useId, useState } from "react";
import type { ChatItem } from "../../api";
import { useT } from "../../i18n";
import { ChatCwdContext, editOutcome, relativize, UnifiedDiff } from "./ToolActivity";
import { ToolPreview } from "./ToolPreview";
import { languageForPath } from "./highlight";
import type { ToolResultItem, ToolUseItem } from "./shared";
import { Icon } from "../../icons/Icon";
import { icPencil } from "../../icons/glyphs";
import { icSidebarRight } from "../../icons/glyphs.chat";
import { RevealChangeContext } from "./panelReveal";

/// 折叠时露出的行数（Kimi 实拍：3 行 + 「还有 N 个文件」）。
const PREVIEW_ROWS = 3;

type FileChange = { path: string; added: number; removed: number; details: {
  id: string;
  file: ReturnType<typeof editOutcome>["files"][number];
  content: string | null;
  replaceAll: boolean;
  confirmed: boolean;
}[] };

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
    if (item.kind !== "edit" && item.kind !== "write") continue;
    const result = results.get(item.id);
    // 失败、被打断的调用没落盘，不计。
    if (result?.is_error || result?.interrupted) continue;
    const outcome = editOutcome(item as ToolUseItem, result);
    for (const file of outcome.files) {
      const path = relativize(file.path, cwd).replace(/\\/g, "/");
      const key = path.toLowerCase();
      const entry = byPath.get(key) ?? { path, added: 0, removed: 0, details: [] };
      entry.added += file.delta?.added ?? 0;
      entry.removed += file.delta?.removed ?? 0;
      entry.details.push({ id: item.id, file, content: outcome.detail?.kind === "write" ? outcome.detail.content : null,
        replaceAll: outcome.detail?.kind === "edit" && outcome.detail.replace_all, confirmed: !!result });
      byPath.set(key, entry);
    }
  }
  return [...byPath.values()];
}

/// 回复末尾的「N 个文件已修改」卡片（对照 Kimi 的回合改动汇总）：目录弱化、文件名加粗，
/// 增删数靠右；超过 3 个文件时折叠，点「还有 N 个文件」展开。
export const TurnChanges = memo(function TurnChanges({ items }: { items: ChatItem[] }) {
  const t = useT();
  const cwd = useContext(ChatCwdContext);
  const reveal = useContext(RevealChangeContext);
  const [expanded, setExpanded] = useState(false);
  const [openFile, setOpenFile] = useState<string | null>(null);
  const detailId = useId();
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
            <li key={file.path}>
              {/* 点行照旧展开本回合的修改记录；行尾另有一个「在「更改」中查看」，跳到右侧面板里这个文件的
                  完整 diff（同 VS Code 改动清单的行尾操作）。两个分开放：不把整行变成跳转，原来的展开不受影响。 */}
              <div className="chat-turn-changes-line">
                <button type="button" className="chat-turn-changes-row" data-tip={file.path}
                  aria-expanded={openFile === file.path} aria-controls={openFile === file.path ? detailId : undefined}
                  onClick={() => setOpenFile((value) => value === file.path ? null : file.path)}>
                  <span className="chat-turn-changes-path">
                    {cut >= 0 && <span className="chat-turn-changes-dir">{file.path.slice(0, cut + 1)}</span>}
                    <span className="chat-turn-changes-name">{file.path.slice(cut + 1)}</span>
                  </span>
                  {file.added > 0 && <span className="is-add">+{file.added}</span>}
                  {file.removed > 0 && <span className="is-del">-{file.removed}</span>}
                  <span className="chat-turn-changes-caret" aria-hidden="true">›</span>
                </button>
                {reveal && (
                  <button type="button" className="chat-turn-changes-open" data-tip={t.chat.revealInChanges}
                    aria-label={`${t.chat.revealInChanges} ${file.path}`}
                    // 传工具记录里的原路径（多半是绝对路径），面板按它找仓库；file.path 是相对会话目录的显示名。
                    onClick={() => reveal(file.details[0]?.file.path ?? file.path)}>
                    <Icon glyph={icSidebarRight} size={14} />
                  </button>
                )}
              </div>
              {openFile === file.path && <div id={detailId} className="chat-turn-changes-detail" role="region" aria-label={file.path}>
                <p className="chat-tool-note">{t.chat.turnChangeHistory}</p>
                {file.details.map((detail, index) => <div className="chat-edit-card" key={`${detail.id}-${index}`}>
                  {file.details.length > 1 && <div className="chat-edit-card-head">{t.chat.turnChangeStep(index + 1, file.details.length)}</div>}
                  {!detail.confirmed && <div className="chat-tool-note">{t.chat.turnChangePending}</div>}
                  {detail.replaceAll && <div className="chat-tool-note">{t.chat.toolReplaceAll}</div>}
                  {detail.file.patch ? <div className="chat-edit-card-code"><UnifiedDiff hunks={detail.file.patch} lang={languageForPath(file.path)} /></div>
                    : detail.content != null ? <div className="chat-edit-card-code"><ToolPreview text={detail.content} lang={languageForPath(file.path)} /></div>
                    : <div className="chat-tool-note">{t.chat.turnChangeMissing}</div>}
                  {detail.file.truncated && <div className="chat-tool-note">{t.chat.toolTruncated}</div>}
                </div>)}
              </div>}
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
