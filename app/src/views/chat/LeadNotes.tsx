/// Lead 面板的「决策」「进度」页签。解析见 leadNotesParse.ts；正文一律交给 ChatMarkdown，不自己拼 HTML。
///
/// 版式取舍（2026-11-19，对照 Linear issue activity / project updates、GitHub PR 时间线、Kimi 与 Claude 的任务面板）：
/// - 进度倒序：GitHub PR、Linear issue 的活动流是正序，因为最底下就是评论框、读完接着写；这里是只读的状态窗，
///   leader 来看的是「现在到哪了」，所以照 Linear project updates、GitHub Releases 的做法把最新一节放最上。
///   旧节收成一行（日期 · 小标题 · N 条），同 GitHub 时间线把中间的事件折成「N hidden items」。
/// - 决定卡：编号与标题一行、日期靠右；leader 原话单独一块（引文样式，带「leader 原话」标签），agent 归纳的要点
///   在下面用普通列表。区分靠底色与标签，不用左侧竖条（standards.md：强调不用左侧彩色竖条）。

import { useEffect, useMemo, useState } from "react";
import { useT } from "../../i18n";
import { ChatMarkdown } from "../ChatMarkdown";
import { isLeadResumeSection, parseLeadDecisions, parseLeadProgress, type LeadProgressSection } from "./leadNotesParse";

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
            <div className="lead-note-body chat-md"><ChatMarkdown text={entry.body} /></div>
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
            {entry.body && <div className="lead-note-body chat-md"><ChatMarkdown text={entry.body} /></div>}
          </li>
        ))}
      </ol>
      {log.truncated && <Earlier file="decisions.md" />}
    </div>
  );
}

/// 节的键：日期 + 标题 + 同名节里的序号（从最旧数起：新节追加在最新一端，旧节的键不跟着变）。
/// 「更早的记录」那节（截断后开头的半节）固定一个键：截断点随文件增长往后挪，它的内容会变。
export function leadSectionKeys(sections: LeadProgressSection[]): string[] {
  const seen = new Map<string, number>();
  const keys = new Array<string>(sections.length);
  for (let i = sections.length - 1; i >= 0; i--) {
    if (sections[i].earlier) {
      keys[i] = "earlier";
      continue;
    }
    const base = `${sections[i].date ?? ""}|${sections[i].title ?? ""}`;
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    keys[i] = `${base}|${n}`;
  }
  return keys;
}

/// 每个任务（slug）每一节的展开 / 收起，记在模块里：切页签（组件重挂）、轮询刷新、追加新节都不丢。
/// 没记过的节按默认：最新一节展开，其余收起；最新一节第一次显示时就把「展开」记下，后来追加了新节它也不会自己收起。
const openBySlug = new Map<string, Map<string, boolean>>();

function sectionOpenState(slug: string): Map<string, boolean> {
  let map = openBySlug.get(slug);
  if (!map) {
    map = new Map();
    openBySlug.set(slug, map);
  }
  return map;
}

/// 测试用：清掉记下的展开状态。
export function resetLeadProgressOpen() {
  openBySlug.clear();
}

/// 接续摘要钉在进度页最上面（2026-11-24）：它是「现在到哪了、下一步做什么」，leader 打开进度页第一眼要看的就是它；
/// 写在 progress.md 开头，时间线按节倒序会把它排到最底，长任务里还落在末尾 6000 字之外。版式同决定卡（底色块，不用左侧竖条）。
function LeadResume({ text }: { text: string }) {
  const t = useT();
  return (
    <section className="lead-resume" aria-label={t.chat.leadResumeTitle}>
      <h4 className="lead-resume-title">{t.chat.leadResumeTitle}</h4>
      <div className="lead-note-body chat-md"><ChatMarkdown text={text} /></div>
    </section>
  );
}

export function LeadProgress({ text, slug = "", resume }: { text: string; slug?: string; resume?: string }) {
  const t = useT();
  const parsed = useMemo(() => parseLeadProgress(text), [text]);
  // 没有摘要的旧任务：时间线原样，不做任何过滤。
  const log = useMemo(() => resume ? { ...parsed, sections: parsed.sections.filter((s) => !isLeadResumeSection(s)) } : parsed, [parsed, resume]);
  const keys = useMemo(() => leadSectionKeys(log.sections), [log]);
  const [, rerender] = useState(0);
  const state = sectionOpenState(slug);
  const latestKey = log.sections[0]?.body ? keys[0] : undefined;
  useEffect(() => {
    if (latestKey && !state.has(latestKey)) state.set(latestKey, true);
  }, [latestKey, state]);
  if (log.sections.length === 0 && !resume) {
    // 空文件、只有 phase 行：当前阶段已在顶部阶段条上，这里如实说没有记录。
    return log.truncated ? <Earlier file="progress.md" /> : <div className="chat-tasks-empty">{t.chat.leadSectionEmpty}</div>;
  }
  return (
    <div className="lead-notes is-progress">
      {resume && <LeadResume text={resume} />}
      <ol className="lead-timeline">
        {log.sections.map((section, index) => {
          const key = keys[index];
          const latest = index === 0;
          const expandable = !!section.body;
          const open = expandable && (state.get(key) ?? latest);
          const date = section.date ?? (section.earlier ? t.chat.leadProgressEarlier : t.chat.leadProgressUndated);
          const title = section.earlier && section.date ? t.chat.leadProgressEarlier : section.title;
          const head = (
            <>
              <span className="lead-entry-line">
                <span className="lead-entry-date">{date}</span>
                {latest && <span className="lead-entry-latest">{t.chat.leadProgressLatest}</span>}
                {!open && section.items > 0 && <span className="lead-entry-count">{t.chat.leadProgressCount(section.items)}</span>}
                {expandable && <span className="lead-check-caret" aria-hidden="true">›</span>}
              </span>
              {title && <span className="lead-entry-title">{title}</span>}
            </>
          );
          return (
            <li key={key} className={"lead-entry" + (latest ? " is-latest" : "") + (open ? " is-open" : "") + (section.earlier ? " is-earlier" : "")}>
              <span className="lead-timeline-marker" aria-hidden="true" />
              <div className="lead-timeline-content">
                {expandable ? (
                  <button type="button" className="lead-entry-head" aria-expanded={open}
                    onClick={() => { state.set(key, !open); rerender((n) => n + 1); }}>
                    {head}
                  </button>
                ) : <div className="lead-entry-head">{head}</div>}
                {open && <div className="lead-note-body chat-md"><ChatMarkdown text={section.body} /></div>}
              </div>
            </li>
          );
        })}
      </ol>
      {log.truncated && <Earlier file="progress.md" />}
    </div>
  );
}
