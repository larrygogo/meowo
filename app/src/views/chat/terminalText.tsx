import { type ReactNode } from "react";

/// 工具展开区里命令与输出的着色（2026-10 用户实拍：通用 hljs bash 只认得 cd/head 与字符串，
/// 管道/&&/参数/环境变量全是一片白，「高亮不够好」）。这里按终端读者的扫读习惯手写一个
/// 轻量分词：命令名、参数、字符串、操作符、变量各一色。只产出 React 文本节点，不拼 HTML。

type Token = { text: string; kind: "name" | "flag" | "str" | "op" | "env" | "var" | "plain" | "space" };

const TOKEN_RE = /(\s+)|('[^']*'?|"(?:\\.|[^"\\])*"?)|(&&|\|\||\d?>&\d|\d?>>?|<<-?|[|;<>()])|(\$\{?[A-Za-z_][A-Za-z0-9_]*\}?)|([^\s'"|;&<>()]+)/g;

function tokenizeLine(line: string): Token[] {
  const tokens: Token[] = [];
  // 命令位：行首、操作符之后；VAR=值 前缀不占命令位。
  let expectName = true;
  for (const match of line.matchAll(TOKEN_RE)) {
    const [text, space, str, op, variable, word] = match;
    if (space) tokens.push({ text, kind: "space" });
    else if (str) { tokens.push({ text, kind: "str" }); expectName = false; }
    else if (op) { tokens.push({ text, kind: "op" }); expectName = /^(&&|\|\||[|;(])$/.test(op); }
    else if (variable) { tokens.push({ text, kind: "var" }); expectName = false; }
    else if (word) {
      if (expectName && /^[A-Za-z_][A-Za-z0-9_]*=/.test(word)) tokens.push({ text, kind: "env" });
      else if (expectName) { tokens.push({ text, kind: "name" }); expectName = false; }
      else tokens.push({ text, kind: /^--?[A-Za-z]/.test(word) ? "flag" : "plain" });
    }
  }
  return tokens;
}

/// heredoc（`<<'X'` … `X`）的正文不是 shell，按普通文本暗色显示，别拿 shell 规则硬拆。
function heredocTerminator(tokens: Token[]): string | null {
  const at = tokens.findIndex((token) => token.kind === "op" && token.text.startsWith("<<"));
  if (at < 0) return null;
  const next = tokens.slice(at + 1).find((token) => token.kind !== "space");
  return next ? next.text.replace(/^['"]|['"]$/g, "") : null;
}

export function CommandLines({ command }: { command: string }) {
  const lines = command.replace(/\n$/, "").split("\n");
  let terminator: string | null = null;
  return (
    <div className="chat-tool-code chat-cmd">
      {lines.map((line, index) => {
        let body: ReactNode;
        if (terminator !== null) {
          if (line.trim() === terminator) terminator = null;
          body = <span className="chat-cmd-heredoc">{line || " "}</span>;
        } else {
          const tokens = tokenizeLine(line);
          terminator = heredocTerminator(tokens);
          body = tokens.length
            ? tokens.map((token, at) => token.kind === "space" || token.kind === "plain"
              ? token.text
              : <span key={at} className={`chat-cmd-${token.kind}`}>{token.text}</span>)
            : " ";
        }
        return (
          <div className="chat-tool-line" key={index}>
            <span className="chat-tool-ln" aria-hidden="true">{index === 0 ? "$" : ""}</span>
            <span className="chat-tool-src">{body}</span>
          </div>
        );
      })}
    </div>
  );
}

/// 去掉一组行的公共前导空白：命令输出常整体缩进（vitest 的汇总行前有 6 个空格），
/// 原样摆在「⎿」后面像错位。
export function dedent(lines: string[]): string[] {
  const indents = lines.filter((line) => line.trim()).map((line) => /^\s*/.exec(line)![0].length);
  const common = indents.length ? Math.min(...indents) : 0;
  return common ? lines.map((line) => line.slice(common)) : lines;
}

const DIFF_MARKER = /^(@@ |diff --git |Diff in |--- |\+\+\+ )/;

/// 输出按行着色：整段像 diff 时 +/- 行染增删色；error/FAIL 行红、warning 行黄。
/// 只着色不改字——复制出来的仍是原文。
export function OutputBlock({ text }: { text: string }) {
  const lines = text.split("\n");
  const diff = lines.some((line) => DIFF_MARKER.test(line));
  return (
    <pre className="chat-tool-output">
      {lines.map((line, index) => {
        const tone = diff && DIFF_MARKER.test(line) ? "hunk"
          : diff && line.startsWith("+") ? "add"
          : diff && line.startsWith("-") ? "del"
          : /^\s*(error|ERROR|FAIL|FAILED|✗|×)\b/.test(line) || /^\s*error(\[|:)/.test(line) ? "err"
          : /^\s*(warning|WARN)\b/i.test(line) ? "warn"
          : null;
        const content = index < lines.length - 1 ? `${line}\n` : line;
        return tone ? <span key={index} className={`is-${tone}`}>{content}</span> : content;
      })}
    </pre>
  );
}
