/// 会话管理页（对照 Kimi Code 的会话管理，用户要求「照 Kimi 做一个完整的」）：
/// 按工作空间（多选）、状态、更新时间筛选全部会话，按页翻看，勾选后批量标记完成 / 恢复进行中，
/// 批量动作可撤销。从侧栏「视图与排序」菜单底部的「会话管理」进入，占据对话主列，「返回」回到对话。
///
/// 筛选条件改完要点「查询」才生效（同 Kimi）：几个条件往往要一起调，每动一下就重查会让表格来回跳。
/// 「标记完成」就是 Meowo 的归档（set_archived），会同时结束还在跑的会话——与侧栏的同名动作一致。

import { useCallback, useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { useT } from "../../i18n";
import { getSessionAdminPage, recentCwds, setArchived, type LiveSession } from "../../api";
import { runBatchCleanup } from "../../worktreeCleanup";
import { folderName, pathKey } from "../../paths";
import { useMenuPopup } from "../menu";
import { CheckIcon } from "../sticker/icons";

type Status = "all" | "open" | "done";
type Filters = { workspaces: string[]; status: Status; days: number | null };
const DEFAULT_FILTERS: Filters = { workspaces: [], status: "all", days: null };
const DAY_OPTIONS = [1, 3, 7, 14, 30, 90];
const PAGE_SIZES = [20, 50, 100];
/// 「选中当前条件下的全部」一次拉多少：后端单页上限。
const MATERIALIZE_PAGE = 200;

/// 表格里的时间：今年的写「MM-DD HH:mm」，往年的带上年份。等宽数字，一列对齐。
function fmtStamp(ms: number | null | undefined): string | null {
  if (ms == null || !Number.isFinite(ms)) return null;
  const date = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  const md = `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
  return date.getFullYear() === new Date().getFullYear() ? md : `${date.getFullYear()}-${md}`;
}

const Chev = () => (
  <svg className="sa-select-chev" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
);

/// 单选下拉（状态 / 更新时间）：样式同 Kimi 的 .sa-select，菜单走 useMenuPopup（Esc / 点外关闭）。
function SaSelect<T extends string | number | null>({ label, value, options, onChange }: {
  label: string;
  value: T;
  options: { value: T; label: string; dot?: "open" | "done" }[];
  onChange: (value: T) => void;
}) {
  const { open, setOpen, pos, ref, btnRef, menuRef, toggle, onKeyDown } = useMenuPopup({ align: "left" });
  const current = options.find((option) => option.value === value) ?? options[0];
  return (
    <div className="dd" ref={ref} onKeyDown={onKeyDown}>
      <button ref={btnRef} type="button" className={"sa-select" + (open ? " is-open" : "")} aria-label={label} aria-haspopup="listbox" aria-expanded={open} onClick={toggle}>
        {current.dot && <i className={`sa-dot sa-dot--${current.dot}`} />}
        <span>{current.label}</span>
        <Chev />
      </button>
      {open && (
        <div className="dd-menu sa-menu" role="listbox" ref={menuRef} style={{ position: "fixed", top: pos.top, bottom: pos.bottom, left: pos.left, minWidth: 160 }}>
          {options.map((option) => (
            <button
              key={String(option.value)}
              type="button"
              role="option"
              aria-selected={option.value === value}
              className="dd-item sf-opt"
              onClick={() => { onChange(option.value); setOpen(false); btnRef.current?.focus(); }}
            >
              <span className="sa-check">{option.value === value && <CheckIcon />}</span>
              {option.dot && <i className={`sa-dot sa-dot--${option.dot}`} />}
              <span className="dd-label">{option.label}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/// 工作空间多选（对照 Kimi）：选中的以小标签显示在按钮里（最多两个，其余「+N」），标签上的 ×
/// 直接移除；菜单顶部可搜索，第一项「全选」。
function WorkspacePicker({ options, value, onChange }: {
  options: { value: string; label: string }[];
  value: string[];
  onChange: (value: string[]) => void;
}) {
  const t = useT();
  const { open, pos, ref, btnRef, menuRef, toggle, onKeyDown } = useMenuPopup({ align: "left" });
  const [query, setQuery] = useState("");
  useEffect(() => { if (!open) setQuery(""); }, [open]);
  const labelOf = (cwd: string) => options.find((option) => option.value === cwd)?.label ?? folderName(cwd) ?? cwd;
  const shown = options.filter((option) => {
    const q = query.trim().toLowerCase();
    return !q || option.label.toLowerCase().includes(q) || option.value.toLowerCase().includes(q);
  });
  const allOn = options.length > 0 && options.every((option) => value.includes(option.value));
  const flip = (cwd: string) => onChange(value.includes(cwd) ? value.filter((entry) => entry !== cwd) : [...value, cwd]);
  return (
    <div className="dd" ref={ref} onKeyDown={onKeyDown}>
      <div
        // 按钮里要放带 × 的标签（button 套 button 非法），用 role=button 的 div；定位只用它的外框。
        ref={btnRef as unknown as RefObject<HTMLDivElement>}
        role="button"
        tabIndex={0}
        className={"sa-select sa-ws-select" + (open ? " is-open" : "")}
        aria-label={t.admin.filterWorkspace}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={toggle}
        onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); toggle(); } }}
      >
        {value.length === 0 ? <span>{t.admin.allWorkspaces}</span> : <>
          {value.slice(0, 2).map((cwd) => (
            <span key={cwd} className="sa-tag">
              <span className="sa-tag-name">{labelOf(cwd)}</span>
              <button
                type="button"
                className="sa-tag-x"
                aria-label={t.admin.removeTag(labelOf(cwd))}
                onClick={(event) => { event.stopPropagation(); flip(cwd); }}
              >×</button>
            </span>
          ))}
          {value.length > 2 && <span className="sa-tag-more">+{value.length - 2}</span>}
        </>}
        <Chev />
      </div>
      {open && (
        <div className="dd-menu sa-menu sa-ws-menu" role="listbox" aria-multiselectable="true" ref={menuRef} style={{ position: "fixed", top: pos.top, bottom: pos.bottom, left: pos.left }}>
          <div className="sa-search">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true"><circle cx="10.8" cy="10.8" r="6.3" /><path d="m15.5 15.5 4 4" /></svg>
            <input className="sa-search-input" autoFocus value={query} placeholder={t.admin.searchWorkspace} aria-label={t.admin.searchWorkspace} onChange={(event) => setQuery(event.target.value)} />
          </div>
          {/* 多选：每项前一个复选框（与表格同款 .sa-cb），没选是空框、选中填色打勾——此前只在选中时
              出一个勾，没选的那格是空白，看不出这是多选、也分不清选没选（用户实拍）。 */}
          <div className="sa-opts">
            {!query.trim() && options.length > 0 && (
              <button type="button" role="option" aria-selected={allOn} className="dd-item sf-opt sa-opt" onClick={() => onChange(allOn ? [] : options.map((option) => option.value))}>
                <span className={"sa-cb" + (allOn ? " on" : value.length > 0 ? " ind" : "")} aria-hidden="true">
                  {allOn ? <CheckIcon /> : value.length > 0 ? <span className="sa-cb-dash" /> : null}
                </span>
                <span className="dd-label">{t.admin.selectAll}</span>
              </button>
            )}
            {shown.map((option) => (
              <button key={option.value} type="button" role="option" aria-selected={value.includes(option.value)} className="dd-item sf-opt sa-opt" data-tip={option.value} onClick={() => flip(option.value)}>
                <span className={"sa-cb" + (value.includes(option.value) ? " on" : "")} aria-hidden="true">{value.includes(option.value) && <CheckIcon />}</span>
                <span className="dd-label sa-ws-name">{option.label}</span>
              </button>
            ))}
            {shown.length === 0 && <div className="sa-menu-empty">{t.admin.noWorkspaceMatch}</div>}
          </div>
        </div>
      )}
    </div>
  );
}

/// 页码条：首页、末页、当前页前后各一页，其余折成「…」。
function pageList(current: number, count: number): (number | "gap")[] {
  const keep = new Set([0, count - 1, current - 1, current, current + 1].filter((page) => page >= 0 && page < count));
  const out: (number | "gap")[] = [];
  let last = -1;
  for (const page of [...keep].sort((a, b) => a - b)) {
    if (page - last > 1) out.push("gap");
    out.push(page);
    last = page;
  }
  return out;
}

export function SessionAdmin({ onBack, onOpen, initialWorkspaces = [], initialStatus = DEFAULT_FILTERS.status }: {
  onBack: () => void;
  onOpen: (sessionId: number) => void;
  /** 打开时预选的工作区（新建会话页「最近会话 · 查看更多」带进来的那一个）。 */
  initialWorkspaces?: string[];
  /** 打开时预选的状态（设置页「已完成会话」入口带进来的「已完成」）。 */
  initialStatus?: Filters["status"];
}) {
  const t = useT();
  const [workspaceOptions, setWorkspaceOptions] = useState<{ value: string; label: string }[]>([]);
  useEffect(() => {
    let alive = true;
    recentCwds(200, true).then((list) => {
      if (!alive) return;
      const seen = new Set<string>();
      setWorkspaceOptions(list.filter((cwd) => {
        const key = pathKey(cwd);
        if (!cwd || seen.has(key)) return false;
        seen.add(key);
        return true;
      }).map((cwd) => ({ value: cwd, label: folderName(cwd) || cwd })));
    }).catch(() => {});
    return () => { alive = false; };
  }, []);

  // draft = 筛选条上正在改的，applied = 「查询」后生效的。
  const [draft, setDraft] = useState<Filters>(() => ({ ...DEFAULT_FILTERS, workspaces: initialWorkspaces, status: initialStatus }));
  const [applied, setApplied] = useState<Filters>(() => ({ ...DEFAULT_FILTERS, workspaces: initialWorkspaces, status: initialStatus }));
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState(PAGE_SIZES[0]);
  const [rows, setRows] = useState<LiveSession[] | null>(null);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);
  // 选中：id → 是否已完成（批量按钮按它分别计数）。跨页保留，换条件时清空。
  const [selected, setSelected] = useState<ReadonlyMap<number, boolean>>(new Map());
  const [allMatching, setAllMatching] = useState(false);
  const [materializing, setMaterializing] = useState(false);
  const [toast, setToast] = useState<{ text: string; undo: (() => void) | null } | null>(null);
  const requestRef = useRef(0);

  const load = useCallback((filters: Filters, pageIndex: number, size: number) => {
    const request = ++requestRef.current;
    setLoading(true);
    getSessionAdminPage({ workspaces: filters.workspaces, status: filters.status, updatedBeforeDays: filters.days, page: pageIndex, pageSize: size })
      .then((result) => {
        if (request !== requestRef.current) return;
        setRows(result.items);
        setTotal(result.total);
        setError(false);
      })
      .catch(() => { if (request === requestRef.current) setError(true); })
      .finally(() => { if (request === requestRef.current) setLoading(false); });
  }, []);
  useEffect(() => { load(applied, page, pageSize); }, [load, applied, page, pageSize]);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 6000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  const query = (filters: Filters) => {
    setApplied(filters);
    setPage(0);
    setSelected(new Map());
    setAllMatching(false);
  };
  const pageCount = Math.max(1, Math.ceil(total / pageSize));
  const pageRows = rows ?? [];
  const pageAllOn = pageRows.length > 0 && pageRows.every((row) => selected.has(row.session.id));
  const pageSomeOn = pageRows.some((row) => selected.has(row.session.id));
  const toggleRow = (row: LiveSession) => {
    setAllMatching(false);
    setSelected((prev) => {
      const next = new Map(prev);
      if (!next.delete(row.session.id)) next.set(row.session.id, row.archived);
      return next;
    });
  };
  const togglePage = () => {
    setAllMatching(false);
    setSelected((prev) => {
      const next = new Map(prev);
      for (const row of pageRows) {
        if (pageAllOn) next.delete(row.session.id);
        else next.set(row.session.id, row.archived);
      }
      return next;
    });
  };
  // 「选中当前条件下的全部 N 项」：按后端单页上限逐页拉齐 id。
  const selectAllMatching = async () => {
    setMaterializing(true);
    try {
      const next = new Map<number, boolean>();
      for (let index = 0; index * MATERIALIZE_PAGE < total; index += 1) {
        const result = await getSessionAdminPage({ workspaces: applied.workspaces, status: applied.status, updatedBeforeDays: applied.days, page: index, pageSize: MATERIALIZE_PAGE });
        for (const row of result.items) next.set(row.session.id, row.archived);
        if (result.items.length < MATERIALIZE_PAGE) break;
      }
      setSelected(next);
      setAllMatching(true);
    } catch {
      setToast({ text: t.admin.loadFailed, undo: null });
    } finally {
      setMaterializing(false);
    }
  };
  const openIds = [...selected].filter(([, done]) => !done).map(([id]) => id);
  const doneIds = [...selected].filter(([, done]) => done).map(([id]) => id);

  /// 批量标记完成 / 恢复进行中：逐条写（set_archived 标记完成时会结束会话，不并发），给结果提示和撤销。
  const batch = async (ids: number[], done: boolean, undoable = true) => {
    let failed = 0;
    const succeeded: number[] = [];
    for (const id of ids) {
      try {
        await setArchived(id, done);
        succeeded.push(id);
      } catch {
        failed += 1;
      }
    }
    setSelected(new Map());
    setAllMatching(false);
    load(applied, page, pageSize);
    const head = done ? t.admin.batchDoneToast(succeeded.length) : t.admin.batchReopenedToast(succeeded.length);
    setToast({
      text: head + (failed > 0 ? t.admin.batchFailedSuffix(failed) : ""),
      undo: undoable && succeeded.length > 0 ? () => { setToast(null); void batch(succeeded, !done, false); } : null,
    });
  };

  /// 批量清理已合并的 worktree（用户定：攒多了要能一次清掉）。清单整体确认一次，之后逐个删；
  /// 某个 worktree 有程序占着时，runWorktreeRemoval 仍会单独问——结束哪个程序必须让用户看到名字。
  const [cleaning, setCleaning] = useState(false);
  const cleanWorktrees = async () => {
    setCleaning(true);
    const text = await runBatchCleanup(t);
    setCleaning(false);
    load(applied, page, pageSize);
    if (text) setToast({ text, undo: null });
  };

  const statusOptions = useMemo(() => [
    { value: "all" as Status, label: t.admin.statusAll },
    { value: "open" as Status, label: t.admin.statusOpen, dot: "open" as const },
    { value: "done" as Status, label: t.admin.statusDone, dot: "done" as const },
  ], [t]);
  const dayOptions = useMemo(() => [
    { value: null as number | null, label: t.admin.timeAll },
    ...DAY_OPTIONS.map((days) => ({ value: days as number | null, label: t.admin.timeDaysAgo(days) })),
  ], [t]);

  return (
    <div className="session-admin">
      <header className="sa-head" data-tauri-drag-region>
        <button type="button" className="sa-back" onClick={onBack} aria-label={t.admin.back} data-tip={t.admin.back}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
        </button>
        <h1 className="sa-title">{t.admin.title}</h1>
      </header>
      <div className="sa-scroll">
        <div className="sa-page">
          <p className="sa-subtitle">{t.admin.subtitle}</p>
          <div className="sa-filters">
            <span className="sa-f-label">{t.admin.filterWorkspace}</span>
            <WorkspacePicker options={workspaceOptions} value={draft.workspaces} onChange={(workspaces) => setDraft((prev) => ({ ...prev, workspaces }))} />
            <span className="sa-f-label">{t.admin.filterStatus}</span>
            <SaSelect label={t.admin.filterStatus} value={draft.status} options={statusOptions} onChange={(status) => setDraft((prev) => ({ ...prev, status }))} />
            <span className="sa-f-label">{t.admin.filterTime}</span>
            <SaSelect label={t.admin.filterTime} value={draft.days} options={dayOptions} onChange={(days) => setDraft((prev) => ({ ...prev, days }))} />
            <span className="sa-f-actions">
              <button type="button" className="sa-btn-q sa-btn-q--primary" onClick={() => query(draft)}>{t.admin.query}</button>
              <button type="button" className="sa-btn-q" onClick={() => { setDraft(DEFAULT_FILTERS); query(DEFAULT_FILTERS); }}>{t.admin.reset}</button>
              {/* 只在看「已完成」时出现：清理对象就是已完成会话的 worktree。 */}
              {applied.status === "done" && (
                <button type="button" className="sa-btn-q" disabled={cleaning} onClick={() => void cleanWorktrees()}>
                  {cleaning ? t.worktree.batchScanning : t.worktree.batch}
                </button>
              )}
            </span>
          </div>
          {selected.size > 0 && (
            <div className="sa-batch" role="toolbar" aria-label={t.admin.batchSelected(selected.size)}>
              <span className="sa-batch-count">{allMatching ? t.admin.allMatchingSelected(selected.size) : t.admin.batchSelected(selected.size)}</span>
              {!allMatching && pageAllOn && total > pageRows.length && (
                <button type="button" className="sa-batch-link" disabled={materializing} onClick={() => void selectAllMatching()}>
                  {materializing ? t.admin.materializingAll : t.admin.selectAllMatching(total)}
                </button>
              )}
              <button type="button" className="sa-batch-link" onClick={() => { setSelected(new Map()); setAllMatching(false); }}>{t.admin.clearSelection}</button>
              <span className="sa-batch-spacer" />
              {openIds.length > 0 && <button type="button" className="sa-btn-q" onClick={() => void batch(openIds, true)}>{t.admin.markDoneCount(openIds.length)}</button>}
              {doneIds.length > 0 && <button type="button" className="sa-btn-q" onClick={() => void batch(doneIds, false)}>{t.admin.reopenCount(doneIds.length)}</button>}
            </div>
          )}
          <div className={"sa-table-card" + (loading && rows ? " is-loading" : "")}>
            <table>
              <colgroup>
                <col className="sa-col-cb" />
                <col className="sa-col-status" />
                <col className="sa-col-title" />
                <col className="sa-col-ws" />
                <col />
                <col className="sa-col-time" />
                <col className="sa-col-time" />
                <col className="sa-col-act" />
              </colgroup>
              <thead>
                <tr>
                  <th className="sa-col-cb">
                    <button
                      type="button"
                      role="checkbox"
                      aria-checked={pageAllOn ? true : pageSomeOn ? "mixed" : false}
                      aria-label={t.admin.selectPageAll}
                      className={"sa-cb" + (pageAllOn ? " on" : pageSomeOn ? " ind" : "")}
                      disabled={pageRows.length === 0}
                      onClick={togglePage}
                    >{pageAllOn ? <CheckIcon /> : pageSomeOn ? <span className="sa-cb-dash" /> : null}</button>
                  </th>
                  <th>{t.admin.colStatus}</th>
                  <th>{t.admin.colTitle}</th>
                  <th>{t.admin.colWorkspace}</th>
                  <th>{t.admin.colPrompt}</th>
                  <th>{t.admin.colUpdated}</th>
                  <th>{t.admin.colCompleted}</th>
                  <th>{t.admin.colActions}</th>
                </tr>
              </thead>
              {rows && rows.length > 0 && (
                <tbody>
                  {rows.map((row) => {
                    const on = selected.has(row.session.id);
                    const updated = fmtStamp(row.session.last_event_at);
                    const completed = row.archived ? fmtStamp(row.archived_at) : null;
                    return (
                      <tr key={row.session.id} className={on ? "is-selected" : ""}>
                        <td className="sa-col-cb">
                          <button type="button" role="checkbox" aria-checked={on} aria-label={row.task_title || t.sticker.waitingFirstInput} className={"sa-cb" + (on ? " on" : "")} onClick={() => toggleRow(row)}>
                            {on && <CheckIcon />}
                          </button>
                        </td>
                        <td>
                          <span className={`sa-st sa-st--${row.archived ? "done" : "open"}`}>
                            <i className={`sa-dot sa-dot--${row.archived ? "done" : "open"}`} />
                            {row.archived ? t.admin.statusDone : t.admin.statusOpen}
                          </span>
                        </td>
                        <td><span className="sa-title-cell" data-tip={row.task_title || undefined}>{row.task_title || t.sticker.waitingFirstInput}</span></td>
                        <td>
                          <span className="sa-ws" data-tip={row.cwd ?? undefined}>
                            <span>{folderName(row.cwd) || t.chat.sidebarNoDir}</span>
                          </span>
                        </td>
                        <td><span className="sa-prompt" data-tip={row.last_user_text ?? undefined}>{row.last_user_text || <span className="sa-none">—</span>}</span></td>
                        <td><span className="sa-time">{updated ?? <span className="sa-none">—</span>}</span></td>
                        <td><span className="sa-time">{completed ?? <span className="sa-none">—</span>}</span></td>
                        <td>
                          <span className="sa-act">
                            <button type="button" className="sa-icon-btn" aria-label={t.admin.open} data-tip={t.admin.open} onClick={() => onOpen(row.session.id)}>
                              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M7 17 17 7M9 7h8v8" /></svg>
                            </button>
                            <button
                              type="button"
                              className="sa-icon-btn"
                              aria-label={row.archived ? t.admin.reopen : t.admin.markDone}
                              data-tip={row.archived ? t.admin.reopen : t.admin.markDone}
                              onClick={() => void batch([row.session.id], !row.archived)}
                            >
                              {row.archived
                                ? <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M9 14 4 9l5-5" /><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11" /></svg>
                                : <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="8.5" /><path d="m8.5 12.2 2.4 2.4 4.6-5" /></svg>}
                            </button>
                          </span>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              )}
            </table>
            {(!rows || rows.length === 0) && (
              <div className="sa-state">
                <span className="sa-empty">{error ? t.admin.loadFailed : !rows ? t.admin.loading : t.admin.empty}</span>
              </div>
            )}
          </div>
          <div className="sa-pager">
            <span className="sa-total">{t.admin.total(total)}</span>
            <div className="sa-pager-right">
              <div className="sa-pages">
                <button type="button" className="sa-pg" aria-label={t.admin.prevPage} disabled={page === 0} onClick={() => setPage(page - 1)}>‹</button>
                {pageList(page, pageCount).map((entry, index) => entry === "gap"
                  ? <span key={`gap-${index}`} className="sa-ellipsis">…</span>
                  : <button key={entry} type="button" className={"sa-pg" + (entry === page ? " cur" : "")} aria-current={entry === page ? "page" : undefined} onClick={() => setPage(entry)}>{entry + 1}</button>)}
                <button type="button" className="sa-pg" aria-label={t.admin.nextPage} disabled={page >= pageCount - 1} onClick={() => setPage(page + 1)}>›</button>
              </div>
              <SaSelect
                label={t.admin.pageSize(pageSize)}
                value={pageSize}
                options={PAGE_SIZES.map((size) => ({ value: size, label: t.admin.pageSize(size) }))}
                onChange={(size) => { setPageSize(size); setPage(0); }}
              />
            </div>
          </div>
        </div>
      </div>
      {toast && (
        <div className="sa-toast" role="status">
          <span>{toast.text}</span>
          {toast.undo && <button type="button" className="sa-batch-link" onClick={toast.undo}>{t.admin.undo}</button>}
        </div>
      )}
    </div>
  );
}
