// AI 交来的文件（Meowo 的 MCP 工具 send_file）与主动提醒（notify_user）。任务 20261123-agent-abilities。
//
// 文件卡：后端把文件复制一份快照（chat_files.rs），回执最后一行 `[File: {JSON}]` 是名字、大小、快照路径和说明。
// 桌面上能打开、在文件夹中显示、另存为；手机上下载走 /download（只认交付目录、流式）。快照被回收后显示「已清理」。
// 提醒行：回执最后一行 `[Notify: {JSON}]` 是级别、内容和实际去向（弹了 / 你正在看 / 通知关了 / 合并了）。
// 两者都只认统一工具名（imageTools.ts），回执是别的工具打出来的同样格式不会画成卡片。
import { memo, useEffect, useMemo, useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { deliveredFileExists, openDeliveredFile, revealDeliveredFile, saveDeliveredFile } from "../../api";
import { deliveredDownloadSrc } from "../../assetSrc";
import { useT } from "../../i18n";
import type { Dict } from "../../i18n/zh";
import { formatBackendError } from "../../i18n/errors";
import { Icon } from "../../icons/Icon";
import { icFile, icFileText, icFileZip, icImage, icPackage, icTerminalBox } from "../../icons/glyphs.chat";
import { icBell, icDownload, icFolderOpen } from "../../icons/glyphs";
import { remoteUi } from "../../remoteMode";
import { useFlash } from "../../hooks/useFlash";
import { ImageFailure } from "./ChatImage";
import { useToolRunState } from "./toolState";
import type { ToolResultItem, ToolUseItem } from "./shared";

export type DeliveredFile = { name: string; size: number; path: string; caption: string | null };
export type NotifyLevel = "needs_you" | "done" | "info";
export type NotifyOutcome = "shown" | "viewing" | "disabled" | "throttled";
export type NotifySummary = { level: NotifyLevel; message: string; outcome: NotifyOutcome };

const FILE_MARKER = "[File: ";
const NOTIFY_MARKER = "[Notify: ";

/** 回执里最后一行以 marker 开头、以 `]` 结尾的 JSON。读不出来返回 null。 */
function markerJson(text: string, marker: string): Record<string, unknown> | null {
  const line = text.split("\n").map((entry) => entry.trim()).reverse().find((entry) => entry.startsWith(marker) && entry.endsWith("]"));
  if (!line) return null;
  try {
    const value = JSON.parse(line.slice(marker.length, -1)) as unknown;
    return value && typeof value === "object" ? value as Record<string, unknown> : null;
  } catch {
    return null;
  }
}

export function parseFileReceipt(text: string): DeliveredFile | null {
  const raw = markerJson(text, FILE_MARKER);
  if (!raw || typeof raw.name !== "string" || typeof raw.path !== "string" || typeof raw.size !== "number") return null;
  const caption = typeof raw.caption === "string" && raw.caption.trim() ? raw.caption.trim() : null;
  return { name: raw.name, size: raw.size, path: raw.path, caption };
}

export function parseNotifyReceipt(text: string): NotifySummary | null {
  const raw = markerJson(text, NOTIFY_MARKER);
  if (!raw || typeof raw.message !== "string") return null;
  const level: NotifyLevel = raw.level === "needs_you" || raw.level === "info" ? raw.level : "done";
  const outcomes: NotifyOutcome[] = ["shown", "viewing", "disabled", "throttled"];
  const outcome = outcomes.includes(raw.outcome as NotifyOutcome) ? raw.outcome as NotifyOutcome : "shown";
  return { level, message: raw.message, outcome };
}

export function formatSize(bytes: number): string {
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  if (unit === 0) return `${bytes} B`;
  return `${value >= 100 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
}

type FileKind = keyof Dict["chat"]["fileKinds"];
const KIND_BY_EXT: [RegExp, FileKind][] = [
  [/^(exe|msi|msix|dmg|pkg|apk|aab|ipa|deb|rpm|appimage)$/, "installer"],
  [/^(zip|7z|rar|tar|gz|tgz|bz2|xz|zst)$/, "archive"],
  [/^pdf$/, "pdf"],
  [/^(doc|docx|odt|rtf|md|txt|pages)$/, "document"],
  [/^(xls|xlsx|ods|csv|tsv|numbers)$/, "sheet"],
  [/^(ppt|pptx|odp|key)$/, "slides"],
  [/^(png|jpe?g|gif|webp|svg|bmp|ico|heic)$/, "image"],
  [/^(mp4|mov|webm|mkv|mp3|wav|m4a|flac|ogg)$/, "media"],
  [/^(log)$/, "log"],
];

/** 类型图标：一眼分出安装包、压缩包、文档；认不出的用普通文件。 */
const KIND_ICON: Partial<Record<FileKind, typeof icFile>> = {
  installer: icPackage,
  archive: icFileZip,
  pdf: icFileText,
  document: icFileText,
  sheet: icFileText,
  slides: icFileText,
  image: icImage,
  log: icTerminalBox,
};

export function fileKind(name: string): FileKind {
  const ext = /\.([a-z0-9]+)$/i.exec(name)?.[1]?.toLowerCase() ?? "";
  return KIND_BY_EXT.find(([pattern]) => pattern.test(ext))?.[1] ?? "other";
}

function Pending({ text }: { text: string }) {
  return (
    <div className="chat-sent-pending chat-file-pending" role="status">
      <span className="chat-model-spinner" aria-hidden="true" />
      {text}
    </div>
  );
}

/** 一条 send_file 调用：还在复制时转圈，失败给原因，成功出文件卡。 */
export const FileMessage = memo(function FileMessage({ item, result }: { item: ToolUseItem; result?: ToolResultItem }) {
  const t = useT();
  const state = useToolRunState(result);
  const file = useMemo(() => (result && !result.is_error ? parseFileReceipt(result.text) : null), [result]);
  let content;
  if (state === "running" || state === "waiting") content = <Pending text={t.chat.fileSending} />;
  else if (state === "interrupted") content = <ImageFailure title={t.chat.fileFailed} reason={t.chat.interrupted} name={null} />;
  else if (!file) content = <ImageFailure title={t.chat.fileFailed} reason={result?.text.trim() || t.chat.imageErrors.unknown} name={null} />;
  else content = <FileCard file={file} />;
  return (
    <article className="chat-message chat-sent" data-enter-id={item.id}>
      {content}
      {file?.caption && <div className="chat-sent-caption">{file.caption}</div>}
    </article>
  );
});

export function FileCard({ file }: { file: DeliveredFile }) {
  const t = useT();
  const remote = remoteUi();
  const { shown: saved, flash } = useFlash();
  const [error, setError] = useState<string | null>(null);
  // 快照被回收（交付目录超了上限）以后：卡片还在，按钮收起，写明已清理。手机端不问，下载时后端回 404 再说。
  const [missing, setMissing] = useState(false);
  useEffect(() => {
    if (remote) return;
    let alive = true;
    deliveredFileExists(file.path).then((exists) => { if (alive) setMissing(!exists); }).catch(() => {});
    return () => { alive = false; };
  }, [file.path, remote]);
  const kind = fileKind(file.name);
  const fail = (failure: unknown) => {
    const text = formatBackendError(failure, t.locale);
    if (/delivered\/missing/.test(String(failure))) setMissing(true);
    else setError(text);
  };
  const open = () => { setError(null); openDeliveredFile(file.path).catch(fail); };
  const reveal = () => { setError(null); revealDeliveredFile(file.path).catch(fail); };
  const saveAs = async () => {
    setError(null);
    const ext = /\.([a-z0-9]+)$/i.exec(file.name)?.[1]?.toLowerCase();
    const dest = await saveDialog({ defaultPath: file.name, filters: ext ? [{ name: ext.toUpperCase(), extensions: [ext] }] : undefined }).catch(() => null);
    if (!dest) return;
    try {
      await saveDeliveredFile(file.path, dest);
      flash();
    } catch (failure) {
      fail(failure);
    }
  };
  const href = remote ? deliveredDownloadSrc(file.path) : null;
  // 手机：点下载先问一声（HEAD，不传文件）还在不在。已被回收的不跳到一页报错，卡片改写「已清理」。
  const download = async (event: { preventDefault: () => void }) => {
    if (!href) return;
    event.preventDefault();
    setError(null);
    const head = await fetch(href, { method: "HEAD" }).catch(() => null);
    if (head?.status === 404) {
      setMissing(true);
      return;
    }
    if (!head?.ok) {
      setError(t.chat.fileFailed);
      return;
    }
    const link = document.createElement("a");
    link.href = href;
    link.download = file.name;
    link.rel = "noreferrer";
    document.body.appendChild(link);
    link.click();
    link.remove();
  };
  return (
    <div className="chat-file" data-missing={missing || undefined} data-testid="file-card">
      <div className="chat-file-icon" data-kind={kind} aria-hidden="true">
        <Icon glyph={KIND_ICON[kind] ?? icFile} size={20} />
      </div>
      <div className="chat-file-body">
        <span className="chat-file-name" title={file.name}>{file.name}</span>
        <span className="chat-file-meta">
          {missing ? t.chat.fileMissing : `${t.chat.fileKinds[kind]} · ${formatSize(file.size)}`}
        </span>
        {error && <span className="chat-file-error" role="alert">{error}</span>}
      </div>
      {!missing && (
        <div className="chat-file-actions">
          {remote ? (
            href && (
              <a className="chat-file-btn is-lead" href={href} download={file.name} rel="noreferrer" onClick={(event) => void download(event)}>
                <Icon glyph={icDownload} size={14} />
                <span>{t.chat.fileDownload}</span>
              </a>
            )
          ) : (
            <>
              <button type="button" className="chat-file-btn is-lead" onClick={open}>{t.chat.fileOpen}</button>
              <button type="button" className="chat-file-btn is-icon" onClick={reveal} aria-label={t.chat.fileReveal} data-tip={t.chat.fileReveal}>
                <Icon glyph={icFolderOpen} size={16} />
              </button>
              <button type="button" className="chat-file-btn is-icon" onClick={() => void saveAs()} aria-label={saved ? t.chat.fileSaved : t.chat.fileSaveAs} data-tip={saved ? t.chat.fileSaved : t.chat.fileSaveAs}>
                <Icon glyph={icDownload} size={16} />
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}

/** 一条 notify_user 调用：一行「提醒了你」，写明级别和实际去向（没弹出来的要说为什么）。 */
export const NotifyMessage = memo(function NotifyMessage({ item, result }: { item: ToolUseItem; result?: ToolResultItem }) {
  const t = useT();
  const state = useToolRunState(result);
  const summary = useMemo(() => (result && !result.is_error ? parseNotifyReceipt(result.text) : null), [result]);
  if (state === "running" || state === "waiting") return null;
  if (!summary) {
    return (
      <article className="chat-message chat-sent" data-enter-id={item.id}>
        <ImageFailure title={t.chat.notifyFailed} reason={state === "interrupted" ? t.chat.interrupted : result?.text.trim() || t.chat.imageErrors.unknown} name={null} />
      </article>
    );
  }
  const note = summary.outcome === "shown" ? null : t.chat.notifyOutcome[summary.outcome];
  return (
    <article className="chat-message chat-notify" data-level={summary.level} data-enter-id={item.id} data-testid="notify-line">
      <span className="chat-notify-icon" aria-hidden="true"><Icon glyph={icBell} size={14} /></span>
      <div className="chat-notify-body">
        <span className="chat-notify-head">
          <span className="chat-notify-title">{t.chat.notifyTitle}</span>
          <span className="chat-notify-level">{t.chat.notifyLevels[summary.level]}</span>
          {note && <span className="chat-notify-note">{note}</span>}
        </span>
        <span className="chat-notify-text">{summary.message}</span>
      </div>
    </article>
  );
});
