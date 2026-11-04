// 灵动岛展开后的黑色面板（照用户参考图：标题 + 日期时间、分页圆点、环形进度）。
// 第 1 页：最要紧的几个会话 + 额度大环；第 2 页（有额度数据才有）：各 agent 的额度。
// 完整列表（搜索、菜单、归档……）不塞进这里：右上「全部」打开原来的贴纸列表。
import { useLayoutEffect, useMemo, useRef, useState } from "react";
import type { ProviderUsage } from "../../api";
import { useT } from "../../i18n";
import { cardTone, fmtAgo } from "../sticker/helpers";
import type { Item } from "../sticker/types";
import { byUrgency, sessionTitle } from "../Island";
import { AgentRing, Ring, useNow, usageColor, usageTrack } from "./islandParts";
import { pickIslandUsage, primaryLane } from "./useIslandUsage";

/// 面板上最多列几个会话。更多的去「全部」里看。
const MAX_ROWS = 3;

function providerLabel(provider: string): string {
  return provider ? provider[0].toUpperCase() + provider.slice(1) : provider;
}

export function IslandPanel({
  sessions,
  usage,
  quotaProviders,
  onOpenSession,
  onShowAll,
  onMeasure,
  onPointerDownDrag,
}: {
  sessions: readonly Item[];
  usage: Record<string, ProviderUsage>;
  quotaProviders: readonly string[];
  onOpenSession: (sessionId: number) => void;
  onShowAll: () => void;
  /** 面板实测高度（逻辑像素），App 据此让窗口贴合。 */
  onMeasure: (height: number) => void;
  onPointerDownDrag: (event: React.PointerEvent<HTMLElement>) => void;
}) {
  const t = useT();
  const now = useNow();
  const panelRef = useRef<HTMLDivElement>(null);
  const [page, setPage] = useState(0);
  const rows = useMemo(() => byUrgency(sessions).slice(0, MAX_ROWS), [sessions]);
  // 大环显示「正在用的」那家：面板里列的会话用的 agent 优先，再按设置顺序。
  const main = pickIslandUsage(usage, quotaProviders, byUrgency(sessions).map((item) => item.provider));
  const usageProviders = Object.keys(usage).filter((p) => primaryLane(usage[p]) != null);
  const pages = usageProviders.length > 0 ? 2 : 1;
  const current = Math.min(page, pages - 1);

  useLayoutEffect(() => {
    const el = panelRef.current;
    if (!el) return;
    const report = () => onMeasure(Math.ceil(el.getBoundingClientRect().height));
    report();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(report);
    observer.observe(el);
    return () => observer.disconnect();
  }, [onMeasure, current]);

  const laneLabel = (kind: string) => t.island.usageLane[kind] ?? t.island.usageLane.other;

  return (
    <div className="isl-stage">
      <div ref={panelRef} className={`isl-notch isl-panel${pages > 1 ? " is-paged" : ""}`}>
        <div className="isl-panel-head" onPointerDown={onPointerDownDrag}>
          <span className="isl-panel-title">
            Meowo <span className="isl-accent">{current === 0 ? t.island.panelSessions : t.island.panelUsage}</span>
          </span>
          <button
            type="button"
            className="isl-btn is-ghost"
            onPointerDown={(event) => event.stopPropagation()}
            onClick={onShowAll}
          >{t.island.showAll}</button>
          <span className="isl-panel-meta">
            {now.toLocaleDateString(t.locale, { month: "long", day: "numeric", weekday: "short" })}
            <b>{now.toLocaleTimeString(t.locale, { hour: "2-digit", minute: "2-digit", hour12: false })}</b>
          </span>
        </div>

        {current === 0 ? (
          <div className="isl-panel-body">
            <div className="isl-rows">
              {rows.length === 0 && <div className="isl-empty">{t.island.idle}</div>}
              {rows.map((item) => {
                const tone = cardTone(item);
                // 不放 current_activity：那是原始工具串（Read: 加完整路径、› cd 加命令），塞进这一行
                // 像调试输出（用户实拍嫌丑）。项目 · 多久前 · 待办 就够认出是哪个。
                const detail = [
                  item.project_name,
                  fmtAgo(item.session.last_event_at, t),
                  item.todo_total > 0 ? t.island.todo(item.todo_done, item.todo_total) : null,
                ].filter(Boolean).join(" · ");
                return (
                  <div
                    key={item.session.id}
                    className="isl-row"
                    role="button"
                    tabIndex={0}
                    onClick={() => onOpenSession(item.session.id)}
                    onKeyDown={(event) => { if (event.key === "Enter") onOpenSession(item.session.id); }}
                  >
                    <AgentRing item={item} size={30} />
                    <span className="isl-row-text">
                      <span className="isl-row-title">{sessionTitle(item, t.sticker.waitingFirstInput)}</span>
                      <span className="isl-row-sub">{detail}</span>
                    </span>
                    <span className={`isl-state is-${tone}`}>{t.island.toneLabel[tone] ?? ""}</span>
                    <span className="isl-btn is-dark" aria-hidden="true">{t.island.open}</span>
                  </div>
                );
              })}
            </div>
            {main && (
              <div className="isl-side">
                <Ring size={84} stroke={6} value={main.pct / 100} color={usageColor(main.pct)} track={usageTrack(main.pct)}>
                  <span className="isl-big-val">{Math.round(main.pct)}<small>%</small></span>
                </Ring>
                <span className="isl-side-label">{providerLabel(main.provider)} {laneLabel(main.kind)}</span>
              </div>
            )}
          </div>
        ) : (
          <div className="isl-panel-body is-usage">
            {usageProviders.map((provider) => {
              const lane = primaryLane(usage[provider]);
              if (!lane) return null;
              return (
                <div key={provider} className="isl-usage-cell">
                  <Ring size={64} stroke={5} value={lane.pct / 100} color={usageColor(lane.pct)} track={usageTrack(lane.pct)}>
                    <span className="isl-mid-val">{Math.round(lane.pct)}<small>%</small></span>
                  </Ring>
                  <span className="isl-side-label">{providerLabel(provider)}</span>
                  <span className="isl-side-sub">{laneLabel(lane.kind)}</span>
                </div>
              );
            })}
          </div>
        )}

        {pages > 1 && (
          <>
            <button
              type="button"
              className="isl-page-arrow is-prev"
              aria-label={t.island.prevPage}
              onClick={() => setPage((current + pages - 1) % pages)}
            >‹</button>
            <button
              type="button"
              className="isl-page-arrow is-next"
              aria-label={t.island.nextPage}
              onClick={() => setPage((current + 1) % pages)}
            >›</button>
          </>
        )}
        {pages > 1 && (
          <div className="isl-dots-nav" role="tablist">
            {Array.from({ length: pages }, (_, i) => (
              <button
                key={i}
                type="button"
                role="tab"
                aria-selected={i === current}
                aria-label={t.island.page(i + 1)}
                className={i === current ? "is-on" : ""}
                onClick={() => setPage(i)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
