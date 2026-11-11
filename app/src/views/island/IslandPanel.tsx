// 灵动岛展开后的黑色面板（照用户参考图：标题 + 日期时间、分页圆点、环形进度）。
// 前几页：会话，和侧栏同一个顺序每页 3 个（不放额度，用户要求会话页只看会话）；最后一页（有额度数据才有）：
// 各 agent 的额度。搜索、菜单、归档这些在对话窗口侧栏里，单击托盘图标打开。
import { useLayoutEffect, useMemo, useRef, useState } from "react";
import type { ProviderUsage, UsageLane } from "../../api";
import { agentAssets, tintStyle } from "../../providers";
import { useT } from "../../i18n";
import { cardTone, fmtAgo } from "../sticker/helpers";
import type { Item } from "../sticker/types";
import { groupTone, sessionTitle, useSidebarOrder } from "../Island";
import { AgentRing, Ring, useNow, usedColor, usedOf } from "./islandParts";
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

/// 叠环里每一圈的颜色，按泳道排序后的位置取（最外圈第一个）。仿 Apple 健康的三环：每圈一个
/// 固定色相，靠颜色把图例和圈对上。不再按用量变色：几圈都是绿的就分不清谁是谁。
/// 快用完改由图例里的数字变色提示。避开琥珀和红，那两个在 Meowo 里是「要处理 / 出错」。
const LANE_COLORS = ["var(--isl-ok)", "#64d2ff", "#bf5af2", "#5e5ce6"];

/// 叠环：一家一个，每档额度一圈，外大内小。用户要求（2026-11-05）：此前每档一个独立的环，
/// 一家三档就横排三个环，面板挤、标签被截断（「Fable · 每周」只露一半）。
const STACK_SIZE = 64;
const STACK_STROKE = 6;
const STACK_GAP = 1.5;

function StackedRings({ lanes }: { lanes: UsageLane[] }) {
  return (
    <span className="isl-usage-stack" style={{ width: STACK_SIZE, height: STACK_SIZE }}>
      {lanes.map((lane, i) => {
        const size = STACK_SIZE - i * 2 * (STACK_STROKE + STACK_GAP);
        if (size <= STACK_STROKE * 2) return null;
        const color = LANE_COLORS[i % LANE_COLORS.length];
        return (
          <Ring
            key={`${lane.kind}-${lane.label ?? i}`}
            size={size}
            stroke={STACK_STROKE}
            value={usedOf(lane.used_pct as number) / 100}
            color={color}
            track={`color-mix(in srgb, ${color} 22%, #000)`}
          />
        );
      })}
    </span>
  );
}

function providerLabel(provider: string): string {
  return provider ? provider[0].toUpperCase() + provider.slice(1) : provider;
}

/// 加号用 SVG（与对话窗工作区头部的新建按钮同一个形状）：全角「＋」拼在文案里会被读屏念出来，
/// 按钮名也就不是「新建会话」了（审查 2026-11-09）。
const PLUS = <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>;

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
  // 和侧栏、收起的胶囊同一个顺序（用户 2026-11-06：要一致）。原来按要紧程度排，
  // 待处理的会话会在列表里跳来跳去，和侧栏对不上。
  const ordered = useSidebarOrder(sessions);
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
      default: return t.island.usageOther;
    }
  };

  return (
    <div className="isl-stage">
      {/* region + 名字：岛窗口里只有这一块，读屏能按地标跳到它（axe region，embed-island 实扫）。 */}
      <div
        ref={panelRef}
        className={`isl-notch isl-panel${pages > 1 ? " is-paged" : ""}`}
        role="region"
        aria-label={`Meowo ${onUsagePage ? t.island.panelUsage : t.island.panelSessions}`}
      >
        <div className="isl-panel-head" onPointerDown={onPointerDownDrag}>
          <span className="isl-panel-title">
            Meowo <span className="isl-accent">{onUsagePage ? t.island.panelUsage : t.island.panelSessions}</span>
          </span>
          {/* 原来「全部」的位置放新建：开新会话不用再绕去托盘或完整列表。 */}
          {onNewSession && (
            <button
              type="button"
              className="isl-btn is-ghost isl-new"
              onPointerDown={(event) => event.stopPropagation()}
              onClick={onNewSession}
            >{PLUS}{t.sticker.newSession}</button>
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
            <span className="isl-panel-meta-date">{now.toLocaleDateString(t.locale, { month: "long", day: "numeric", weekday: "short" })}</span>
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
                    <button type="button" className="isl-btn is-primary isl-new" onClick={onNewSession}>{PLUS}{t.sticker.newSession}</button>
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
                    // 空格也算（i8）：role=button 的约定，原来只认 Enter。
                    onKeyDown={(event) => {
                      if (event.key !== "Enter" && event.key !== " ") return;
                      event.preventDefault();
                      onOpenSession(item.session.id);
                    }}
                  >
                    <AgentRing item={item} size={30} />
                    <span className="isl-row-text">
                      <span className="isl-row-title" title={sessionTitle(item, t.sticker.waitingFirstInput)}>{sessionTitle(item, t.sticker.waitingFirstInput)}</span>
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
                  <StackedRings lanes={shownLanes(usage[provider])} />
                  {/* 图例：色点对应上面的圈，后面是这档的名字和已用量。快用完时数字变琥珀 / 红。 */}
                  <div className="isl-usage-lanes">
                    {shownLanes(usage[provider]).map((lane, i) => {
                      const used = usedOf(lane.used_pct as number);
                      return (
                        <div key={`${lane.kind}-${lane.label ?? i}`} className="isl-usage-cell" title={laneName(lane)}>
                          <i className="isl-usage-key" style={{ background: LANE_COLORS[i % LANE_COLORS.length] }} />
                          <span className="isl-usage-name">{laneName(lane)}</span>
                          <span className="isl-usage-left" style={used >= 60 ? { color: usedColor(used) } : undefined}>
                            {used}<small>%</small>
                          </span>
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
          // 分页圆点是普通按钮组、当前页标 aria-current（i8）。不用 tablist：这里是翻页，没有对应的
          // tabpanel，冒充 tablist 却不支持方向键反而让读屏用户以为键盘坏了；按钮组每个点各占一个 Tab
          // 停靠，最多 4 页，代价可接受，也不必等 a11y 车道的 useTablist。
          <div className="isl-dots-nav" role="group" aria-label={t.island.pages}>
            {Array.from({ length: pages }, (_, i) => (
              <button
                key={i}
                type="button"
                aria-current={i === current ? "page" : undefined}
                aria-label={t.island.page(i + 1)}
                // 会话页的圆点按该页最急状态上色（二期 #3），用量页没有状态。
                className={[
                  i === current ? "is-on" : "",
                  i < sessionPages ? `is-${groupTone(ordered.slice(i * MAX_ROWS, i * MAX_ROWS + MAX_ROWS))}` : "",
                ].filter(Boolean).join(" ")}
                onClick={() => setPage(i)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
