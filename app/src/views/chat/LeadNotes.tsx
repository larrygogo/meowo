/// Lead 面板的「决策」「进度」页签。解析见 leadNotesParse.ts；正文一律交给 ChatMarkdown，不自己拼 HTML。
///
/// 版式取舍（2026-11-19，对照 Linear issue activity / project updates、GitHub PR 时间线、Kimi 与 Claude 的任务面板）：
/// - 进度倒序：GitHub PR、Linear issue 的活动流是正序，因为最底下就是评论框、读完接着写；这里是只读的状态窗，
///   leader 来看的是「现在到哪了」，所以照 Linear project updates、GitHub Releases 的做法把最新一节放最上。
///   旧节收成一行（日期 · 小标题 · N 条），同 GitHub 时间线把中间的事件折成「N hidden items」。
/// - 决定卡：编号与标题一行、日期靠右；leader 原话单独一块（引文样式，带「leader 原话」标签），agent 归纳的要点
///   在下面用普通列表。区分靠底色与标签，不用左侧竖条（standards.md：强调不用左侧彩色竖条）。

import { useMemo, useState } from "react";
import { useT } from "../../i18n";
import { ChatMarkdown } from "../ChatMarkdown";
import { parseLeadDecisions, parseLeadProgress, type LeadProgressSection } from "./leadNotesParse";

function Earlier({ file }: { file: string }) {
  const t = useT();
  return <p className="lead-notes-earlier">{t.chat.leadEarlierInFile(file)}</p>;
}

export function LeadDecisions({ text }: { text: string }) {
  const t = useT();
  // 解析只在内容变了时做一次：轮询每 4 秒回一份同样的文本，切页签也不重算。
  const log = useMemo(() => parseLeadDecisions(text), [text]);
  if (log.entries.length === 0) {
    return log.truncated ? <Earlier file="decisions.md" /> : <div className="chat-md"><ChatMarkdown text={text.trim() || t.chat.leadSectionEmpty} /></div>;
  }
  return (
    <div className="lead-notes is-decisions">
      <ol className="lead-decisions">
        {log.entries.map((entry, index) => entry.kind === "raw" ? (
          <li key={index} className="lead-decision is-raw">
            <div className="lead-note-body"><ChatMarkdown text={entry.body} /></div>
          </li>
        ) : (
          <li key={index} className="lead-decision">
            {(entry.id || entry.title || entry.date) && (
              <div className="lead-decision-head">
                {entry.id && <span className="lead-decision-id">{entry.id}</span>}
                {entry.title && <h4 className="lead-decision-title">{entry.title}</h4>}
                {entry.date && <time className="lead-decision-date" dateTime={/^\d{4}-\d{2}-\d{2}$/.test(entry.date) ? entry.date : undefined}>{entry.date}</time>}
              </div>
            )}
            {entry.meta && <p className="lead-decision-meta">{entry.meta}</p>}
            {entry.quotes.map((quote, i) => (
              <blockquote key={i} className="lead-decision-quote">
                <span className="lead-decision-quote-label">{t.chat.leadDecisionQuote}</span>
                <span className="lead-decision-quote-text">{t.locale === "en" ? `“${quote}”` : `「${quote}」`}</span>
              </blockquote>
            ))}
            {entry.body && <div className="lead-note-body"><ChatMarkdown text={entry.body} /></div>}
          </li>
        ))}
      </ol>
      {log.truncated && <Earlier file="decisions.md" />}
    </div>
  );
}

/// 节的键：日期 + 标题 + 同名节里的序号（从最旧数起：新节追加在最新一端，旧节的键不跟着变）。
function sectionKeys(sections: LeadProgressSection[]): string[] {
  const seen = new Map<string, number>();
  const keys = new Array<string>(sections.length);
  for (let i = sections.length - 1; i >= 0; i--) {
    const base = `${sections[i].date ?? ""}|${sections[i].title ?? ""}`;
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    keys[i] = `${base}|${n}`;
  }
  return keys;
}

export function LeadProgress({ text }: { text: string }) {
  const t = useT();
  const log = useMemo(() => parseLeadProgress(text), [text]);
  const keys = useMemo(() => sectionKeys(log.sections), [log]);
  // 记的是「用户翻过的节」：默认最新一节展开、其余收起，点一下与默认相反。
  const [flipped, setFlipped] = useState<ReadonlySet<string>>(new Set());
  if (log.sections.length === 0) {
    // 空文件、只有 phase 行：当前阶段已在顶部阶段条上，这里如实说没有记录。
    return log.truncated ? <Earlier file="progress.md" /> : <div className="chat-tasks-empty">{t.chat.leadSectionEmpty}</div>;
  }
  return (
    <div className="lead-notes is-progress">
      <ol className="lead-timeline">
        {log.sections.map((section, index) => {
          const key = keys[index];
          const latest = index === 0;
          const expandable = !!section.body;
          const open = expandable && latest !== flipped.has(key);
          const head = (
            <>
              <span className="lead-entry-line">
                <span className="lead-entry-date">{section.date ?? t.chat.leadProgressUndated}</span>
                {latest && <span className="lead-entry-latest">{t.chat.leadProgressLatest}</span>}
                {!open && section.items > 0 && <span className="lead-entry-count">{t.chat.leadProgressCount(section.items)}</span>}
                {expandable && <span className="lead-check-caret" aria-hidden="true">›</span>}
              </span>
              {section.title && <span className="lead-entry-title">{section.title}</span>}
            </>
          );
          return (
            <li key={key} className={"lead-entry" + (latest ? " is-latest" : "") + (open ? " is-open" : "")}>
              <span className="lead-timeline-marker" aria-hidden="true" />
              <div className="lead-timeline-content">
                {expandable ? (
                  <button type="button" className="lead-entry-head" aria-expanded={open}
                    onClick={() => setFlipped((prev) => { const next = new Set(prev); if (next.has(key)) next.delete(key); else next.add(key); return next; })}>
                    {head}
                  </button>
                ) : <div className="lead-entry-head">{head}</div>}
                {open && <div className="lead-note-body"><ChatMarkdown text={section.body} /></div>}
              </div>
            </li>
          );
        })}
      </ol>
      {log.truncated && <Earlier file="progress.md" />}
    </div>
  );
}
