/// Lead 面板「决策」「进度」页签的解析：把 decisions.md / progress.md 拆成决定卡与按节的时间线。
///
/// 数据来自 meowo_reporter::lead::snapshot_for：两份文件各取**尾部** 6000 字（按 Unicode 字符数），
/// 所以最旧的一节可能被截在中间。这里只做呈现层的结构化，不改写、不丢内容：
/// 截断时只去掉第一行可能的半句，其余认不出结构的正文单独成「更早的记录」一节或原样一块，交给 ChatMarkdown 渲染。
/// 原则（2026-11-19 独立复核）：修复前面板能看到的内容，修复后也必须能看到。
///
/// 实际写法（2026-11-19 翻过 .lead 下 40 多个任务目录）：
/// - decisions.md：新任务是 `## D<n> 标题（日期，leader：「原话」）` + 若干 `- ` 要点；
///   旧任务有 `| 日期 | 决策 | 理由 | 谁定的 | 推翻条件 |` 表格、`- D0：正文` / `- 标题：正文` 单行条目。
/// - progress.md：首行 `phase: …`；新任务按 `## <日期> <小标题>` 分节，节里有 `###`、表格、代码块；
///   旧任务没有节标题，是 `- 2026-11-09 …` 带日期的条目，同一天后面的条目常不写日期（或只写 `21:36`）。

/// 后端截取的字数（lead.rs 的 tail(…, 6000)），两边要一起改。
export const LEAD_TAIL_LIMIT = 6000;

const DATE = /\d{4}-\d{2}-\d{2}/;
const FENCE = /^\s*(```|~~~)/;
const TOP_ITEM = /^(?:[-*+]|\d+[.)])\s+/;
const H1 = /^#\s+/;
const H2 = /^##\s+(.*)$/;
const PHASE = /^phase\s*:/i;
const SEPARATOR = /^\s*\|?\s*:?-{2,}/;
/// 一行完整的开头：标题、列表项、表格行、围栏。截断点恰好落在行首时，第一行就是这样的一行，不能当半句丢掉。
const LINE_START = /^(?:#{1,6}\s|\s*[-*+]\s|\s*\d+[.)]\s|\s*\||\s*```|\s*~~~)/;

/// 文件的正常开头：`phase:` 行、`#` 总标题、带分隔行的表头、顶层列表项（无标题的旧格式 decisions.md）。
function cleanStart(text: string): boolean {
  if (/^\s*(?:phase\s*:|#\s|[-*+]\s)/i.test(text)) return true;
  const [first = "", second = ""] = text.split(/\r?\n/, 2);
  return first.trim().startsWith("|") && SEPARATOR.test(second);
}

/// 是否被后端尾部截断：字数到了上限、开头又不像文件的正常开头才算。文件恰好 6000 字时开头是完整的，不误报；
/// 代价是截断点恰好落在顶层条目行首时认不出截断（内容一样全显示，只是少了「更早的记录在文件里」那句）。
export function leadTailTruncated(text: string): boolean {
  if ([...text].length < LEAD_TAIL_LIMIT) return false;
  return !cleanStart(text);
}

type Line = { text: string; fenced: boolean };

/// 逐行标出是否在代码块里（代码块里的 `##`、`- ` 不是结构）。围栏行本身算在块里。
/// opened：文本开头就在代码块里（截断点落在代码块中间）。
function scan(lines: string[], opened = false): Line[] {
  let fenced = opened;
  return lines.map((raw) => {
    if (FENCE.test(raw)) {
      fenced = !fenced;
      return { text: raw, fenced: true };
    }
    return { text: raw, fenced };
  });
}

/// 截断后的文本：只去掉第一行可能的半句（不是完整行开头的才丢）；截断点落在代码块里时（围栏总数为奇数、
/// 第一个围栏不带语言名，说明它是收尾的那个），在开头补一个起始围栏，代码照样按代码显示。
function tailLines(text: string, truncated: boolean): Line[] {
  let raw = text.split(/\r?\n/);
  if (!truncated) return scan(raw);
  if (raw.length > 1 && !LINE_START.test(raw[0])) raw = raw.slice(1);
  const fences = raw.filter((l) => FENCE.test(l));
  const firstFence = fences[0]?.trim() ?? "";
  if (fences.length % 2 === 1 && /^(```|~~~)$/.test(firstFence)) return scan([firstFence.slice(0, 3), ...raw], false);
  return scan(raw);
}

/// 去掉首尾空行后拼回文本。
function join(lines: Line[]): string {
  let a = 0;
  let b = lines.length;
  while (a < b && !lines[a].text.trim()) a++;
  while (b > a && !lines[b - 1].text.trim()) b--;
  return lines.slice(a, b).map((l) => l.text).join("\n");
}

// ── 进度 ────────────────────────────────────────────

export type LeadProgressSection = {
  /// 节的日期（YYYY-MM-DD）；没写日期的节为 null。
  date: string | null;
  /// 节标题里日期之后的部分；旧格式（按条目日期分组）没有标题。
  title: string | null;
  /// 节正文（Markdown 原文，交给 ChatMarkdown）。
  body: string;
  /// 顶层条目数（折叠时显示「N 条」）。
  items: number;
  /// 截断后开头那段不属于任何完整节的正文（节标题被截掉了），面板标「更早的记录」。
  earlier?: boolean;
};

export type LeadProgressLog = { sections: LeadProgressSection[]; truncated: boolean };

function countItems(lines: Line[]): number {
  return lines.filter((l) => !l.fenced && TOP_ITEM.test(l.text)).length;
}

/// 日期打头的顶层条目：`- 2026-11-09 定位根因…`、`- 2026-11-16：用户批准…`。返回日期与去掉日期后的条目行。
function datedItem(line: Line): { date: string; text: string } | null {
  if (line.fenced) return null;
  const m = /^([-*+]\s+)(\d{4}-\d{2}-\d{2})(?:[\s：:，,]+|$)(.*)$/.exec(line.text);
  return m ? { date: m[2], text: `${m[1]}${m[3]}` } : null;
}

/// 没有 `##` 节标题的旧格式：按条目日期分组，日期变了开新的一组；不写日期的条目跟着上一组。
/// 第一个带日期的条目之前的内容单独一组（lead：true）。
function groupByItemDate(lines: Line[]): { date: string | null; lines: Line[]; lead: boolean }[] {
  const groups: { date: string | null; lines: Line[]; lead: boolean }[] = [];
  for (const line of lines) {
    const dated = datedItem(line);
    const current = groups.at(-1);
    if (dated && (!current || current.date !== dated.date)) {
      groups.push({ date: dated.date, lines: [{ text: dated.text, fenced: false }], lead: false });
    } else if (current) {
      current.lines.push(dated ? { text: dated.text, fenced: false } : line);
    } else if (line.text.trim()) {
      groups.push({ date: null, lines: [line], lead: true });
    }
  }
  return groups;
}

function splitHeading(text: string): { date: string | null; title: string | null } {
  const m = /^(\d{4}-\d{2}-\d{2})[\s：:，,·—-]*(.*)$/.exec(text.trim());
  if (!m) return { date: null, title: text.trim() || null };
  return { date: m[1], title: m[2].trim() || null };
}

/// progress.md → 按节的时间线，**最新的一节在最前**；节内条目保持原顺序。
/// `phase:` 行与 `#` 总标题不进时间线（当前阶段由面板顶部的阶段条显示，二者同源）。
export function parseLeadProgress(text: string): LeadProgressLog {
  const truncated = leadTailTruncated(text);
  const lines = tailLines(text, truncated).filter((l) => l.fenced || !(PHASE.test(l.text.trim()) || H1.test(l.text)));
  const hasSections = lines.some((l) => !l.fenced && H2.test(l.text));
  const sections: LeadProgressSection[] = [];
  const push = (date: string | null, title: string | null, body: Line[], lead = false) => {
    const text = join(body);
    if (!text && !date && !title) return;
    // 截断后开头那段：节标题已经丢了，标「更早的记录」；日期能从正文里推就推（最早提到的那天）。
    if (lead && truncated) sections.push({ date: date ?? DATE.exec(text)?.[0] ?? null, title: null, body: text, items: countItems(body), earlier: true });
    else sections.push({ date, title, body: text, items: countItems(body) });
  };
  if (!hasSections) {
    for (const g of groupByItemDate(lines)) push(g.date, null, g.lines, g.lead);
  } else {
    let head: { date: string | null; title: string | null } | null = null;
    let body: Line[] = [];
    const flush = () => {
      // 第一个 `##` 之前的内容（前言、旧格式的条目、截断后的半节）按条目日期分组，不丢。
      if (head) push(head.date, head.title, body);
      else for (const g of groupByItemDate(body)) push(g.date, null, g.lines, true);
    };
    for (const line of lines) {
      const m = line.fenced ? null : H2.exec(line.text);
      if (m) {
        flush();
        head = splitHeading(m[1]);
        body = [];
      } else {
        body.push(line);
      }
    }
    flush();
  }
  // 兜底：认不出任何结构也不返回空，原样一节。
  if (sections.length === 0 && text.trim() && lines.some((l) => l.text.trim() && !PHASE.test(l.text.trim()) && !H1.test(l.text))) {
    sections.push({ date: null, title: null, body: join(lines), items: countItems(lines), earlier: truncated || undefined });
  }
  return { sections: sections.reverse(), truncated };
}

// ── 决定 ────────────────────────────────────────────

export type LeadDecision =
  | {
      kind: "card";
      /// 编号：D3、D-17、关卡号 Q-20261113-scope；没有为 null。
      id: string | null;
      title: string;
      date: string | null;
      /// leader 原话：括号里明确写在「leader：」「原话」之后的引号内容，与 agent 归纳的要点分开显示。
      quotes: string[];
      /// 括号里除日期、原话以外的说明：「agent 决定」「关卡 A-…」「无附言」，以及 agent 引用的界面文案。
      meta: string | null;
      /// 要点（Markdown 原文）。
      body: string;
    }
  | { kind: "raw"; body: string };

export type LeadDecisionLog = { entries: LeadDecision[]; truncated: boolean };

const QUOTE_BODY = String.raw`「([^」]+)」|“([^”]+)”|"([^"]+)"`;
/// leader 原话：「leader：」「leader 2026-11-18：」「原话」之后紧跟的引号。agent 决定里引用的「自动」这类界面文案不算。
const LEADER_QUOTE = new RegExp(String.raw`(?:\bleader\b[^，,；;（）()「」“”"]*?[：:]|原话\s*[：:]?)\s*(?:${QUOTE_BODY})`, "gi");

const ID = /^(D-?\d+|[ABCDQ]-\d{8}[\w-]*)(?=$|[\s（(：:，,·])/;

/// 末尾成对的括号（全角或半角，允许嵌套）：返回括号前的文字与括号里的内容。
function trailingParen(text: string): { head: string; inner: string } | null {
  const s = text.trimEnd();
  const last = s.at(-1);
  if (last !== "）" && last !== ")") return null;
  let depth = 0;
  for (let i = s.length - 1; i >= 0; i--) {
    const c = s[i];
    if (c === "）" || c === ")") depth++;
    else if (c === "（" || c === "(") {
      depth--;
      if (depth === 0) return { head: s.slice(0, i).trim(), inner: s.slice(i + 1, -1).trim() };
    }
  }
  return null;
}

/// 取出 leader 原话，返回原话与去掉原话后的文字（「leader：「…」」只留下「leader」，交给 restMeta 收拾）。
function takeLeaderQuotes(text: string): { quotes: string[]; rest: string } {
  const quotes: string[] = [];
  const rest = text.replace(LEADER_QUOTE, (m: string, a?: string, b?: string, c?: string) => {
    quotes.push((a ?? b ?? c ?? "").trim());
    // 保留「leader」这个身份词，去掉冒号与引号。
    return /^\s*原话/.test(m) ? "" : m.replace(/[：:]\s*(?:「[^」]*」|“[^”]*”|"[^"]*")$/, "");
  });
  return { quotes: quotes.filter(Boolean), rest };
}

/// 括号说明去掉日期与原话后剩下的部分；只剩「leader」这类身份词时省掉（原话块已经写明是 leader 的话）。
function restMeta(inner: string, quotes: boolean): string | null {
  const rest = inner
    .replace(new RegExp(DATE.source, "g"), "")
    .replace(/\s*[，,；;]\s*(?=[，,；;]|$)/g, "")
    .replace(/^[\s，,；;：:]+|[\s，,；;：:]+$/g, "")
    .replace(/\s{2,}/g, " ")
    .trim();
  if (!rest) return null;
  if (quotes && /^(?:leader|用户)$/i.test(rest)) return null;
  // 「leader：「原话」，leader 自己……」去掉原话后剩「leader，leader 自己……」：开头的身份词已由原话块交代。
  return quotes ? rest.replace(/^(?:leader|用户)\s*[，,]\s*/i, "") : rest;
}

/// 决定的标题行（`## D1 范围（2026-11-19，leader：「…」）` 或单行条目的冒号前半段）→ 编号、标题、日期、原话、说明。
function parseHead(text: string): Omit<Extract<LeadDecision, { kind: "card" }>, "kind" | "body"> {
  let rest = text.trim();
  let id: string | null = null;
  const idMatch = ID.exec(rest);
  if (idMatch) {
    id = idMatch[1];
    rest = rest.slice(idMatch[0].length).replace(/^[\s·：:，,]+/, "");
  }
  let quotes: string[] = [];
  let meta: string | null = null;
  let date: string | null = null;
  const paren = trailingParen(rest);
  if (paren) {
    const taken = takeLeaderQuotes(paren.inner);
    quotes = taken.quotes;
    date = DATE.exec(paren.inner)?.[0] ?? null;
    meta = restMeta(taken.rest, quotes.length > 0);
    rest = paren.head;
  }
  // 标题打头的日期（`## 2026-11-18 …`、`- 2026-11-16，leader：…`）。
  const lead = /^(\d{4}-\d{2}-\d{2})[\s：:，,·]*/.exec(rest);
  if (lead) {
    date ??= lead[1];
    rest = rest.slice(lead[0].length);
  }
  // 只剩身份词（`2026-11-16，leader：正文`）：它是说明，不是标题。
  if (/^(?:leader|agent|用户)$/i.test(rest.trim())) {
    meta = meta ? `${rest.trim()} · ${meta}` : rest.trim();
    rest = "";
  }
  return { id, title: rest.trim(), date, quotes, meta };
}

/// 单行条目 `- 标题：正文` 的冒号位置：只认括号与「」之外、在前 80 字以内的第一个冒号，正文里的冒号不算。
function headColon(text: string): number {
  let depth = 0;
  let quote = 0;
  for (let i = 0; i < Math.min(text.length, 80); i++) {
    const c = text[i];
    if (c === "（" || c === "(") depth++;
    else if (c === "）" || c === ")") depth = Math.max(0, depth - 1);
    else if (c === "「") quote++;
    else if (c === "」") quote = Math.max(0, quote - 1);
    else if ((c === "：" || (c === ":" && text[i + 1] === " ")) && depth === 0 && quote === 0) return i;
  }
  return -1;
}

function cells(row: string): string[] {
  return row.trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((c) => c.trim());
}

/// Lead skill 旧版决定表的列（2026-11-09 ～ 11-14 的任务都是这五列）。截断把表头截掉时按列数还原。
const DEFAULT_TABLE_HEADER = ["日期", "决策", "理由", "谁定的", "推翻条件"];

/// 表格格式（旧任务）：表头里有「决策 / 决定 / Decision」列才当决定表，每行一张卡。
/// 表头被截掉（第二行不是分隔行）时，列数与旧版决定表一致的行按默认表头还原，列数对不上的行原样保留。
/// 返回的 rawRows 是没能成卡的行（原样显示）；整张表不是决定表时返回 null。
function tableCards(rows: string[]): { cards: LeadDecision[]; rawRows: string[] } | null {
  let header: string[];
  let body: string[];
  if (rows.length >= 2 && SEPARATOR.test(rows[1])) {
    header = cells(rows[0]);
    body = rows.slice(2);
  } else {
    header = DEFAULT_TABLE_HEADER;
    body = rows;
    if (!body.some((r) => cells(r).length === header.length)) return null;
  }
  const col = (re: RegExp) => header.findIndex((h) => re.test(h));
  const what = col(/决策|决定|decision/i);
  if (what < 0) return null;
  const when = col(/日期|时间|date/i);
  const who = col(/谁定|决定人|拍板|who|owner/i);
  const out: LeadDecision[] = [];
  const rawRows: string[] = [];
  for (const row of body.filter((r) => r.trim())) {
    const c = cells(row);
    if (c.length !== header.length) {
      rawRows.push(row);
      continue;
    }
    const quotes: string[] = [];
    // 「leader 原话「…」」写在理由或谁定的里：取出来放进原话块，原文里删掉这一段，免得同一句话出现两遍。
    const take = (value: string) => value.replace(/(?:leader\s*)?原话\s*[“"]?「([^」]+)」[”"]?\s*[；;，,。]?\s*/g, (_m, q: string) => {
      quotes.push(q.trim());
      return "";
    }).trim();
    const points = header.map((h, i) => ({ h, v: i === what || i === when || i === who ? "" : take(c[i] ?? "") }))
      .filter(({ v }) => v && !/^[—–-]+$/.test(v))
      .map(({ h, v }) => `- **${h}**：${v}`);
    const meta = who >= 0 ? take(c[who] ?? "") : "";
    out.push({
      kind: "card" as const,
      id: null,
      title: c[what] ?? "",
      date: when >= 0 ? DATE.exec(c[when] ?? "")?.[0] ?? (c[when] || null) : null,
      quotes,
      meta: meta && !/^[—–-]+$/.test(meta) ? meta : null,
      body: points.join("\n"),
    });
  }
  return { cards: out, rawRows };
}

/// decisions.md → 决定卡，**最新的在最前**（文件按时间追加）。认不出的段落、非决定表格原样成一块 raw，不丢。
export function parseLeadDecisions(text: string): LeadDecisionLog {
  const truncated = leadTailTruncated(text);
  const lines = tailLines(text, truncated).filter((l) => l.fenced || !H1.test(l.text));
  const entries: LeadDecision[] = [];
  let raw: Line[] = [];
  // 正在收集的卡：heading 卡到下一个 `##` 为止；item 卡（单行条目）到空行或下一条为止。
  let card: { kind: "heading" | "item"; head: ReturnType<typeof parseHead>; body: Line[] } | null = null;
  const flushRaw = () => {
    const body = join(raw);
    if (body) entries.push({ kind: "raw", body });
    raw = [];
  };
  const flushCard = () => {
    if (card) entries.push({ kind: "card", ...card.head, body: join(card.body) });
    card = null;
  };
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const h2 = line.fenced ? null : H2.exec(line.text);
    if (h2) {
      flushCard();
      flushRaw();
      card = { kind: "heading", head: parseHead(h2[1]), body: [] };
      continue;
    }
    if (card?.kind === "heading") {
      card.body.push(line);
      continue;
    }
    // 表格：连续的 `|` 行。
    if (!line.fenced && line.text.trim().startsWith("|")) {
      const rows: string[] = [];
      let j = i;
      while (j < lines.length && !lines[j].fenced && lines[j].text.trim().startsWith("|")) rows.push(lines[j++].text);
      const table = tableCards(rows);
      flushCard();
      if (table) {
        raw.push(...table.rawRows.map((text) => ({ text, fenced: false })));
        flushRaw();
        entries.push(...table.cards);
      } else {
        raw.push(...rows.map((text) => ({ text, fenced: false })));
      }
      i = j - 1;
      continue;
    }
    const item = line.fenced ? null : /^[-*+]\s+(.*)$/.exec(line.text);
    if (item) {
      flushCard();
      flushRaw();
      const at = headColon(item[1]);
      const head = at > 0 ? parseHead(item[1].slice(0, at)) : { id: null, title: "", date: null, quotes: [], meta: null };
      card = { kind: "item", head, body: [{ text: at > 0 ? item[1].slice(at + 1).trim() : item[1], fenced: false }] };
      continue;
    }
    if (card?.kind === "item") {
      // 缩进的续行、紧跟的懒续行属于这一条；空行结束它。
      if (line.text.trim()) {
        card.body.push(line);
        continue;
      }
      flushCard();
      continue;
    }
    raw.push(line);
  }
  flushCard();
  flushRaw();
  // 兜底：认不出任何结构也不返回空，原样一块。
  if (entries.length === 0 && join(lines)) entries.push({ kind: "raw", body: join(lines) });
  return { entries: entries.reverse(), truncated };
}
