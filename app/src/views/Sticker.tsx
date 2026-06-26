import { type MouseEvent as ReactMouseEvent, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import {
  LiveSession,
  Settings,
  TerminalOpenMode,
  Usage,
  UsageWindow,
  getSettings,
  getAccount,
  refreshUsage,
} from "../api";
import { isMacPanel } from "../platform";
import { useT } from "../i18n";
import type { Dict } from "../i18n/zh";

const DAY_MS = 86_400_000;

function fmtAgo(ms: number, t: Dict): string {
  const m = Math.floor((Date.now() - ms) / 60000);
  if (m < 1) return t.time.now;
  if (m < 60) return t.time.minAgo(m);
  const h = Math.floor(m / 60);
  if (h < 24) return t.time.hourAgo(h);
  return t.time.dayAgo(Math.floor(h / 24));
}

function ArchiveIcon({ archived }: { archived: boolean }) {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect width="20" height="5" x="2" y="3" rx="1" />
      <path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8" />
      {archived ? <path d="m9 15 3-3 3 3" /> /* 还原：向上箭头 */ : <path d="M10 12h4" /> /* 归档：把手 */}
    </svg>
  );
}

function PencilIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 20h9" />
      <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" />
    </svg>
  );
}

function OpenIcon() {
  // lucide terminal：命令行 >_ 符号，明确表达「打开/跳转终端」（原 share 样图标易误读为分享）
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <polyline points="4 17 10 11 4 5" />
      <line x1="12" y1="19" x2="20" y2="19" />
    </svg>
  );
}

function NoteIcon() {
  // lucide sticky-note：折角便签纸，区别于 rename 的铅笔
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M16 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h11l5-5V5a2 2 0 0 0-2-2z" />
      <path d="M15 21v-5a1 1 0 0 1 1-1h5" />
    </svg>
  );
}

function StarIcon({ starred }: { starred: boolean }) {
  // lucide star：未星标描边、星标时填充金色以示激活
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill={starred ? "currentColor" : "none"}
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.79 21.55a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.554 10.34a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z" />
    </svg>
  );
}

function AgentMark({ provider }: { provider?: string }) {
  // Kimi 品牌「K」字标 + 右上圆点；与 Claude logomark 一样走 accent 单色（随连接状态着色）。
  if (provider === "kimi") {
    return (
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor"
        strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M7.5 4v16" />
        <path d="M7.5 12.5 15 5" />
        <path d="M7.5 11.5 16.5 20.5" />
        <circle cx="17.6" cy="4.4" r="1.8" fill="currentColor" stroke="none" />
      </svg>
    );
  }
  // 官方 Claude logomark（赤陶色 sunburst），accent 色。
  return (
    <svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z" />
    </svg>
  );
}

/** agent 提供方展示名：kimi → Kimi Code，其余 → Claude Code。 */
function agentName(provider: string, t: Dict): string {
  return provider === "kimi" ? t.sticker.agentKimiCode : t.sticker.agentClaudeCode;
}

type Item = LiveSession & { connected: boolean };
type Tab = "all" | "waiting" | "running" | "archived";

function TabIcon({ tab }: { tab: Tab }) {
  const common = { width: 11, height: 11, viewBox: "0 0 16 16", "aria-hidden": true } as const;
  switch (tab) {
    case "all": // 列表
      return (
        <svg {...common} fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
          <line x1="3" y1="4.5" x2="13" y2="4.5" />
          <line x1="3" y1="8" x2="13" y2="8" />
          <line x1="3" y1="11.5" x2="13" y2="11.5" />
        </svg>
      );
    case "waiting": // 举手（待交互）——与 macOS 菜单栏 hand.raised 同隐喻；lucide hand
      return (
        <svg {...common} viewBox="0 0 24 24" fill="none" stroke="currentColor"
          strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2" />
          <path d="M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2" />
          <path d="M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8" />
          <path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15" />
        </svg>
      );
    case "running": // 循环箭头（运行中）
      return (
        <svg {...common} viewBox="0 0 24 24" fill="none" stroke="currentColor"
          strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
          <path d="M21 3v5h-5" />
          <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
          <path d="M3 21v-5h5" />
        </svg>
      );
    case "archived": // 归档盒
      return (
        <svg {...common} fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round">
          <rect x="2.5" y="3" width="11" height="3" rx="0.5" />
          <path d="M3.5 6v6.5h9V6" />
          <line x1="6.5" y1="9" x2="9.5" y2="9" strokeLinecap="round" />
        </svg>
      );
  }
}

function PinIcon({ pinned }: { pinned: boolean }) {
  // lucide pin：未置顶描边、置顶时填充以示激活（尺寸与搜索/设置图标统一为 13）
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill={pinned ? "currentColor" : "none"}
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 17v5" />
      <path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" />
    </svg>
  );
}

function GearIcon() {
  // lucide settings：齿轮，打开设置
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}

function SearchIcon() {
  // lucide search：放大镜
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="11" cy="11" r="8" />
      <line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  );
}

function CloseIcon() {
  // lucide x：关闭搜索
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  );
}

/** 状态徽标：圆角矩形边框上流动的亮线（conic 渐变 + transform 旋转，纯 GPU 合成，
 *  拖动窗口不占主线程）+ 中心实心圆，圆内显示 Content 已用百分比。
 *  tone：running=绿（运行中），waiting=黄（待交互），pending=琥珀（待审批）。 */
function RunBadge({
  pct,
  tone = "running",
}: {
  pct: number | null;
  tone?: "running" | "waiting" | "pending";
}) {
  const t = useT();
  const what = tone === "waiting" ? t.badge.waiting : t.badge.running;
  const label = pct != null ? t.badge.full(what, pct) : what;
  return (
    <span
      className={"run-badge" + (tone === "waiting" ? " run-badge--waiting" : tone === "pending" ? " run-badge--pending" : "")}
      role="img"
      aria-label={label}
      data-tip={label}
    >
      {/* 旋转的亮段（被 .run-badge 的圆角裁剪 → 光点沿边框跑） */}
      <span className="run-sweep" />
      {/* 遮住中心黑底，只露出外圈一圈边框 */}
      <span className="run-mask" />
      {/* 中心实心圆 + 百分比 */}
      <span className="run-core">{pct != null ? `${pct}%` : ""}</span>
    </span>
  );
}

const TAB_KEY = "cc-kanban-tab";
const PIN_KEY = "cc-kanban-pinned";
const STAR_KEY = "cc-kanban-starred";
const TAB_KEYS: Tab[] = ["all", "waiting", "running", "archived"];

/** 读取已星标会话集合（按 cc_session_id 持久化，跨重启/换库稳定）。 */
function loadStarred(): Set<string> {
  try {
    const raw = JSON.parse(localStorage.getItem(STAR_KEY) ?? "[]");
    return new Set(Array.isArray(raw) ? raw.filter((x): x is string => typeof x === "string") : []);
  } catch {
    return new Set();
  }
}

function match(tab: Tab, l: Item, hideDays = 0): boolean {
  if (tab === "archived") {
    if (!l.archived) return false;
    // 归档超过 hideDays 天自动隐藏；archived_at 缺失的旧条目不隐藏。
    if (hideDays > 0 && l.archived_at && Date.now() - l.archived_at > hideDays * DAY_MS) {
      return false;
    }
    return true;
  }
  if (l.archived) return false; // 已归档的不在其它分类显示
  if (tab === "all") return true;
  if (tab === "waiting") return l.connected && (l.session.status === "waiting" || l.errored || l.pending_review != null);
  if (tab === "running") return l.connected && l.session.status === "running" && !l.errored && l.pending_review == null;
  return true;
}

function emptyCopy(tab: Tab, t: Dict): { title: string; hint: string | null } {
  switch (tab) {
    case "all": return { title: t.empty.allTitle, hint: t.empty.allHint };
    case "waiting": return { title: t.empty.waitingTitle, hint: t.empty.waitingHint };
    case "running": return { title: t.empty.runningTitle, hint: null };
    case "archived": return { title: t.empty.archivedTitle, hint: t.empty.archivedHint };
  }
}

function EmptyIcon({ tab }: { tab: Tab }) {
  const common = {
    width: 28, height: 28, viewBox: "0 0 24 24", fill: "none",
    stroke: "currentColor", strokeWidth: 1.6, strokeLinecap: "round",
    strokeLinejoin: "round", "aria-hidden": true,
  } as const;
  switch (tab) {
    case "all": // 显示器
      return (
        <svg {...common}>
          <rect width="20" height="14" x="2" y="3" rx="2" />
          <line x1="8" y1="21" x2="16" y2="21" />
          <line x1="12" y1="17" x2="12" y2="21" />
        </svg>
      );
    case "waiting": // 举手（待交互）
      return (
        <svg {...common}>
          <path d="M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2" />
          <path d="M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2" />
          <path d="M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8" />
          <path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15" />
        </svg>
      );
    case "running": // 循环箭头
      return (
        <svg {...common}>
          <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
          <path d="M21 3v5h-5" />
          <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
          <path d="M3 21v-5h5" />
        </svg>
      );
    case "archived": // 归档盒
      return (
        <svg {...common}>
          <rect width="20" height="5" x="2" y="3" rx="1" />
          <path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8" />
          <path d="M10 12h4" />
        </svg>
      );
  }
}

export function EmptyState({ tab }: { tab: Tab }) {
  const t = useT();
  const { title, hint } = emptyCopy(tab, t);
  return (
    <div className="stk-empty">
      <span className="stk-empty-icon"><EmptyIcon tab={tab} /></span>
      <div className="stk-empty-title">{title}</div>
      {hint && <div className="stk-empty-hint">{hint}</div>}
    </div>
  );
}

/** 底部用量：嵌在底栏左侧的「凹陷小屏读数」——黑屏(复刻卡片徽标 .stk-ind 材质)内每个窗口一行：
   标签 + 凹槽里的发光液柱 + 百分比；与右侧凸起按钮组成「凹陷显示屏 + 凸起按钮」的物理设备面板。 */
function usagePct(win: UsageWindow | null): number | null {
  return win ? Math.max(0, Math.min(100, win.utilization)) : null;
}
// 利用率档位 → 复用应用既有状态色(绿/黄/红)，与卡片状态点同语义；越满越红即预警。
function usageSev(pct: number): string {
  return pct >= 80 ? "is-high" : pct >= 50 ? "is-warn" : "is-ok";
}

function UsageScreen({ wins }: { wins: (UsageWindow | null)[] }) {
  const t = useT();
  // 标签多语言；顺序对应 wins = [five_hour, seven_day, seven_day_opus]
  const labels = [t.sticker.usage5h, t.sticker.usage7d, t.sticker.usageOpus];
  const rows = labels.map((label, i) => ({ label, pct: usagePct(wins[i]) })).filter(
    (r): r is { label: string; pct: number } => r.pct != null
  );
  if (!rows.length) return null;
  return (
    <div className="stk-uscreen" role="group" aria-label="用量">
      {rows.map((r) => {
        const sev = usageSev(r.pct);
        return (
          <div className="stk-urow" key={r.label}>
            <span className="stk-ulabel">{r.label}</span>
            <span className="stk-utrack">
              <i className={"stk-ufill " + sev} style={{ width: `${r.pct}%` }} />
            </span>
            <span className="stk-uval">{Math.round(r.pct)}%</span>
          </div>
        );
      })}
    </div>
  );
}

export function Sticker({ data, hasUpdate }: { data: Item[]; hasUpdate?: boolean }) {
  const t = useT();
  const [tab, setTab] = useState<Tab>(() => {
    const s = localStorage.getItem(TAB_KEY);
    return s === "waiting" || s === "running" || s === "archived" ? s : "all";
  });

  const pick = (t: Tab) => {
    setTab(t);
    localStorage.setItem(TAB_KEY, t);
    closeSearch(); // 切 tab 即退出搜索，避免 tab 高亮与过滤结果不一致
  };

  // 会话搜索：激活时底栏整条变成输入框；按标题 + 仓库名跨所有 tab 即时过滤。
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState("");
  const closeSearch = () => {
    setSearchOpen(false);
    setQuery("");
  };

  // 归档自动隐藏天数 + 打开终端方式：启动时读设置，并监听设置窗口的实时变更。
  const [hideDays, setHideDays] = useState(0);
  const [openMode, setOpenMode] = useState<TerminalOpenMode>("card");
  const [previewEnabled, setPreviewEnabled] = useState(true);
  useEffect(() => {
    const apply = (s: Settings) => {
      setHideDays(s.archive_hide_days);
      setOpenMode(s.terminal_open_mode);
      setPreviewEnabled(s.preview_enabled);
    };
    getSettings().then(apply).catch(() => {});
    // cleanup 可能先于 listen resolve 执行：用 cancelled 标记，resolve 后立即注销，防监听器泄漏。
    let cancelled = false;
    let un: (() => void) | undefined;
    try {
      listen<Settings>("settings-changed", (e) => apply(e.payload))
        .then((f) => {
          if (cancelled) f();
          else un = f;
        })
        .catch(() => {});
    } catch {
      /* 非 Tauri 环境（测试/浏览器） */
    }
    return () => {
      cancelled = true;
      try { un?.(); } catch { /* noop */ }
    };
  }, []);

  // 相对时间（fmtAgo）每分钟重算：递增计数触发轻量重渲染。
  const [, setTick] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => setTick((n) => n + 1), 60_000);
    return () => window.clearInterval(id);
  }, []);

  // 置顶开关：默认不置顶，激活后才把窗口设为 alwaysOnTop，状态持久化。
  const [pinned, setPinned] = useState<boolean>(() => localStorage.getItem(PIN_KEY) === "1");
  useEffect(() => {
    // 非 Tauri 环境（测试/浏览器）下 getCurrentWindow 会抛错，吞掉即可。
    try {
      getCurrentWindow().setAlwaysOnTop(pinned).catch(() => {});
    } catch {
      /* noop */
    }
  }, [pinned]);
  const togglePin = () => {
    setPinned((p) => {
      const next = !p;
      localStorage.setItem(PIN_KEY, next ? "1" : "0");
      return next;
    });
  };

  // 托盘「找回贴纸」：窗口居中/置顶由 App + 后端负责，这里把置顶按钮 UI 同步为已置顶。
  useEffect(() => {
    let cancelled = false;
    let un: (() => void) | undefined;
    try {
      listen("recall-sticker", () => setPinned(true))
        .then((f) => {
          if (cancelled) f();
          else un = f;
        })
        .catch(() => {});
    } catch {
      /* 非 Tauri 环境（测试/浏览器） */
    }
    return () => {
      cancelled = true;
      try { un?.(); } catch { /* noop */ }
    };
  }, []);

  // 会话星标：星标的会话永远排到列表最前（跨重启保留）。与「置顶窗口(pin)」是两回事。
  const [starred, setStarred] = useState<Set<string>>(loadStarred);
  const toggleStar = (sid: string) => {
    setStarred((prev) => {
      const next = new Set(prev);
      if (next.has(sid)) next.delete(sid);
      else next.add(sid);
      localStorage.setItem(STAR_KEY, JSON.stringify([...next]));
      return next;
    });
  };

  // 重命名：editingId 为正在编辑的会话 id，draft 为输入内容。
  const [editingId, setEditingId] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const startRename = (l: Item) => {
    setNotingId(null); // 与便签编辑互斥，同卡只开一个编辑器
    const cur = l.task_title && l.task_title !== "(未命名会话)" ? l.task_title : "";
    setDraft(cur);
    setEditingId(l.session.id);
  };
  const submitRename = (l: Item) => {
    const t = draft.trim();
    if (t && t !== l.task_title) {
      invoke("rename_session", { cwd: l.cwd, sessionId: l.session.cc_session_id, title: t }).catch(() => {});
    }
    setEditingId(null);
  };

  // 便签编辑：notingId 为正在编辑便签的会话 id，noteDraft 为输入内容。与重命名互斥（同卡只开一个）。
  const [notingId, setNotingId] = useState<number | null>(null);
  const [noteDraft, setNoteDraft] = useState("");
  const startNote = (l: Item) => {
    setEditingId(null);
    setNoteDraft(l.note ?? "");
    setNotingId(l.session.id);
  };
  const submitNote = (l: Item) => {
    if (noteDraft !== (l.note ?? "")) {
      invoke("set_session_note", { sessionId: l.session.cc_session_id, note: noteDraft }).catch(() => {});
    }
    setNotingId(null);
  };

  // 打开终端：连接中→跳转 WT 标签页；已断开未归档→新建终端 resume；归档不开。
  const buttonMode = openMode === "button";
  const canOpen = (l: Item) => l.connected || !l.archived;
  const openTerminal = (l: Item) => {
    if (l.connected) {
      if (l.pid)
        invoke("focus_session", {
          pid: l.pid,
          title: l.task_title,
          cwd: l.cwd,
          sessionId: l.session.cc_session_id,
        }).catch(() => {});
    } else if (!l.archived) {
      invoke("resume_session", { cwd: l.cwd, sessionId: l.session.cc_session_id }).catch(() => {});
    }
  };

  // 先按当前 tab 过滤，再排序：星标恒在最前；「待交互」标签内按等待最久优先（先处理被晾最久的）；
  // 其它标签保留服务端顺序（连接中优先 → 最近活跃）。Array.sort 稳定，组内次序不乱。
  // useMemo 缓存：编辑便签/重命名时每次按键都会重渲染，不必每次重跑 filter+sort。
  const isStarred = (l: Item) => starred.has(l.session.cc_session_id);
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    // 搜索激活：按标题 + 仓库名过滤，按星标优先排序。归档 tab 搜归档、其余 tab 搜活跃
    // (否则站在归档 tab 怎么搜都搜不到归档会话)。
    if (q) {
      const wantArchived = tab === "archived";
      return data
        .filter(
          (l) =>
            (wantArchived ? l.archived : !l.archived) &&
            ((l.task_title ?? "").toLowerCase().includes(q) ||
              (l.project_name ?? "").toLowerCase().includes(q))
        )
        .sort(
          (a, b) =>
            Number(starred.has(b.session.cc_session_id)) - Number(starred.has(a.session.cc_session_id))
        );
    }
    return data
      .filter((l) => match(tab, l, hideDays))
      .sort((a, b) => {
        const star =
          Number(starred.has(b.session.cc_session_id)) -
          Number(starred.has(a.session.cc_session_id));
        if (star !== 0) return star;
        if (tab === "waiting") {
          const ap = a.pending_review != null ? 0 : 1;
          const bp = b.pending_review != null ? 0 : 1;
          if (ap !== bp) return ap - bp; // pending 整组置顶
          return a.session.last_event_at - b.session.last_event_at; // 组内等最久优先
        }
        return 0;
      });
  }, [data, tab, hideDays, starred, query]);

  // 各标签角标计数：同样随每次按键重渲染，缓存避免对 4 个标签各跑一遍全量 filter。
  const counts = useMemo(() => {
    const c = {} as Record<Tab, number>;
    for (const k of TAB_KEYS) c[k] = data.filter((l) => match(k, l, hideDays)).length;
    return c;
  }, [data, hideDays]);

  // 自绘 overlay 滚动条：原生滚动条全程隐藏(不占布局→无抖动)，这里按滚动位置算出
  // thumb 的高度/位置，浮在内容右侧。null = 内容未超出、不需要滚动条。
  const scrollRef = useRef<HTMLDivElement>(null);
  const [sb, setSb] = useState<{ top: number; height: number } | null>(null);
  const [sbDrag, setSbDrag] = useState(false);
  // 滚动边缘淡出：仅当该方向确有被遮内容时才淡(滚到顶/底则对应边不淡，首/末卡保持清晰)。
  const [edge, setEdge] = useState({ top: false, bottom: false });

  // 底部用量：首屏用 getAccount 缓存秒显(仅在还没有联网值时填充，避免缓存晚到覆盖更新的联网值)，
  // 联网用 refreshUsage 为准、每 5 分钟刷一次。
  const [usage, setUsage] = useState<Usage | null>(null);
  useEffect(() => {
    let cancelled = false;
    getAccount()
      .then((p) => { if (!cancelled && p.usage) setUsage((cur) => cur ?? p.usage); })
      .catch(() => {});
    const refresh = () => {
      refreshUsage()
        .then((u) => { if (!cancelled) setUsage(u); })
        .catch(() => {}); // 第三方账号 USAGE_UNSUPPORTED 等：保持无用量，不显示用量条
    };
    refresh();
    const id = window.setInterval(refresh, 5 * 60_000);
    return () => { cancelled = true; window.clearInterval(id); };
  }, []);
  const syncSb = () => {
    const el = scrollRef.current;
    if (!el) return;
    const { scrollTop, scrollHeight, clientHeight, offsetTop } = el;
    if (scrollHeight <= clientHeight + 1) {
      setSb(null);
      setEdge((p) => (p.top || p.bottom ? { top: false, bottom: false } : p));
      return;
    }
    const canUp = scrollTop > 1;
    const canDown = scrollTop + clientHeight < scrollHeight - 1;
    setEdge((p) => (p.top === canUp && p.bottom === canDown ? p : { top: canUp, bottom: canDown }));
    const thumbH = Math.max(28, (clientHeight * clientHeight) / scrollHeight);
    const top = offsetTop + (clientHeight - thumbH) * (scrollTop / (scrollHeight - clientHeight));
    setSb({ top, height: thumbH });
  };
  // 列表内容(shown)或可视尺寸变化时重算 thumb。
  useEffect(() => {
    syncSb();
    const el = scrollRef.current;
    // 测试/非浏览器环境无 ResizeObserver：仅同步一次即可。
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(syncSb);
    ro.observe(el);
    return () => ro.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shown.length]);

  // 拖拽 thumb 滚动列表。
  const onSbDown = (e: ReactMouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const el = scrollRef.current;
    if (!el) return;
    setSbDrag(true);
    const startY = e.clientY;
    const startScroll = el.scrollTop;
    const { scrollHeight, clientHeight } = el;
    const thumbH = Math.max(28, (clientHeight * clientHeight) / scrollHeight);
    const move = (ev: MouseEvent) => {
      const ratio = (ev.clientY - startY) / (clientHeight - thumbH);
      el.scrollTop = startScroll + ratio * (scrollHeight - clientHeight);
    };
    const up = () => {
      setSbDrag(false);
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  return (
    <div className="sticker">
      {!isMacPanel() && <div className="drag" data-tauri-drag-region />}
      <div className="tabs">
        <div className="tabseg">
          {/* 选中态立体滑块：切换时平滑滑到目标 tab(translateX 动画) */}
          <span
            className="tabseg-slider"
            style={{ transform: `translateX(${TAB_KEYS.indexOf(tab) * 100}%)` }}
          />
          {TAB_KEYS.map((k) => {
            const n = counts[k];
            return (
              <span
                key={k}
                className={"stab " + (tab === k ? "stab-on" : "")}
                onClick={() => pick(k)}
              >
                <TabIcon tab={k} />
                {t.tabs[k]}
                <span className="stab-n">{n}</span>
              </span>
            );
          })}
        </div>
      </div>
      <div
        className={"stk-scroll" + (edge.top ? " fade-top" : "") + (edge.bottom ? " fade-bottom" : "")}
        ref={scrollRef}
        onScroll={syncSb}
      >
        {shown.length === 0 ? (
          <EmptyState tab={tab} />
        ) : (
          shown.map((l) => {
            const unnamed = !l.task_title || l.task_title === "(未命名会话)";
            const title = unnamed ? t.sticker.waitingFirstInput : l.task_title;
            // AI 活动行显示「最近一条 AI 正文」(last_ai_text，回退 transcript preview)；出错优先显示错误标签；
            // previewEnabled（对话预览开关）关闭则不显示 AI 正文（仅保留错误）；用户行同受该开关门控。
            const sub = l.errored && l.error_label
              ? t.errorLabels[l.error_label] ?? l.error_label
              : previewEnabled && (l.last_ai_text ?? l.preview)
              ? (l.last_ai_text ?? l.preview)
              : null;
            const subTitle = l.errored ? l.error_raw ?? undefined : sub ?? undefined;
            const indicator = !l.connected ? (
              <span className="ring-stop" data-tip={t.sticker.stopped} />
            ) : l.errored ? (
              <span className="needs-error" data-tip={l.error_raw ?? t.sticker.sessionError} />
            ) : l.pending_review ? (
              <RunBadge pct={l.context_pct} tone="pending" />
            ) : l.session.status === "running" ? (
              <RunBadge pct={l.context_pct} />
            ) : l.session.status === "waiting" ? (
              <RunBadge pct={l.context_pct} tone="waiting" />
            ) : (
              <span className="sdot sdot-on" data-tip={t.sticker.online} />
            );
            return (
              <div
                className={"stk-card" + (isStarred(l) ? " is-star" : "")}
                key={l.session.id}
                onClick={() => {
                  // 编辑态(重命名/便签)下，点击卡片仅用于关闭编辑器（失焦），绝不导航开终端。
                  // 注：编辑输入框已 stopPropagation，这里只会被「点击卡片空白处」触发。
                  if (editingId !== null || notingId !== null) {
                    setEditingId(null);
                    setNotingId(null);
                    return;
                  }
                  // 按钮模式：点击卡片不开终端，改由卡片上的打开按钮触发。
                  if (buttonMode) return;
                  openTerminal(l);
                }}
                style={{ cursor: !buttonMode && canOpen(l) ? "pointer" : "default" }}
                data-tip={buttonMode ? "" : l.connected ? t.sticker.jumpToTerminal : l.archived ? "" : t.sticker.resumeInTerminal}
              >
                <div className="stk-top">
                  <span className="stk-ind">{indicator}</span>
                  <div className="stk-top-body">
                    <div className="stk-line1">
                      {editingId === l.session.id ? (
                        <div className="stk-edit-row" onClick={(e) => e.stopPropagation()}>
                          <input
                            className="stk-edit"
                            autoFocus
                            value={draft}
                            placeholder={t.sticker.renamePlaceholder}
                            onChange={(e) => setDraft(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter") submitRename(l);
                              else if (e.key === "Escape") setEditingId(null);
                            }}
                          />
                          <button
                            type="button"
                            className="stk-btn-save"
                            onMouseDown={(e) => e.preventDefault()}
                            onClick={() => submitRename(l)}
                          >{t.sticker.noteSave}</button>
                          <button
                            type="button"
                            className="stk-btn-cancel"
                            onMouseDown={(e) => e.preventDefault()}
                            onClick={() => setEditingId(null)}
                          >{t.sticker.noteCancel}</button>
                        </div>
                      ) : (
                        <>
                          <span className="stk-title">{title}</span>
                          {l.pending_review && (
                            <span className={"pending-pill pending-" + l.pending_review}>
                              {t.pending[l.pending_review]}
                            </span>
                          )}
                          <span className="stk-time">{fmtAgo(l.session.last_event_at, t)}</span>
                          {/* 操作按钮默认收起，hover 卡片才浮现，避免每张卡 4 个图标拥挤。
                              星标态由卡片金边、便签由便签块表达，静止时藏图标不丢信息。 */}
                          <span className="stk-actions">
                            <span
                              className={"stk-star" + (isStarred(l) ? " stk-star-on" : "")}
                              data-tip={isStarred(l) ? t.sticker.unstar : t.sticker.star}
                              aria-label={isStarred(l) ? t.sticker.unstar : t.sticker.star}
                              onClick={(e) => { e.stopPropagation(); toggleStar(l.session.cc_session_id); }}
                            ><StarIcon starred={isStarred(l)} /></span>
                            <span
                              className={"stk-noteb" + (l.note ? " stk-noteb-on" : "")}
                              data-tip={l.note ? t.sticker.noteEdit : t.sticker.noteAdd}
                              aria-label={l.note ? t.sticker.noteEdit : t.sticker.noteAdd}
                              onClick={(e) => { e.stopPropagation(); startNote(l); }}
                            ><NoteIcon /></span>
                            <span
                              className="stk-rename"
                              data-tip={t.sticker.renameTitle}
                              aria-label={t.sticker.renameTitle}
                              onClick={(e) => { e.stopPropagation(); startRename(l); }}
                            ><PencilIcon /></span>
                            <span
                              className="stk-arch"
                              data-tip={l.archived ? t.sticker.unarchive : t.sticker.archive}
                              aria-label={l.archived ? t.sticker.unarchive : t.sticker.archive}
                              onClick={(e) => { e.stopPropagation(); invoke("set_archived", { sessionId: l.session.id, archived: !l.archived }).catch(() => {}); }}
                            ><ArchiveIcon archived={l.archived} /></span>
                          </span>
                        </>
                      )}
                    </div>
                    {editingId === l.session.id && l.connected && (
                      <div className="stk-edit-hint">{t.sticker.renameHint}</div>
                    )}
                    <div className="stk-line2">
                      <span
                        className={"stk-agent" + (l.connected ? "" : " stk-agent-off")}
                        data-tip={agentName(l.provider, t)}
                        aria-label={agentName(l.provider, t)}
                      >
                        <AgentMark provider={l.provider} />
                      </span>
                      <span className="stk-repo" data-tip={l.project_name}>{l.project_name.split("/").pop()}</span>
                      {l.model && <span className="stk-model">{l.model}</span>}
                    </div>
                  </div>
                </div>
                {notingId === l.session.id ? (
                  <div className="stk-note-editbox" onClick={(e) => e.stopPropagation()}>
                    <input
                      className="stk-note-edit"
                      autoFocus
                      value={noteDraft}
                      placeholder={t.sticker.notePlaceholder}
                      onChange={(e) => setNoteDraft(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") submitNote(l);
                        else if (e.key === "Escape") setNotingId(null);
                      }}
                    />
                    <div className="stk-note-actions">
                      {/* mousedown preventDefault：点按钮不抢走输入框焦点，避免触发其它失焦逻辑 */}
                      <button
                        type="button"
                        className="stk-btn-save"
                        onMouseDown={(e) => e.preventDefault()}
                        onClick={() => submitNote(l)}
                      >{t.sticker.noteSave}</button>
                      <button
                        type="button"
                        className="stk-btn-cancel"
                        onMouseDown={(e) => e.preventDefault()}
                        onClick={() => setNotingId(null)}
                      >{t.sticker.noteCancel}</button>
                    </div>
                  </div>
                ) : l.note ? (
                  <div
                    className="stk-note"
                    data-tip={t.sticker.noteEdit}
                    onClick={(e) => { e.stopPropagation(); startNote(l); }}
                  >
                    <span className="stk-note-icon"><NoteIcon /></span>
                    <span className="stk-note-txt">{l.note}</span>
                  </div>
                ) : null}
                {previewEnabled && l.last_user_text && (
                  <div className="stk-subrow stk-userrow">
                    <span className="stk-msg-tag">{t.sticker.youPrefix}</span>
                    <span className="stk-sub" data-tip={l.last_user_text}>{l.last_user_text}</span>
                  </div>
                )}
                {(sub || (buttonMode && canOpen(l))) && (
                  <div className="stk-subrow">
                    {/* 活动行：最近一条 AI 正文(或错误标签)，单行截断；title 给完整文本，hover 原生提示可读全文 */}
                    {sub && !l.errored && <span className="stk-msg-tag is-ai">{t.sticker.aiPrefix}</span>}
                    {sub && <span className={"stk-sub" + (l.errored ? " stk-sub-err" : "")} data-tip={subTitle}>{sub}</span>}
                    {/* 按钮模式：打开终端按钮内联在该行末尾，不突兀 */}
                    {buttonMode && canOpen(l) && (
                      <button
                        type="button"
                        className="stk-open"
                        data-tip={l.connected ? t.sticker.jumpToTerminal : t.sticker.resumeInTerminal}
                        aria-label={l.connected ? t.sticker.jumpToTerminal : t.sticker.resumeInTerminal}
                        onClick={(e) => { e.stopPropagation(); openTerminal(l); }}
                      ><OpenIcon /></button>
                    )}
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>
      {sb && (
        <div
          className={"stk-sb" + (sbDrag ? " is-drag" : "")}
          style={{ top: sb.top, height: sb.height }}
          onMouseDown={onSbDown}
        />
      )}
      {/* 底栏:用量(左) + 搜索/设置/固定(右)聚为一处;搜索激活时整条变输入框。 */}
      <div className="stk-bar">
        {searchOpen ? (
          <div className="stk-search">
            <span className="stk-search-ic">
              <SearchIcon />
            </span>
            <input
              className="stk-search-in"
              autoFocus
              value={query}
              placeholder={t.sticker.searchPlaceholder}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") closeSearch();
              }}
            />
            <span className="stk-act stk-search-x" data-tip={t.sticker.searchClose} aria-label={t.sticker.searchClose} onClick={closeSearch}>
              <CloseIcon />
            </span>
          </div>
        ) : (
          <>
            {usage &&
              (usage.five_hour || usage.seven_day || usage.seven_day_opus) && (
                <UsageScreen
                  wins={[usage.five_hour, usage.seven_day, usage.seven_day_opus]}
                />
              )}
            <div className="stk-bar-actions">
              <span className="stk-act" data-tip={t.sticker.search} aria-label={t.sticker.search} onClick={() => setSearchOpen(true)}>
                <SearchIcon />
              </span>
              <span
                className="stk-act"
                data-tip={hasUpdate ? t.sticker.updateAvailable : t.sticker.openSettings}
                aria-label={hasUpdate ? t.sticker.updateAvailable : t.sticker.openSettings}
                onClick={() => invoke("open_settings").catch(() => {})}
              >
                <GearIcon />
                {hasUpdate && <span className="stk-dot" aria-hidden="true" />}
              </span>
              {!isMacPanel() && (
                <span
                  className={"stk-act " + (pinned ? "stk-pin-on" : "")}
                  data-tip={pinned ? t.sticker.pinOn : t.sticker.pinOff}
                  aria-label={pinned ? t.sticker.pinOn : t.sticker.pinOff}
                  onClick={togglePin}
                >
                  <PinIcon pinned={pinned} />
                </span>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
