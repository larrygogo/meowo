import { memo, useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { openLink, revealTerminalPath } from "../api";
import { remoteUi } from "../remoteMode";
import { openInPanel } from "./chat/panelLinks";
import { useT } from "../i18n";
import { formatBackendError } from "../i18n/errors";
// 语法高亮复用文件/diff 面板的同一套 hljs 基建（highlight.ts）：语言表、回退策略、
// --cc-syn-* 主题变量全部现成，不另引第二套高亮引擎——两处不分叉。
import { highlightCode, languageForFence, useHighlighter } from "./chat/highlight";
import { copyPlainText } from "./chat/clipboard";
import { markdownTooComplex } from "./chat/markdownGuard";
import { ErrorBoundary } from "../ErrorBoundary";
import { Icon } from "../icons/Icon";
import { icImage } from "../icons/glyphs.chat";

/** 文件面板的 FileMarkdown 复用同一份 remark 插件表——渲染规则两处不分叉。 */
export const PLUGINS = [remarkGfm];

/// 超过这个规模的代码块不高亮：hljs 同步跑在渲染路径上，几百 KB 的日志粘贴会卡住
/// 整窗；纯文本已有滚动限高，大块不上色是可接受的降级。
const HIGHLIGHT_MAX_CHARS = 20_000;
const HIGHLIGHT_MAX_LINES = 200;

/// (lang, code) → hljs HTML 的模块级缓存：流式期间正文每个 delta 都让整条消息重新
/// 渲染，同一批历史代码块会反复走到 code 组件；memo 只挡「text 未变」的整条消息，
/// 挡不住变化那条里的旧块。上限防长会话膨胀，超限整表清空（重算成本可接受）。
const highlightCache = new Map<string, string>();
function cachedHighlight(code: string, lang: string): string {
  const key = lang + String.fromCharCode(0) + code;
  const hit = highlightCache.get(key);
  if (hit !== undefined) return hit;
  const html = highlightCode(code, lang);
  if (highlightCache.size > 500) highlightCache.clear();
  highlightCache.set(key, html);
  return html;
}

/** 制表/框线/方块字符（U+2500–259F）。模型爱用它们画结构图。 */
const BOX_DRAWING = /[─-▟]/;

/**
 * 该字符在终端网格里占几格。生成端（CLI/模型）按 wcwidth 惯例排版：CJK 与全角标点 2 格，
 * 框线 1 格，几何图形/箭头等歧义宽度按 1 格。返回 0 表示交给等宽字体自然渲染（ASCII 等）。
 */
function charCells(cp: number): 0 | 1 | 2 {
  if (
    (cp >= 0x1100 && cp <= 0x115f) || // Hangul Jamo
    (cp >= 0x2e80 && cp <= 0x303e) || // CJK 部首/符号
    (cp >= 0x3041 && cp <= 0x33ff) || // 假名/CJK 标点/全角附号
    (cp >= 0x3400 && cp <= 0x4dbf) ||
    (cp >= 0x4e00 && cp <= 0x9fff) ||
    (cp >= 0xa000 && cp <= 0xa4cf) ||
    (cp >= 0xac00 && cp <= 0xd7a3) || // Hangul 音节
    (cp >= 0xf900 && cp <= 0xfaff) ||
    (cp >= 0xfe30 && cp <= 0xfe4f) ||
    (cp >= 0xff00 && cp <= 0xff60) || // 全角 ASCII
    (cp >= 0xffe0 && cp <= 0xffe6) ||
    cp === 0x3000 || // 全角空格
    (cp >= 0x1f300 && cp <= 0x1faff) || // emoji 主区
    cp >= 0x20000
  ) {
    return 2;
  }
  // 歧义宽度的常客（几何图形 ●○■、箭头 ←→、杂项符号与 dingbats ✕✓⚠ 等）：
  // wcwidth 记 1 格，但雅黑会画得接近全角，不锁宽就会把行推歪。
  // 框线本身（U+2500–259F）不锁——Consolas 有字形且恰为 1 格。
  if (
    (cp >= 0x2190 && cp <= 0x21ff) || // 箭头
    (cp >= 0x25a0 && cp <= 0x27bf) || // 几何图形 + 杂项符号 + dingbats（框线段 2500–259F 在此区间之前）
    (cp >= 0x2b00 && cp <= 0x2bff) || // 补充箭头/星形
    cp === 0x2022
  ) {
    return 1;
  }
  return 0;
}

/**
 * 像终端一样把文本钉到字符网格上：占 2 格的字符包进 2ch 盒子、歧义符号锁 1ch，
 * 其余走等宽字体自然流。对齐从此不依赖用户装了哪款字体——这是 xterm 的策略在 DOM 里的复刻。
 * 连续的普通字符合并成整段文本节点，span 数只与宽字符数量同阶。
 */
function renderGrid(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  let plain = "";
  let key = 0;
  for (const ch of text) {
    const cells = charCells(ch.codePointAt(0) ?? 0);
    if (cells === 0) {
      plain += ch;
      continue;
    }
    if (plain) {
      out.push(plain);
      plain = "";
    }
    out.push(<span key={key++} className={cells === 2 ? "chat-md-cell2" : "chat-md-cell1"}>{ch}</span>);
  }
  if (plain) out.push(plain);
  return out;
}

/// 代码块自动换行（对照 Kimi 代码块的「开启自动换行」）：长命令、长日志行在窄窗口和手机上
/// 只能横向拖着读，读到行尾就看不到行首。做成一个**全局**偏好而不是逐块开关：想换行的人
/// 是嫌横滚难读，这个诉求对每个代码块都一样，逐块点是重复劳动。按设备记住（localStorage）。
const CODE_WRAP_KEY = "meowo-chat-code-wrap";
const codeWrapListeners = new Set<() => void>();
function readCodeWrap(): boolean {
  try { return localStorage.getItem(CODE_WRAP_KEY) === "1"; } catch { return false; }
}
let codeWrap = readCodeWrap();
function setCodeWrap(next: boolean) {
  codeWrap = next;
  try { localStorage.setItem(CODE_WRAP_KEY, next ? "1" : "0"); } catch { /* 隐私模式：只在本次生效 */ }
  codeWrapListeners.forEach((listener) => listener());
}
function useCodeWrap(): boolean {
  return useSyncExternalStore((listener) => {
    codeWrapListeners.add(listener);
    return () => { codeWrapListeners.delete(listener); };
  }, () => codeWrap);
}

/** 带复制按钮与语言角标的代码块外壳。AI 输出的代码是最高频「要拿走」的内容，
 *  此前只能跨滚动区手动框选。文本从 DOM 取（innerText）——网格重排（chat-md-diagram）
 *  后 children 已不是纯文本，从 props 拼会丢字。 */
function CopyablePre({ children }: { children?: ReactNode }) {
  const t = useT();
  const preRef = useRef<HTMLPreElement>(null);
  const [copied, setCopied] = useState(false);
  const wrap = useCodeWrap();
  // 换行钮只在真的有横向溢出（或已开换行、要能关回去）时出现：短代码块挂一个点了没变化的
  // 按钮只是噪声。框图不给——它的对齐靠一行不断，换行就散架。
  const [overflowing, setOverflowing] = useState(false);
  const [diagram, setDiagram] = useState(false);
  useEffect(() => {
    const el = preRef.current;
    if (!el) return;
    const measure = () => {
      setDiagram(el.querySelector(".chat-md-diagram") !== null);
      setOverflowing(el.scrollWidth > el.clientWidth + 1);
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [children]);
  const copy = () => {
    const text = preRef.current?.innerText ?? "";
    if (!text) return;
    const done = () => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    };
    if (navigator.clipboard?.writeText) {
      void navigator.clipboard.writeText(text).then(done).catch(() => {});
      return;
    }
    // 回退:远程页跑在明文 HTTP(局域网 IP 非 secure context),navigator.clipboard
    // 直接是 undefined——没有这条回退,手机上代码一个字都拿不走。
    // iOS Safari 三件套:readOnly(否则弹键盘且可能不建选区)、钉在 (0,0)(fixed 不给
    // 坐标会落在静态位,select 把页面滚走)、setSelectionRange(iOS 对 select() 不认账)。
    const stage = document.createElement("textarea");
    stage.value = text;
    stage.readOnly = true;
    stage.style.position = "fixed";
    stage.style.top = "0";
    stage.style.left = "0";
    stage.style.opacity = "0";
    document.body.appendChild(stage);
    stage.focus();
    stage.setSelectionRange(0, stage.value.length);
    try {
      if (document.execCommand("copy")) done();
    } catch {
      /* 连 execCommand 都没有就没辙了 */
    }
    stage.remove();
  };
  // 语言角标：react-markdown 给 code 子元素挂 language-xxx 类，从子节点 props 里读。
  let lang: string | null = null;
  if (children && typeof children === "object" && "props" in children) {
    const cls = (children as { props?: { className?: string } }).props?.className ?? "";
    lang = /language-(\w+)/.exec(cls)?.[1] ?? null;
  }
  // 顶栏常驻（A+B 改版定稿）：语言角标 + 复制。此前悬停才显形，代码块看着像一段没有出处
  // 的灰底文字，复制入口也要先找；顶栏占一行但不压代码，阅读时不抢戏的诉求仍然成立。
  return (
    <div className={"chat-md-pre" + (wrap && !diagram ? " is-wrapped" : "")}>
      <div className="chat-md-pre-head">
        {lang && <span className="chat-md-lang">{lang}</span>}
        {!diagram && (overflowing || wrap) && (
          <button type="button" className="chat-md-pre-wrap" aria-pressed={wrap} onClick={() => setCodeWrap(!wrap)}>
            {wrap ? t.chat.codeUnwrap : t.chat.codeWrap}
          </button>
        )}
        <button type="button" className="chat-md-pre-copy" onClick={copy}>{copied ? t.chat.codeCopied : t.chat.codeCopy}</button>
      </div>
      {/* 横向溢出时代码块是可滚动区域，键盘用户要能 Tab 进来用方向键滚（axe scrollable-region-focusable，
          验收 #23）。不溢出时不进 Tab 序，免得每个短代码块都多停一下。 */}
      <pre ref={preRef} {...(overflowing ? { tabIndex: 0, role: "region", "aria-label": t.chat.codeBlockLabel(lang) } : {})}>{children}</pre>
    </div>
  );
}

/// 表格 → markdown 源样式的纯文本（单元格里的 | 转义、换行压成空格）。
export function tableToMarkdown(table: HTMLTableElement): string {
  const rows = Array.from(table.rows).map((row) =>
    Array.from(row.cells).map((cell) => (cell.textContent ?? "").replace(/\s+/g, " ").trim().replace(/\|/g, "\\|")));
  if (rows.length === 0) return "";
  const line = (cells: string[]) => `| ${cells.join(" | ")} |`;
  const out = [line(rows[0])];
  if (table.tHead) out.push(line(rows[0].map(() => "---")));
  for (const row of rows.slice(1)) out.push(line(row));
  return out.join("\n");
}

/// 带「复制表格」的表格外壳（对照 Kimi 的 copyTable）：AI 列的对比表、清单常要贴进文档
/// 或表格软件，框选表格会把相邻正文一起带走、单元格边界也丢了。富文本同时给 HTML 与
/// markdown：贴进飞书/Excel 是真表格，贴进编辑器或聊天框是 markdown 源。
function CopyableTable({ children }: { children?: ReactNode }) {
  const t = useT();
  const tableRef = useRef<HTMLTableElement>(null);
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    const table = tableRef.current;
    if (!table) return;
    const text = tableToMarkdown(table);
    let ok = false;
    // 明文 HTTP 的远程页没有 ClipboardItem/clipboard.write，退回只给纯文本。
    if (typeof ClipboardItem !== "undefined" && navigator.clipboard?.write) {
      try {
        await navigator.clipboard.write([new ClipboardItem({
          "text/html": new Blob([table.outerHTML], { type: "text/html" }),
          "text/plain": new Blob([text], { type: "text/plain" }),
        })]);
        ok = true;
      } catch { /* 权限拒绝：走纯文本 */ }
    }
    if (!ok) ok = await copyPlainText(text);
    if (!ok) return;
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div className="chat-md-table">
      <table ref={tableRef}>{children}</table>
      <button type="button" className="chat-md-table-copy" onClick={() => void copy()}>
        {copied ? t.chat.codeCopied : t.chat.tableCopy}
      </button>
    </div>
  );
}

/// 行内代码里的网址和本地绝对路径可以点（用户 2026-11-06：回复里给的安装包路径想点开）。
/// 网址在默认浏览器打开；路径在文件管理器里选中，不打开也不运行（.exe 点一下就装上太危险）。
/// 路径只认绝对路径：`C:\…`、`\\server\…`，或至少两段的 `/a/b`。单段的 `/model` 是斜杠命令，不算。
const CODE_URL = /^https?:\/\/[^\s<>"]+$/i;
const CODE_PATH = /^(?:[A-Za-z]:[\\/]|\\\\[^\\/\s]+[\\/]|\/[^/\s]+\/)[^\n<>"|?*]*$/;
export function codeLinkKind(text: string): "url" | "path" | null {
  const value = text.trim();
  if (!value || value.includes("\n")) return null;
  if (CODE_URL.test(value)) return "url";
  if (CODE_PATH.test(value)) return "path";
  return null;
}

/// 回复里的路径常带行号（`src/main.ts:42`、`a.ts:10:5`、MSVC 风格 `a.cpp(12,5)`），CODE_PATH
/// 照收，但后端按字面查存在性，带着行号必报「路径不存在」。定位只看文件，行号剥掉再交给后端
///（终端那边 scanLineForFilePaths 的段字符集本来就不含冒号/括号，行号进不了命中）。
export function stripLineSuffix(path: string): string {
  return path.replace(/(?::\d+(?::\d+)?|\(\d+(?:,\d+)?\))$/, "");
}

function LinkCode({ text, kind }: { text: string; kind: "url" | "path" }) {
  const t = useT();
  // 点了打不开（路径已删、被移走）要说出来：这里没有 toast，静默吞掉就是「点了没反应」，
  // 必然引来连点。错误就地跟在链接后面。
  // 带原因的提示不自动消失（做空第一轮 #13，2026-11-12）：原先 4 秒自收，「路径不存在」这类原因
  // 还没读完就没了。现在停留到用户点 × 或下一次点开成功（设计准则「含原因的提示读完前不消失」）。
  const [failed, setFailed] = useState<string | null>(null);
  const codeRef = useRef<HTMLElement>(null);
  const open = () => {
    // 拖选这段文字（想复制）松手时也会触发 click：有选区就当是在选字，不打开。
    if (window.getSelection()?.toString()) return;
    const value = text.trim();
    void (kind === "url" ? openLink(value) : revealTerminalPath("", stripLineSuffix(value))).then(
      () => setFailed(null),
      (e) => setFailed(formatBackendError(e, t.locale)),
    );
  };
  return (
    <>
      <code
        ref={codeRef}
        className="chat-md-link-code"
        role="link"
        tabIndex={0}
        data-tip={kind === "url" ? t.chat.codeOpenLink : t.chat.codeRevealPath}
        onClick={open}
        onKeyDown={(event) => {
          if (event.key === "Enter") open();
        }}
      >{text}</code>
      {failed && (
        <span className="chat-md-link-error">
          <span role="status">{failed}</span>
          {/* × 随提示卸载，焦点交还给这条链接，不掉到 body（设计准则「焦点」）。 */}
          <button type="button" className="chat-md-link-error-close" aria-label={t.chat.close} data-tip={t.chat.close}
            onClick={() => { setFailed(null); codeRef.current?.focus(); }}>×</button>
        </span>
      )}
    </>
  );
}

/// react-markdown 默认的 urlTransform 会把非 http(s)/mailto 的 src 置空（防 javascript:）。
/// 图片降级成文件名 chip 需要**看得到原路径**，被置空就只剩一个没名字的空 chip（复核指出）。
/// 这里放行相对路径与本地绝对路径，仍挡掉可执行 scheme——渲染层从不把它当链接点开，
/// 只用来取末段文件名，风险面仅限于「显示一个字符串」。
function keepUrl(url: string): string {
  return /^\s*(javascript|data|vbscript):/i.test(url) ? "" : url;
}

/** 带语言的围栏代码块。hljs 懒加载（见 chat/highlight.ts）：引擎到位前按原样文本显示，
 *  到位后这一块自己重渲染上色——整条消息是 memo 的，不能指望父层重来。 */
function HighlightedCode({ className, text, lang, children }: { className?: string; text: string; lang: string; children: ReactNode }) {
  const ready = useHighlighter();
  if (!ready) return <code className={className}>{children}</code>;
  // 围栏文本以 \n 收尾（remark 保留）,高亮前剥掉,免得末尾多渲染一个空行。
  const html = cachedHighlight(text.replace(/\n$/, ""), lang);
  return <code className={className} dangerouslySetInnerHTML={{ __html: html }} />;
}

/** 导出供 chat/FileMarkdown.tsx 复用（它只覆写 img，其余规则原样继承）。 */
export const components: Components = {
  pre: ({ children }) => <CopyablePre>{children}</CopyablePre>,
  table: ({ children }) => <CopyableTable>{children}</CopyableTable>,
  // ASCII 框图的对齐前提是「中文恰为两倍拉丁宽」，但代码字体 Consolas 没有中文字形，
  // 中文会回退到比例失配的雅黑；Windows 自带字体里也不存在框线半角 + 中文两倍宽的组合。
  // 故检测到框线字符的代码块直接按网格重排（见 renderGrid）；普通代码块原样输出。
  code: ({ className, children }) => {
    const text = String(children);
    // 框图优先于高亮：对齐比颜色重要，网格重排后的 span 结构也没法再叠 hljs 输出。
    if (BOX_DRAWING.test(text)) {
      return <code className={(className ? className + " " : "") + "chat-md-diagram"}>{renderGrid(text)}</code>;
    }
    // 块级代码（带 language-xxx 类）走 hljs 高亮；内联 code 没有语言类，自然跳过。
    // 文件头「不要换成 dangerouslySetInnerHTML 方案」的禁令针对的是**渲染消息里的
    // 原始 HTML**（XSS 面）；这里的 HTML 由 hljs 从纯文本转义生成（文本全部经它
    // 转义、只插入无属性值注入面的 span），与那条禁令不冲突。
    // 行内代码（没有语言类、不带换行）里是网址或本地路径就做成可点的。手机端的路径对着的是
    // 电脑上的文件，点了也打不开，只给网址。
    if (!className && !text.includes("\n")) {
      const kind = codeLinkKind(text);
      if (kind === "url" || (kind === "path" && !remoteUi())) return <LinkCode text={text} kind={kind} />;
    }
    const tag = /language-([\w+-]+)/.exec(className ?? "")?.[1];
    const lang = tag ? languageForFence(tag) : null;
    if (!lang || text.length > HIGHLIGHT_MAX_CHARS || text.split("\n").length > HIGHLIGHT_MAX_LINES) {
      return <code className={className}>{children}</code>;
    }
    return <HighlightedCode className={className} text={text} lang={lang}>{children}</HighlightedCode>;
  },
  // 7C-10：markdown 里的图片此前完全没管。远端大图按原始像素铺开，把 720px 的消息列
  // 撑出横向滚动条；本地路径/相对路径在 webview 里根本加载不到，只剩一个碎图标，
  // 用户不知道那是什么。远端图限宽自适应，非 http 源降级成与附件同款的文件名 chip。
  img: ({ src, alt }) => {
    const url = typeof src === "string" ? src : "";
    if (!/^https?:\/\//i.test(url)) {
      const name = alt || url.split(/[\\/]/).pop() || url;
      return (
        <span className="chat-image-chip" data-tip={url || name}>
          <Icon glyph={icImage} size={12} />
          <span>{name}</span>
        </span>
      );
    }
    return <img src={url} alt={alt ?? ""} loading="lazy" referrerPolicy="no-referrer" />;
  },
  // 链接绝不能让 webview 自己导航（这个窗口没有地址栏，跳走就回不来了）；
  // 交给后端在默认浏览器打开，scheme 校验也在后端。
  a: ({ href, children }) => {
    // 后端 open_link 只放行 http/https：mailto:/xmpp: 这类链接点了必然被拒，
    // 「可点却必失败」比不可点更糟——渲染层直接降级为纯文本（终端侧另有显示
    // 拒绝原因的通道，见 ManagedTerminal；消息气泡里没有错误展示位）。
    if (!href || !/^https?:\/\//i.test(href)) return <>{children}</>;
    return (
      <a
        href={href}
        data-tip={href}
        onClick={(event) => {
          event.preventDefault();
          // 本机地址（dev server）在右侧面板的浏览器里打开，其余交给系统浏览器。
          if (openInPanel(href)) return;
          void openLink(href).catch(() => {});
        }}
      >
        {children}
      </a>
    );
  },
};

/**
 * 模型输出（正文 / reasoning）的 markdown 渲染。react-markdown 不渲染内嵌原始 HTML，
 * transcript 里的 `<script>` 之类只会按文本展示——不要换成任何 dangerouslySetInnerHTML 方案。
 * memo 按 text 比较：流式期间只有 delta 那一条重新解析，历史消息不重复付解析成本。
 *
 * 两道保护：病态正文（超深嵌套、超长）预检后直接按原文显示，不去解析（markdownGuard.ts）；
 * 预检没拦住的渲染异常由 ErrorBoundary 接住，同样退回原文——一条消息坏了不连累整个窗口。
 */
export const ChatMarkdown = memo(function ChatMarkdown({ text }: { text: string }) {
  if (markdownTooComplex(text)) return <PlainMarkdown text={text} />;
  return (
    <ErrorBoundary scope="markdown" resetKey={text} fallback={() => <PlainMarkdown text={text} />}>
      <ReactMarkdown remarkPlugins={PLUGINS} components={components} urlTransform={keepUrl}>
        {text}
      </ReactMarkdown>
    </ErrorBoundary>
  );
});

/** 不排版、原样显示正文（保留换行，长行折行）。markdown 降级与消息渲染失败共用。 */
export function PlainMarkdown({ text }: { text: string }) {
  return <pre className="chat-md-plain">{text}</pre>;
}

/* 「文件」面板的预览形态（FileMarkdown）住在 chat/FileMarkdown.tsx：它要渲染 README
   的内嵌 HTML，得引 rehype-raw（整套 HTML 解析器，约 193KB min / 43KB br），而对话渲染
   一个字节都用不上。同住一个模块时 Message.tsx 一引 ChatMarkdown 就把它拖进首屏 chunk，
   偏偏改动面板在远程 UI 里是砍掉的——手机端纯白付。拆开后由 GitDiffView 懒加载带出。
   两者共用的 components / PLUGINS 从这里导出，渲染规则不分叉。 */
