import { memo, useState } from "react";
import { useT } from "../../i18n";
import { highlightLines } from "./highlight";

/// 预览露出的行数上限。CC 终端里 Write 也只露十来行再写「+262 lines」——整篇摊开会把
/// 对话流顶掉几屏，而用户要的是「看一眼在写什么」，不是在对话页读文件。
const PREVIEW_LINES = 12;

/// 拆行：末尾的换行不算一行（"a\nb\n" 是两行），与后端 `lines()` 的口径一致。
function splitLines(text: string): string[] {
  const lines = text.split("\n");
  if (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
  return lines;
}

/// 带行号（或 diff 的 +/- 记号）的代码行。高亮复用文件面板那套 hljs——highlightLines
/// 逐行对齐，失败整体退纯文本，不会出现错位。
function CodeLines({ lines, lang, start = 1, tone, gutter }: {
  lines: string[];
  lang: string | null;
  start?: number;
  tone?: "add" | "del";
  gutter?: (index: number) => string;
}) {
  const html = highlightLines(lines, lang);
  const mark = (index: number) => gutter ? gutter(index) : tone ? (tone === "add" ? "+" : "-") : String(start + index);
  return (
    <div className={"chat-tool-code" + (tone ? ` is-${tone}` : "")}>
      {html.map((line, index) => (
        <div className="chat-tool-line" key={index}>
          <span className="chat-tool-ln" aria-hidden="true">{mark(index)}</span>
          <span className="chat-tool-src" dangerouslySetInnerHTML={{ __html: line || " " }} />
        </div>
      ))}
    </div>
  );
}

/// 长正文只露前 PREVIEW_LINES 行，余下折成「还有 N 行 · 展开全部」；展开后可收回。
/// 后端截断过的正文在展开到底时提示「只保留了开头」——不能让用户以为文件就这么长。
export const ToolPreview = memo(function ToolPreview({ text, lang, tone, truncated }: { text: string; lang: string | null; tone?: "add" | "del"; truncated?: boolean }) {
  const t = useT();
  const [all, setAll] = useState(false);
  const lines = splitLines(text);
  const shown = all ? lines : lines.slice(0, PREVIEW_LINES);
  const rest = lines.length - shown.length;
  const foldable = lines.length > PREVIEW_LINES;
  return (
    <>
      <CodeLines lines={shown} lang={lang} tone={tone} />
      {foldable && (
        <button type="button" className="chat-tool-more" onClick={() => setAll((value) => !value)}>
          {all ? t.chat.toolShowLess : `${t.chat.toolMoreLines(rest)} · ${t.chat.toolShowAll}`}
        </button>
      )}
      {truncated && all && <div className="chat-tool-note">{t.chat.toolTruncated}</div>}
    </>
  );
});
