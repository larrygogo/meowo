import { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { emit } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getSettings, setSettings, availableTerminals, type Settings, type ThemeMode, type ResumeTerminal } from "../api";
import { getAccount, refreshUsage, type AccountPayload, type Usage, type DailyEntry } from "../api";
import { useUpdate, type UpdateStatus } from "../useUpdate";

const HIDE_OPTIONS = [
  { value: 0, label: "永不" },
  { value: 1, label: "1 天" },
  { value: 7, label: "7 天" },
  { value: 30, label: "30 天" },
];

const REPO = "github.com/larrygogo/cc-kanban";
const REPO_URL = "https://github.com/larrygogo/cc-kanban";
const openExt = (url: string) => invoke("open_url", { url }).catch(() => {});

type Section = "general" | "appearance" | "account" | "about";

const SETTINGS_DEFAULTS: Settings = {
  archive_hide_days: 0,
  notifications_enabled: true,
  theme: "dark",
  opacity: 94,
  ui_scale: 100,
  resume_terminal: "terminal",
};

// 打开未连接会话用的终端：按平台给不同选项。WKWebView 的 UA 含 "Mac"/"Win"，与 main.tsx 同步判定一致。
const IS_MAC = typeof navigator !== "undefined" && /Mac/i.test(navigator.userAgent);
const IS_WIN = typeof navigator !== "undefined" && /Win/i.test(navigator.userAgent);
const RESUME_TERM_OPTIONS_MAC: { value: ResumeTerminal; label: string }[] = [
  { value: "terminal", label: "Terminal" },
  { value: "iterm", label: "iTerm2" },
];
const RESUME_TERM_OPTIONS_WIN: { value: ResumeTerminal; label: string }[] = [
  { value: "wt", label: "Windows Terminal" },
  { value: "powershell", label: "PowerShell" },
  { value: "cmd", label: "命令提示符" },
];

// 设置读写：本地保留完整对象，每次只 patch 改动字段后整对象写回（后端 set_settings 收整对象，
// 漏字段会被 serde 默认值覆盖 → 必须整对象提交）。写失败则回读后端保持一致。
function useSettingsState() {
  const [settings, setSettingsState] = useState<Settings | null>(null);
  const ref = useRef<Settings>(SETTINGS_DEFAULTS);
  useEffect(() => {
    getSettings()
      .then((s) => {
        ref.current = s;
        setSettingsState(s);
      })
      .catch(() => {});
  }, []);
  const patch = (p: Partial<Settings>) => {
    // 真实设置尚未回填（首帧到 get_settings resolve 之间）时忽略：避免用默认值整对象覆盖磁盘。
    if (settings === null) return;
    const next = { ...ref.current, ...p };
    ref.current = next;
    setSettingsState(next);
    setSettings(next).catch(() => {
      getSettings()
        .then((s) => {
          ref.current = s;
          setSettingsState(s);
        })
        .catch(() => {});
    });
  };
  return [settings, patch] as const;
}

function IconGear() {
  return (
    <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}
function IconInfo() {
  return (
    <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="9" />
      <line x1="12" y1="11" x2="12" y2="16" />
      <line x1="12" y1="8" x2="12" y2="8" />
    </svg>
  );
}

function IconUser() {
  return (
    <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="8" r="4" />
      <path d="M4 21v-1a6 6 0 0 1 6-6h4a6 6 0 0 1 6 6v1" />
    </svg>
  );
}

// 半填充对比圆：外观/主题的经典图标。
function IconAppearance() {
  return (
    <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="9" />
      <path d="M12 3a9 9 0 0 0 0 18z" fill="currentColor" stroke="none" />
    </svg>
  );
}

function RefreshIcon({ spinning }: { spinning?: boolean }) {
  return (
    <svg className={spinning ? "spin" : undefined} width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M3 12a9 9 0 0 1 15-6.7L21 8" />
      <path d="M21 3v5h-5" />
      <path d="M21 12a9 9 0 0 1-15 6.7L3 16" />
      <path d="M3 21v-5h5" />
    </svg>
  );
}

function fmtResetIn(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const now = Date.now();
  const diffMs = t - now;
  if (diffMs <= 0) return "即将重置";
  // 按自然日差判断：今天显示剩余小时/分钟，跨天则用相对词/日期并精确到钟点。
  const startOf = (ms: number) => {
    const d = new Date(ms);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  };
  const dayDiff = Math.round((startOf(t) - startOf(now)) / 86_400_000);
  if (dayDiff <= 0) {
    const min = Math.round(diffMs / 60000);
    if (min < 60) return `${min} 分钟后重置`;
    const h = Math.floor(min / 60);
    const m = min % 60;
    return m > 0 ? `${h} 小时 ${m} 分后重置` : `${h} 小时后重置`;
  }
  const r = new Date(t);
  const pad = (n: number) => String(n).padStart(2, "0");
  const clock = `${pad(r.getHours())}:${pad(r.getMinutes())}`;
  if (dayDiff === 1) return `明天 ${clock} 重置`;
  if (dayDiff === 2) return `后天 ${clock} 重置`;
  return `${r.getMonth() + 1} 月 ${r.getDate()} 日 ${clock} 重置`;
}

function UsageBar({ label, win }: { label: string; win: { utilization: number; resets_at: string } | null }) {
  if (!win) return null;
  const pct = Math.max(0, Math.min(100, win.utilization));
  return (
    <div className="usage-row">
      <div className="usage-head">
        <span className="usage-label">{label}</span>
        <span className="usage-pct">{pct.toFixed(0)}%</span>
      </div>
      <div className="usage-track"><i style={{ width: `${pct}%` }} /></div>
      <div className="usage-reset">{fmtResetIn(win.resets_at)}</div>
    </div>
  );
}

// 每日用量热力图的一个格子：占位（补齐周对齐）或某天的数据 + 着色档位 0-4。
type GridCell = { pad: true } | { pad: false; date: string; tokens: number; messages: number; level: number };

// 固定显示的周数：用足够多的日期（含无活动的空白日）把整宽铺满（设置窗为固定宽）。
const GRID_WEEKS = 18;

/// 排成贡献图格子序列：以末日为终点回溯固定 GRID_WEEKS 周，逐日补全（stats-cache 里没有的
/// 无活动日期填 0 档淡色格子表示空），首日按星期补占位（周日为列起点）；
/// 配合 CSS grid-auto-flow:column + 7 行 + 1fr 列，自动按「每列一周」竖排并铺满整宽。
function buildDailyGrid(days: DailyEntry[]): GridCell[] {
  if (days.length === 0) return [];
  const byDate = new Map(days.map((d) => [d.date, d]));
  const max = Math.max(1, ...days.map((d) => d.tokens));
  const end = new Date(days[days.length - 1].date + "T00:00:00");
  const start = new Date(end);
  start.setDate(end.getDate() - (GRID_WEEKS * 7 - 1)); // 回溯固定周数，不足的早期日期以空白格补满
  const cells: GridCell[] = [];
  for (let i = 0; i < start.getDay(); i++) cells.push({ pad: true }); // 首列按星期补空格对齐
  for (const t = new Date(start); t <= end; t.setDate(t.getDate() + 1)) {
    const iso = `${t.getFullYear()}-${String(t.getMonth() + 1).padStart(2, "0")}-${String(t.getDate()).padStart(2, "0")}`;
    const d = byDate.get(iso);
    const tokens = d?.tokens ?? 0;
    const level = tokens === 0 ? 0 : Math.min(4, Math.ceil((tokens / max) * 4));
    cells.push({ pad: false, date: iso, tokens, messages: d?.message_count ?? 0, level });
  }
  return cells;
}

function AccountSection() {
  const [data, setData] = useState<AccountPayload | null>(null);
  const [usage, setUsage] = useState<Usage | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [usageErr, setUsageErr] = useState(false);

  const doRefresh = () => {
    setRefreshing(true);
    setUsageErr(false);
    refreshUsage()
      .then((u) => setUsage(u))
      .catch(() => setUsageErr(true))
      .finally(() => setRefreshing(false));
  };

  useEffect(() => {
    // 先缓存后请求：getAccount 立即给账号/每日/缓存用量，再 refreshUsage 联网刷新。
    getAccount()
      .then((d) => { setData(d); setUsage(d.usage); })
      .catch(() => {});
    doRefresh();
  }, []);

  const acc = data?.account ?? null;
  const daily = data?.daily ?? null;

  return (
    <>
      {acc ? (
        <div className="row-card">
          <div className="row">
            <div className="row-icon"><div className="acc-avatar">{(acc.display_name || acc.email).slice(0, 1).toUpperCase()}</div></div>
            <div className="row-text">
              <div className="row-label">{acc.display_name}</div>
              <div className="row-desc">{acc.email}{acc.plan ? ` · ${acc.plan}` : ""}{acc.organization ? ` · ${acc.organization}` : ""}</div>
            </div>
          </div>
        </div>
      ) : (
        <div className="row-card"><div className="row"><div className="row-text"><div className="row-label">未登录 Claude Code</div><div className="row-desc">在终端运行 <code>claude</code> 登录后即可查看账号与用量</div></div></div></div>
      )}

      <div className="row-card usage-card">
        <div className="usage-bar-head">
          <span className="usage-card-title">配额</span>
          <button className="icon-btn" title="刷新" aria-label="刷新" disabled={refreshing} onClick={doRefresh}>
            <RefreshIcon spinning={refreshing} />
          </button>
        </div>
        {usage ? (
          <>
            <UsageBar label="5 小时配额" win={usage.five_hour} />
            <UsageBar label="7 天配额" win={usage.seven_day} />
            <UsageBar label="Opus · 7 天" win={usage.seven_day_opus} />
            <UsageBar label="Sonnet · 7 天" win={usage.seven_day_sonnet} />
            {usage.extra_usage_enabled && <div className="usage-extra">已开启超额用量</div>}
            {usageErr && <div className="usage-stale">最新数据刷新失败，显示的是缓存值</div>}
          </>
        ) : usageErr ? (
          <div className="usage-stale">用量暂不可用，请确认已登录 Claude Code（终端运行 claude）或检查网络</div>
        ) : (
          <div className="usage-stale">加载中…</div>
        )}
      </div>

      {daily && daily.days.length > 0 && (
        <>
          <div className="row-card cal-card">
            <div className="usage-bar-head"><span className="usage-card-title">每日用量</span></div>
            <div className="cal-grid">
              {buildDailyGrid(daily.days).map((c, i) =>
                c.pad ? (
                  <span key={i} className="cal-cell cal-pad" />
                ) : (
                  <span
                    key={i}
                    className={`cal-cell cal-l${c.level}`}
                    title={`${c.date} · ${(c.tokens / 1000).toFixed(1)}k token · ${c.messages} 条`}
                  />
                )
              )}
            </div>
            <div className="cal-legend">
              <span>少</span>
              <i className="cal-cell cal-l1" />
              <i className="cal-cell cal-l2" />
              <i className="cal-cell cal-l3" />
              <i className="cal-cell cal-l4" />
              <span>多</span>
            </div>
            <div className="sec-hint">数据截至 {daily.last_computed_date || "—"}，在终端运行 /stats 可刷新</div>
          </div>
        </>
      )}
    </>
  );
}

function Switch({ checked, onChange }: { checked: boolean; onChange: () => void }) {
  return (
    <button type="button" role="switch" aria-checked={checked} className={"pswitch" + (checked ? " on" : "")} onClick={onChange}>
      <span className="pswitch-knob" />
    </button>
  );
}

function Dropdown<T extends string | number>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  const [open, setOpen] = useState(false);
  // 菜单用 fixed 定位（脱离 .row-card/.main-body 的 overflow 裁剪），按钮坐标实时测量。
  const [pos, setPos] = useState<{ top: number; right: number }>({ top: 0, right: 0 });
  const ref = useRef<HTMLDivElement>(null);
  const btnRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const close = () => setOpen(false);
    document.addEventListener("mousedown", onDoc);
    window.addEventListener("resize", close);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      window.removeEventListener("resize", close);
    };
  }, [open]);
  const toggle = () => {
    if (!open) {
      const r = btnRef.current?.getBoundingClientRect();
      if (r) setPos({ top: r.bottom + 6, right: Math.max(0, window.innerWidth - r.right) });
    }
    setOpen((v) => !v);
  };
  const cur = options.find((o) => o.value === value);
  return (
    <div className="dd" ref={ref}>
      <button ref={btnRef} type="button" className={"dd-btn" + (open ? " open" : "")} onClick={toggle}>
        <span>{cur?.label ?? ""}</span>
        <svg className="dd-chev" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>
      {open && (
        <div className="dd-menu" role="listbox" style={{ position: "fixed", top: pos.top, right: pos.right }}>
          {options.map((o) => (
            <button
              type="button"
              role="option"
              aria-selected={o.value === value}
              key={o.value}
              className={"dd-item" + (o.value === value ? " sel" : "")}
              onClick={() => { onChange(o.value); setOpen(false); }}
            >
              <span>{o.label}</span>
              {o.value === value && (
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="20 6 9 17 4 12" />
                </svg>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function GeneralSection() {
  const [autostart, setAutostart] = useState(false);
  const [settings, patch] = useSettingsState();
  const [availTerms, setAvailTerms] = useState<ResumeTerminal[] | null>(null);
  useEffect(() => {
    invoke<boolean>("get_autostart").then(setAutostart).catch(() => {});
    availableTerminals().then(setAvailTerms).catch(() => setAvailTerms([]));
  }, []);
  const toggleAutostart = () => {
    const next = !autostart;
    setAutostart(next);
    invoke("set_autostart", { enabled: next }).catch(() => setAutostart(!next));
  };
  const hideDays = settings?.archive_hide_days ?? 0;
  const notifyOn = settings?.notifications_enabled ?? true;
  const changeHideDays = (days: number) => patch({ archive_hide_days: days });
  const toggleNotify = () => patch({ notifications_enabled: !notifyOn });
  // 终端选项按平台给，再用后端探测到的「本机实际可用」列表过滤（未装的不列出）。
  const platformOpts = IS_MAC ? RESUME_TERM_OPTIONS_MAC : RESUME_TERM_OPTIONS_WIN;
  const termOptions = platformOpts.filter((o) => (availTerms ?? []).includes(o.value));
  // 保存值若不在可用项内（如未装 iTerm 仍存着 "iterm"，或 Windows 上残留 macOS 默认 "terminal"），显示退回首项。
  const storedTerm = settings?.resume_terminal ?? "terminal";
  const resumeTerm = termOptions.some((o) => o.value === storedTerm) ? storedTerm : (termOptions[0]?.value ?? "terminal");
  const changeResumeTerm = (v: ResumeTerminal) => patch({ resume_terminal: v });
  // 至少两个可用终端才有选择意义；只有一个（如 macOS 没装 iTerm）就不显示这一行。
  const showTermRow = (IS_MAC || IS_WIN) && termOptions.length >= 2;
  return (
    <>
      <div className="row-card">
        <div className="row">
          <div className="row-text">
            <div className="row-label">开机自启</div>
            <div className="row-desc">登录系统后自动启动 cc-kanban</div>
          </div>
          <Switch checked={autostart} onChange={toggleAutostart} />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">桌面通知</div>
            <div className="row-desc">会话需要你回复或出错时弹系统通知</div>
          </div>
          <Switch checked={notifyOn} onChange={toggleNotify} />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">归档自动隐藏</div>
            <div className="row-desc">归档超过所选时长后，自动从「已归档」中隐藏</div>
          </div>
          <Dropdown value={hideDays} options={HIDE_OPTIONS} onChange={changeHideDays} />
        </div>
        {showTermRow && (
          <div className="row">
            <div className="row-text">
              <div className="row-label">未连接会话打开终端</div>
              <div className="row-desc">点开已断开的会话时，用哪个终端运行 claude --resume</div>
            </div>
            <Dropdown value={resumeTerm} options={termOptions} onChange={changeResumeTerm} />
          </div>
        )}
      </div>
      <div className="sec-hint">更多设置项陆续补充中…</div>
    </>
  );
}

// 一排互斥的分段按钮（外观模式 / 界面密度）：语义上是单选，用 radiogroup/radio。
function Segmented<T extends string | number>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  label: string;
}) {
  return (
    <div className="seg" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button
          type="button"
          role="radio"
          aria-checked={o.value === value}
          key={String(o.value)}
          className={"seg-btn" + (o.value === value ? " on" : "")}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

const THEME_OPTIONS: { value: ThemeMode; label: string }[] = [
  { value: "dark", label: "深色" },
  { value: "light", label: "浅色" },
  { value: "system", label: "跟随系统" },
];
const DENSITY_OPTIONS: { value: number; label: string }[] = [
  { value: 90, label: "紧凑" },
  { value: 100, label: "标准" },
  { value: 112, label: "宽松" },
];
const OPACITY_MIN = 60;
const OPACITY_MAX = 100;

function AppearanceSection() {
  const [settings, patch] = useSettingsState();
  const theme = settings?.theme ?? "dark";
  const opacity = settings?.opacity ?? 94;
  const uiScale = settings?.ui_scale ?? 100;
  const fill = ((opacity - OPACITY_MIN) / (OPACITY_MAX - OPACITY_MIN)) * 100;
  return (
    <>
      <div className="row-card">
        <div className="row">
          <div className="row-text">
            <div className="row-label">外观模式</div>
            <div className="row-desc">深色、浅色，或跟随系统</div>
          </div>
          <Segmented value={theme} options={THEME_OPTIONS} onChange={(v) => patch({ theme: v })} label="外观模式" />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">界面密度</div>
            <div className="row-desc">调整贴纸卡片的字号与间距</div>
          </div>
          <Segmented value={uiScale} options={DENSITY_OPTIONS} onChange={(v) => patch({ ui_scale: v })} label="界面密度" />
        </div>
        <div className="row row-col">
          <div className="row-head">
            <div className="row-text">
              <div className="row-label">贴纸不透明度</div>
              <div className="row-desc">调整桌面贴纸的背景透明度</div>
            </div>
            <span className="row-val">{opacity}%</span>
          </div>
          <input
            type="range"
            className="slider"
            min={OPACITY_MIN}
            max={OPACITY_MAX}
            value={opacity}
            style={{ background: `linear-gradient(90deg, var(--cc-accent) ${fill}%, var(--cc-border) ${fill}%)` }}
            onChange={(e) => patch({ opacity: Number(e.target.value) })}
            aria-label="贴纸不透明度"
          />
        </div>
      </div>
      <div className="sec-hint">外观更改即时生效，并保存到本地。</div>
    </>
  );
}

function AboutSection({
  status,
  newVersion,
  recheck,
}: {
  status: UpdateStatus;
  newVersion: string | null;
  recheck: () => void;
}) {
  const [version, setVersion] = useState("");
  const [triggered, setTriggered] = useState(false);

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  const onAvailable = () => {
    setTriggered(true);
    emit("trigger-update").catch(() => {});
  };
  const updateBtn =
    status === "available"
      ? { label: triggered ? "更新中…" : `更新到 v${newVersion}`, onClick: onAvailable, disabled: triggered, primary: true }
      : { label: status === "checking" ? "检查中…" : "检查更新", onClick: recheck, disabled: status === "checking", primary: false };

  const verText = `v${version || "—"}`;
  const verStatus =
    status === "available" ? `发现新版本 v${newVersion}` : status === "latest" ? "已是最新版本" : "";
  const verSub = verStatus ? `${verText} · ${verStatus}` : verText;

  return (
    <>
      <div className="row-card">
        <div className="row">
          <div className="row-icon"><div className="pmark"><svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2" strokeLinecap="round"><line x1="7" y1="7" x2="7" y2="17" /><line x1="12" y1="7" x2="12" y2="14" /><line x1="17" y1="7" x2="17" y2="12" /></svg></div></div>
          <div className="row-text">
            <div className="row-label">版本信息</div>
            <div className="row-desc">{verSub}</div>
          </div>
          <button className={"sbtn" + (updateBtn.primary ? " primary" : "")} disabled={updateBtn.disabled} onClick={updateBtn.onClick}>
            {updateBtn.label}
          </button>
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">项目主页</div>
            <div className="row-desc">{REPO}</div>
          </div>
          <button className="sbtn" onClick={() => openExt(REPO_URL)}>
            打开
          </button>
        </div>
      </div>

      <p className="about-blurb">常驻桌面贴纸，实时显示所有 Claude Code 会话的进度。</p>

      <div className="about-foot">
        <a onClick={() => openExt(REPO_URL + "/issues")}>意见反馈</a>
        <span className="dot">·</span>
        <a onClick={() => openExt(REPO_URL + "/releases")}>更新日志</a>
        <div className="copy">MIT License · © 2026 larrygogo</div>
      </div>
    </>
  );
}

export function About() {
  const [sec, setSec] = useState<Section>("general");
  const close = () => getCurrentWindow().close().catch(() => {});
  // 在不随标签切换卸载的父组件里检查更新：每次打开设置窗口只查一次（避免反复点「关于」标签重复请求）。
  const { status, version: newVersion, recheck } = useUpdate();

  return (
    <div className="settings">
      <aside className="side">
        <div className="side-top" data-tauri-drag-region />
        <nav className="side-nav">
          <button className={"nav-item" + (sec === "general" ? " on" : "")} onClick={() => setSec("general")}>
            <IconGear />
            <span>通用</span>
          </button>
          <button className={"nav-item" + (sec === "appearance" ? " on" : "")} onClick={() => setSec("appearance")}>
            <IconAppearance />
            <span>外观</span>
          </button>
          <button className={"nav-item" + (sec === "account" ? " on" : "")} onClick={() => setSec("account")}>
            <IconUser />
            <span>账号</span>
          </button>
          <button className={"nav-item" + (sec === "about" ? " on" : "")} onClick={() => setSec("about")}>
            <IconInfo />
            <span>关于</span>
          </button>
        </nav>
      </aside>

      <main className="main">
        <div className="main-bar" data-tauri-drag-region>
          <button className="winclose" title="关闭" onClick={close} aria-label="关闭">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
              <line x1="6" y1="6" x2="18" y2="18" />
              <line x1="18" y1="6" x2="6" y2="18" />
            </svg>
          </button>
        </div>
        <div className="main-body" key={sec}>
          {sec === "general" ? (
            <GeneralSection />
          ) : sec === "appearance" ? (
            <AppearanceSection />
          ) : sec === "account" ? (
            <AccountSection />
          ) : (
            <AboutSection status={status} newVersion={newVersion} recheck={recheck} />
          )}
        </div>
      </main>
    </div>
  );
}
