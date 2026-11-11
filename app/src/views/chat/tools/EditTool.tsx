import { memo, useContext } from "react";
import { useT } from "../../../i18n";
import { ToolDisclosure } from "../ToolDisclosure";
import { toolShortName } from "../ToolIcon";
import { ToolPreview } from "../ToolPreview";
import { ChatCwdContext, editOutcome, toolSummary, UnifiedDiff } from "../ToolActivity";
import { OutputBlock } from "../terminalText";
import { stripAnsi, type ToolResultItem, type ToolUseItem } from "../shared";
import { languageForPath } from "../highlight";
import { copyPlainText } from "../clipboard";
import { Icon } from "../../../icons/Icon";
import { icCopy } from "../../../icons/glyphs.chat";

/// Edit/Write 工具：收起时只有摘要行；展开后是「文件 + 增删 + 复制」卡片头与着色 diff。
export const EditTool = memo(function EditTool({ item, result }: { item: ToolUseItem; result?: ToolResultItem }) {
  const t = useT();
  const cwd = useContext(ChatCwdContext);
  const { detail, path, patch, delta } = editOutcome(item, result);
  const kind = detail?.kind === "write" ? "write" : "edit";
  const deltaView = delta && (
    <span className="chat-tool-delta">
      {delta.added > 0 && <span className="is-add">+{delta.added}</span>}
      {delta.removed > 0 && <span className="is-del">-{delta.removed}</span>}
    </span>
  );
  const preview = patch ? <UnifiedDiff hunks={patch} lang={languageForPath(path)} />
    : detail?.kind === "write" ? <ToolPreview text={detail.content} lang={languageForPath(path)} truncated={detail.truncated} />
    : null;
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
          </div>
          {(!preview || result?.is_error) && <OutputBlock text={result ? stripAnsi(result.text) || t.chat.toolNoOutput : t.chat.toolRunning} />}
        </>
      }
    />
  );
});
