import { memo, useContext } from "react";
import { useT } from "../../../i18n";
import { ToolDisclosure } from "../ToolDisclosure";
import { toolShortName } from "../ToolIcon";
import { ToolPreview } from "../ToolPreview";
import { ChatCwdContext, editOutcome, relativize, toolSummary, UnifiedDiff } from "../ToolActivity";
import type { ToolResultItem, ToolUseItem } from "../shared";
import { ToolBodyText } from "./ToolBodyText";
import { languageForPath } from "../highlight";
import { copyPlainText } from "../clipboard";
import { Icon } from "../../../icons/Icon";
import { icCopy } from "../../../icons/glyphs.chat";

/// Edit/Write 工具：收起时只有摘要行；展开后是「文件 + 增删 + 复制」卡片头与着色 diff。
export const EditTool = memo(function EditTool({ item, result }: { item: ToolUseItem; result?: ToolResultItem }) {
  const t = useT();
  const cwd = useContext(ChatCwdContext);
  const { detail, path, patch, delta, files } = editOutcome(item, result);
  // 「写入」还是「编辑」由后端的类别定（codex 只新建文件的补丁也是写入）。
  const kind = item.kind === "write" ? "write" : "edit";
  const deltaOf = (value: { added: number; removed: number } | null) => value && (
    <span className="chat-tool-delta">
      {value.added > 0 && <span className="is-add">+{value.added}</span>}
      {value.removed > 0 && <span className="is-del">-{value.removed}</span>}
    </span>
  );
  const deltaView = deltaOf(delta);
  const preview = patch ? <UnifiedDiff hunks={patch} lang={languageForPath(path)} />
    : detail?.kind === "write" ? <ToolPreview text={detail.content} lang={languageForPath(path)} truncated={detail.truncated} />
    : null;
  // 一次补丁改了几个文件（codex）：每个文件一张卡，路径、增删与 diff 各归各的。
  const multi = files.length > 1;
  return (
    <ToolDisclosure
      item={item}
      result={result}
      summary={
        <>
          <span className="chat-tool-verb" data-kind={kind}>{toolShortName(item.name, kind, t)}</span>
          <span className="chat-tool-summary">{toolSummary(item, cwd)}</span>
          {deltaView}
        </>
      }
      body={
        <>
          {multi ? files.map((file) => (
            <div className="chat-edit-card" key={file.path}>
              <div className="chat-edit-card-head">
                <span className="chat-edit-card-path">{relativize(file.path, cwd).replace(/\\/g, "/")}</span>
                {deltaOf(file.delta)}
                <button type="button" className="chat-edit-card-copy" aria-label={t.chat.codeCopy} data-tip={t.chat.codeCopy} onClick={() => void copyPlainText(file.path)}>
                  <Icon glyph={icCopy} size={12} />
                </button>
              </div>
              {file.patch && <div className="chat-edit-card-code"><UnifiedDiff hunks={file.patch} lang={languageForPath(file.path)} /></div>}
              {file.truncated && <div className="chat-tool-note">{t.chat.toolTruncated}</div>}
            </div>
          )) : (
            <div className="chat-edit-card">
              <div className="chat-edit-card-head">
                <span className="chat-edit-card-path">{toolSummary(item, cwd)}</span>
                {deltaView}
                <button type="button" className="chat-edit-card-copy" aria-label={t.chat.codeCopy} data-tip={t.chat.codeCopy} onClick={() => void copyPlainText(path)}>
                  <Icon glyph={icCopy} size={12} />
                </button>
              </div>
              {detail?.kind === "edit" && detail.replace_all && <div className="chat-tool-note">{t.chat.toolReplaceAll}</div>}
              {preview && <div className="chat-edit-card-code">{preview}</div>}
              {files[0]?.truncated && patch && <div className="chat-tool-note">{t.chat.toolTruncated}</div>}
            </div>
          )}
          {((!preview && !multi) || !result || result.is_error || result.interrupted) && <ToolBodyText result={result} />}
        </>
      }
    />
  );
});
