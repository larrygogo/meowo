// 灵动岛展开后的黑色面板（照用户参考图：标题 + 日期时间、分页圆点、环形进度）。
// 前几页：会话，按要紧程度每页 3 个（不放额度，用户要求会话页只看会话）；最后一页（有额度数据才有）：
// 各 agent 的额度。搜索、菜单、归档这些在对话窗口侧栏里，单击托盘图标打开。
import { useLayoutEffect, useMemo, useRef, useState } from "react";
import type { ProviderUsage, UsageLane } from "../../api";
import { agentAssets, tintStyle } from "../../providers";
import { useT } from "../../i18n";
import { cardTone, fmtAgo } from "../sticker/helpers";
import type { Item } from "../sticker/types";
import { byUrgency, sessionTitle } from "../Island";
import { AgentRing, Ring, remainingColor, remainingOf, useNow } from "./islandParts";
import { primaryLane } from "./useIslandUsage";
import { ChatIcon, GearIcon } from "../sticker/icons";

/** 极简模式图标：一条贴顶的细线。 */
function StripIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
      <rect x="3" y="4" width="18" height="16" rx="3" />
      <path d="M8 8h8" strokeWidth="3" />
    </svg>
  );
}

/// 每页列几个会话。面板去掉「全部」按钮后，多于这个数的会话翻页看（左右箭头 / 圆点）。
const MAX_ROWS = 3;

/// 用量页的泳道顺序：短窗口在前，再是周/7 天，再是模型专属周限，其余殿后。
const LANE_ORDER = ["five_hour", "seven_day", "weekly", "model_weekly", "opus", "balance", "other"];

function shownLanes(usage: ProviderUsage | undefined): UsageLane[] {
  if (!usage) return [];
  return usage.lanes
    .filter((lane) => lane.used_pct != null)
    .sort((a, b) => LANE_ORDER.indexOf(a.kind) - LANE_ORDER.indexOf(b.kind));
}

function providerLabel(provider: string): string {
  return provider ? provider[0].toUpperCase() + provider.slice(1) : provider;
}

export function IslandPanel({
  sessions,
  usage,
  quotaProviders,
  onOpenSession,
  onMeasure,
  onPointerDownDrag,
  initialPage = 0,
  onNewSession,
  onOpenChat,
  onOpenSettings,
  strip = false,
  onToggleStrip,
}: {
  sessions: readonly Item[];
  usage: Record<string, ProviderUsage>;
  quotaProviders: readonly string[];
  onOpenSession: (sessionId: number) => void;
  /** 面板实测高度（逻辑像素），App 据此让窗口贴合。 */
  onMeasure: (height: number) => void;
  onPointerDownDrag: (event: React.PointerEvent<HTMLElement>) => void;
  /** 打开时停在第几页（0 起）。 */
  initialPage?: number;
  /** 新建会话（标题旁的「＋」与空态按钮）。 */
  onNewSession?: () => void;
  /** 打开对话窗口。轻量模式（对话功能关闭）时不传，按钮不出现。 */
  onOpenChat?: () => void;
  /** 打开设置。 */
  onOpenSettings?: () => void;
  /** 当前是否极简模式（收起成细条）。 */
  strip?: boolean;
  /** 切换极简模式。 */
  onToggleStrip?: () => void;
}) {
  const t = useT();
  const now = useNow();
  const panelRef = useRef<HTMLDivElement>(null);
  const [page, setPage] = useState(initialPage);
  const ordered = useMemo(() => byUrgency(sessions), [sessions]);
  // 会话分页：每页 MAX_ROWS 个，至少一页（空态也占一页）；用量页排在最后。
  const sessionPages = Math.max(1, Math.ceil(ordered.length / MAX_ROWS));
  // 用量页按「显示配额」的设置顺序排，设置里没有的殿后。
  const usageProviders = [...quotaProviders, ...Object.keys(usage)]
    .filter((p, i, all) => all.indexOf(p) === i && primaryLane(usage[p]) != null);
  const pages = sessionPages + (usageProviders.length > 0 ? 1 : 0);
  const current = Math.min(page, pages - 1);
  const onUsagePage = current >= sessionPages;
  const rows = ordered.slice(current * MAX_ROWS, current * MAX_ROWS + MAX_ROWS);

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

  // 泳道全名与贴纸底栏同一套叫法（t.account.lane*），两处说法一致。
  const laneName = (lane: UsageLane) => {
    switch (lane.kind) {
      case "five_hour": return t.account.laneFiveHour;
      case "seven_day": return t.account.laneSevenDay;
      case "weekly": return t.account.laneWeekly;
      case "opus": return t.account.laneOpus;
      case "model_weekly": return lane.label ? t.account.laneModelWeekly(lane.label) : t.account.laneWeekly;
      case "balance": return t.account.laneBalance;
      default: return t.island.usageLane.other;
    }
  };

  return (
    <div className="isl-stage">
      <div ref={panelRef} className={`isl-notch isl-panel${pages > 1 ? " is-paged" : ""}`}>
        <div className="isl-panel-head" onPointerDown={onPointerDownDrag}>
          <span className="isl-panel-title">
            Meowo <span className="isl-accent">{onUsagePage ? t.island.panelUsage : t.island.panelSessions}</span>
          </span>
          {/* 原来「全部」的位置放新建：开新会话不用再绕去托盘或完整列表。 */}
          {onNewSession && (
            <button
              type="button"
              className="isl-btn is-ghost"
              onPointerDown={(event) => event.stopPropagation()}
              onClick={onNewSession}
            >＋ {t.sticker.newSession}</button>
          )}
          {/* 托盘菜单去掉「打开会话列表」后，对话窗与设置在岛上也要有入口（用户 2026-11-05 要求）。 */}
          {onOpenChat && (
            <button
              type="button"
              className="isl-btn is-ghost is-icon"
              title={t.sticker.openChatWindow}
              aria-label={t.sticker.openChatWindow}
              onPointerDown={(event) => event.stopPropagation()}
              onClick={onOpenChat}
            ><ChatIcon /></button>
          )}
          {onOpenSettings && (
            <button
              type="button"
              className="isl-btn is-ghost is-icon"
              title={t.sticker.openSettings}
              aria-label={t.sticker.openSettings}
              onPointerDown={(event) => event.stopPropagation()}
              onClick={onOpenSettings}
            ><GearIcon /></button>
          )}
          {/* 极简模式开关放在标题栏空位上（用户 2026-11-05 指定位置）。 */}
          {onToggleStrip && (
            <button
              type="button"
              className={`isl-btn is-ghost is-icon${strip ? " is-on" : ""}`}
              title={strip ? t.island.stripOff : t.island.stripOn}
              aria-label={strip ? t.island.stripOff : t.island.stripOn}
              aria-pressed={strip}
              onPointerDown={(event) => event.stopPropagation()}
              onClick={onToggleStrip}
            ><StripIcon /></button>
          )}
          <span className="isl-panel-meta">
            {now.toLocaleDateString(t.locale, { month: "long", day: "numeric", weekday: "short" })}
            <b>{now.toLocaleTimeString(t.locale, { hour: "2-digit", minute: "2-digit", hour12: false })}</b>
          </span>
        </div>

        {!onUsagePage ? (
          <div className="isl-panel-body">
            <div className="isl-rows">
              {rows.length === 0 && (
                <div className="isl-empty">
                  <span>{t.island.idle}</span>
                  {/* 标题栏已有「新建会话」，空态的主按钮改成打开对话窗口（用户 2026-11-05 要求）；
                      轻量模式没有对话窗口，才退回新建。 */}
                  {onOpenChat ? (
                    <button type="button" className="isl-btn is-primary" onClick={onOpenChat}>{t.sticker.openChatWindow}</button>
                  ) : onNewSession && (
                    <button type="button" className="isl-btn is-primary" onClick={onNewSession}>＋ {t.sticker.newSession}</button>
                  )}
                </div>
              )}
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
          </div>
        ) : (
          <div className="isl-panel-body is-usage">
            {/* 每家一栏、每档额度一个环：此前每家只取一个主泳道，5 小时与本周、模型周限只露一个，
                用户问「为什么只有一个用量」。 */}
            {usageProviders.map((provider) => {
              const { Icon } = agentAssets(provider);
              return (
                <div key={provider} className="isl-usage-group">
                  <span className="isl-usage-head">
                    <span className="isl-usage-head-logo" style={tintStyle(provider)}><Icon /></span>
                    {providerLabel(provider)}
                  </span>
                  <div className="isl-usage-lanes">
                    {shownLanes(usage[provider]).map((lane, i) => {
                      const left = remainingOf(lane.used_pct as number);
                      return (
                        <div key={`${lane.kind}-${lane.label ?? i}`} className="isl-usage-cell">
                          <Ring size={50} stroke={4.5} value={left / 100} color={remainingColor(left)} track="var(--isl-track)">
                            <span className="isl-mid-val">{left}<small>%</small></span>
                          </Ring>
                          <span className="isl-side-sub">{laneName(lane)}</span>
                        </div>
                      );
                    })}
                  </div>
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
