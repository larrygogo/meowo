import { memo, useContext, useId, useMemo, useState } from "react";
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
import { collectTurnChecks, isDocPath, type CheckKind, type TurnCheck } from "./turnChecks";
import { revealItem } from "./foldMemory";
import { RELEASE_FOLLOW_EVENT } from "./useStickToBottom";

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
  // 只在命令或回执变了时重算：流式输出时每来一段字这张卡都会重渲染，命令多、输出长时一次要 5~20ms（实测 100~400 条）。
  const runSignature = items.map((item) => (item.type === "tool_use" && item.kind === "run") ? item.id
    : item.type === "tool_result" ? `${item.id}:${item.text.length}:${item.is_error ? 1 : 0}:${item.interrupted ? 1 : 0}:${item.exit_code ?? ""}` : "").join(",");
  // eslint-disable-next-line react-hooks/exhaustive-deps -- runSignature 已覆盖 collectTurnChecks 用到的全部字段
  const checks = useMemo(() => collectTurnChecks(items), [runSignature]);
  // 没改文件、检查又全都没有确定结果（多半是用户打断了）：这张卡说不出什么，不出。
  if (files.length === 0 && !checks.some((check) => check.status !== "unknown")) return null;
  const codeFiles = files.filter((file) => !isDocPath(file.path));
  // 最后一次检查之后又改了哪些代码文件：上面的「通过」不含这些改动。
  // 每条检查各自看：它之后有没有又改了代码。从最早一条「过时」的检查算起，列出之后改过的代码文件。
  const staleIds = new Set(checks.filter((check) => editedAfterIndex(items, check.at, cwd).some((path) => !isDocPath(path))).map((check) => check.toolId));
  const firstStale = checks.find((check) => staleIds.has(check.toolId));
  const editedAfter = firstStale ? editedAfterIndex(items, firstStale.at, cwd).filter((path) => !isDocPath(path)) : [];
  const shown = expanded ? files : files.slice(0, PREVIEW_ROWS);
  const hidden = files.length - PREVIEW_ROWS;
  return (
    <div className="chat-turn-changes">
      {files.length > 0 && <div className="chat-turn-changes-head">
        <Icon glyph={icPencil} size={14} />
        <span>{t.chat.turnChangedFiles(files.length)}</span>
      </div>}
      {files.length > 0 && <ul className="chat-turn-changes-list">
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
      </ul>}
      {hidden > 0 && (
        <button type="button" className="chat-turn-changes-more" aria-expanded={expanded} onClick={() => setExpanded((value) => !value)}>
          {expanded ? t.chat.turnChangedLess : t.chat.turnChangedMore(hidden)}
          <span className="chat-turn-changes-caret" aria-hidden="true">›</span>
        </button>
      )}
      <TurnChecks checks={checks} replyId={items[0]?.id ?? null} editedAfter={editedAfter} staleIds={staleIds} codeChanged={codeFiles.length > 0} />
    </div>
  );
});

/// 某个位置之后又改了哪些文件（相对路径，去重，按首次出现）。
function editedAfterIndex(items: ChatItem[], after: number, cwd: string | null): string[] {
  return collectFileChanges(items.slice(after + 1), cwd).map((file) => file.path);
}

const KIND_LABEL = (t: ReturnType<typeof useT>, kind: CheckKind) => t.chat.checkKind[kind];

/// 检查段（对照 GitHub PR 底部的检查区）：一行一项，状态、类别、命令原文、结果。数据全部来自会话记录。
/// 点一行跳回过程里的那次命令：展开回合折叠和那一行，滚过去，看完整输出。
function TurnChecks({ checks, replyId, editedAfter, staleIds, codeChanged }: { checks: TurnCheck[]; replyId: string | null; editedAfter: string[]; staleIds: Set<string>; codeChanged: boolean }) {
  const t = useT();
  if (checks.length === 0) {
    // 改了代码却一项检查都没跑：如实写出来。只改文档不提。
    return codeChanged ? <p className="chat-turn-checks-note">{t.chat.checkNone}</p> : null;
  }
  const jump = (toolId: string, from: HTMLElement) => {
    // 停在底部时对话区在自动跟随：先关掉，否则展开折叠后又被钉回底部。
    from.closest(".chat-scroll")?.dispatchEvent(new Event(RELEASE_FOLLOW_EVENT));
    revealItem(toolId, [...(replyId ? [`reply:${replyId}`] : []), `tool:${toolId}`]);
    // 折叠展开要等下一帧挂载，最多等几帧再滚。滚到以后再看守一小会儿（约 20 帧）：展开引起的排版、
    // 还没派发的滚动事件都可能把自动跟随又打开、把视口钉回底部（手机页 4 次里实测 1 次），被拉走就再关一次、再滚一次。
    const scroller = from.closest<HTMLElement>(".chat-scroll");
    let tries = 0;
    let guard = 0;
    let flashed = false;
    const seek = () => {
      const target = document.querySelector<HTMLElement>(`[data-enter-id="${CSS.escape(toolId)}"]`);
      if (!target) {
        if ((tries += 1) < 10) requestAnimationFrame(seek);
        return;
      }
      const box = scroller?.getBoundingClientRect();
      const rect = target.getBoundingClientRect();
      const visible = !box || (rect.bottom > box.top && rect.top < box.bottom);
      if (!flashed || !visible) {
        scroller?.dispatchEvent(new Event(RELEASE_FOLLOW_EVENT));
        // 瞬时滚过去：平滑滚动的头几帧还在底部附近，滚动事件会把自动跟随又判回「在底部」，被钉回去。
        target.scrollIntoView({ block: "center", behavior: "instant" });
      }
      if (!flashed) {
        flashed = true;
        target.classList.remove("is-flash");
        void target.offsetWidth;
        target.classList.add("is-flash");
      }
      if ((guard += 1) < 20) requestAnimationFrame(seek);
    };
    requestAnimationFrame(seek);
  };
  return (
    <>
      <ul className={"chat-turn-checks" + (codeChanged || editedAfter.length ? "" : " is-only")} aria-label={t.chat.checkTitle}>
        {checks.map((check) => {
          const label = [...new Set(check.kinds)].map((kind) => KIND_LABEL(t, kind)).join(" + ");
          const stale = staleIds.has(check.toolId);
          const result = t.chat.checkResult(check.note) + (stale ? ` · ${t.chat.checkStaleTag}` : "");
          return (
            <li key={`${check.toolId}:${check.keys.join()}`}>
              <button type="button" className={`chat-turn-check is-${check.status}` + (stale ? " is-stale" : "")} data-tip={check.command}
                aria-label={`${label} ${result} · ${t.chat.checkJump}`} onClick={(event) => jump(check.toolId, event.currentTarget)}>
                <span className="chat-turn-check-icon" aria-hidden="true">{check.status === "pass" ? "✓" : check.status === "fail" ? "✕" : "?"}</span>
                <span className="chat-turn-check-kind">{label}</span>
                <code className="chat-turn-check-cmd">{check.command.split(/\r?\n/)[0]}</code>
                <span className="chat-turn-check-result">{result}</span>
              </button>
            </li>
          );
        })}
      </ul>
      {editedAfter.length > 0 && (
        <p className="chat-turn-checks-note is-warn">{t.chat.checkStale(editedAfter.length, editedAfter.map((path) => path.slice(path.lastIndexOf("/") + 1)))}</p>
      )}
    </>
  );
}
