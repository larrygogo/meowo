import { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { emit, listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getSettings, setSettings, availableTerminals, type Settings, type ThemeMode, type ResumeTerminal, type TerminalOpenMode, type StickerStyle } from "../api";
import { getAccounts, refreshUsage, type ProviderAccountPayload, type ProviderUsage, type UsageLane } from "../api";
import { PROVIDERS } from "../providers";
import { STICKER_COLORS, STICKER_COLOR_KEYS } from "../appearance";
import { useUpdate, type UpdateStatus } from "../useUpdate";
import { useT } from "../i18n";
import logoUrl from "../../src-tauri/icons/128x128.png";
import type { Dict } from "../i18n/zh";

const hideOptions = (t: Dict) => [
  { value: 0, label: t.settings.hideNever },
  { value: 1, label: t.settings.hideDays(1) },
  { value: 7, label: t.settings.hideDays(7) },
  { value: 30, label: t.settings.hideDays(30) },
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
  language: "auto",
  terminal_open_mode: "card",
  preview_enabled: true,
  sticker_style: "elevated",
  sticker_color: "classic",
  sticker_quota_providers: ["claude"],
};

// 打开未连接会话用的终端：按平台给不同选项。WKWebView 的 UA 含 "Mac"/"Win"，与 main.tsx 同步判定一致。
const IS_MAC = typeof navigator !== "undefined" && /Mac/i.test(navigator.userAgent);
const IS_WIN = typeof navigator !== "undefined" && /Win/i.test(navigator.userAgent);
const RESUME_TERM_OPTIONS_MAC: { value: ResumeTerminal; label: string }[] = [
  { value: "terminal", label: "Terminal" },
  { value: "iterm", label: "iTerm2" },
];
const resumeTermOptionsWin = (t: Dict): { value: ResumeTerminal; label: string }[] => [
  { value: "wt", label: "Windows Terminal" },
  { value: "powershell", label: "PowerShell" },
  { value: "cmd", label: t.settings.cmdPrompt },
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

function fmtResetIn(iso: string, t: Dict): string {
  const ts = Date.parse(iso);
  if (Number.isNaN(ts)) return "";
  const now = Date.now();
  const diffMs = ts - now;
  if (diffMs <= 0) return t.account.resetSoon;
  // 按自然日差判断：今天显示剩余小时/分钟，跨天则用相对词/日期并精确到钟点。
  const startOf = (ms: number) => {
    const d = new Date(ms);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  };
  const dayDiff = Math.round((startOf(ts) - startOf(now)) / 86_400_000);
  if (dayDiff <= 0) {
    const min = Math.round(diffMs / 60000);
    if (min < 1) return t.account.resetSoon; // 剩余不足半分钟时 round 得 0，归入「即将重置」
    if (min < 60) return t.account.resetInMin(min);
    const h = Math.floor(min / 60);
    const m = min % 60;
    return m > 0 ? t.account.resetInHourMin(h, m) : t.account.resetInHour(h);
  }
  const r = new Date(ts);
  const pad = (n: number) => String(n).padStart(2, "0");
  const clock = `${pad(r.getHours())}:${pad(r.getMinutes())}`;
  if (dayDiff === 1) return t.account.resetTomorrow(clock);
  if (dayDiff === 2) return t.account.resetDayAfter(clock);
  return t.account.resetOnDate(r.getMonth() + 1, r.getDate(), clock);
}

function laneLabel(kind: string, t: Dict): string {
  switch (kind) {
    case "five_hour": return t.account.laneFiveHour;
    case "seven_day": return t.account.laneSevenDay;
    case "opus": return t.account.laneOpus;
    case "weekly": return t.account.laneWeekly;
    case "balance": return t.account.laneBalance;
    default: return kind;
  }
}

function UsageBar({ lane, label }: { lane: UsageLane; label: string }) {
  const t = useT();
  if (lane.used_pct != null) {
    const pct = Math.max(0, Math.min(100, lane.used_pct));
    return (
      <div className="usage-row">
        <div className="usage-head">
          <span className="usage-label">{label}</span>
          <span className="usage-pct">{pct.toFixed(0)}%</span>
        </div>
        <div className="usage-track"><i style={{ width: `${pct}%` }} /></div>
        {lane.resets_at && <div className="usage-reset">{fmtResetIn(lane.resets_at, t)}</div>}
      </div>
    );
  }
  // 余额型：显数值，不画进度条
  const valText = lane.used != null ? `${lane.used}${lane.unit ? ` ${lane.unit}` : ""}` : "—";
  return (
    <div className="usage-row">
      <div className="usage-head">
        <span className="usage-label">{label}</span>
        <span className="usage-pct">{valText}</span>
      </div>
    </div>
  );
}

// 单个 provider 卡片：账号信息 + 用量泳道 + 刷新按钮 + 贴纸显示开关
function ProviderCard({ payload, usage, err, onRefresh, refreshing, settings, onToggleQuota }: {
  payload: ProviderAccountPayload;
  usage: ProviderUsage | null;
  err: "unsupported" | "error" | null;
  onRefresh: () => void;
  refreshing: boolean;
  /** 当前应用设置，用于读取 sticker_quota_providers 开关态。 */
  settings: Settings | null;
  /** 切换本 provider 的贴纸配额显示开关。 */
  onToggleQuota: () => void;
}) {
  const t = useT();
  const acc = payload.account;
  const cfg = PROVIDERS[payload.provider];
  const providerName = cfg ? cfg.label(t) : payload.provider;

  // 当前 provider 是否在贴纸配额列表中
  const inQuota = settings?.sticker_quota_providers?.includes(payload.provider) ?? false;

  // activePayloads 已过滤，此处恒不触发（防御）
  if (!acc) return null;

  return (
    <div className="row-card provider-card">
      <div className="provider-head">
        {cfg && <span className="provider-icon"><cfg.Icon /></span>}
        <span className="provider-name">{providerName}</span>
      </div>

      <div className="acc-block">
        <div className="acc-avatar">{((acc.display_name ?? acc.email ?? acc.login_label) ?? "?").slice(0, 1).toUpperCase()}</div>
        <div className="acc-info">
          <div className="acc-name-row">
            <span className="acc-name">{acc.display_name ?? acc.email ?? acc.login_label ?? ""}</span>
            {acc.plan && <span className="acc-plan">{acc.plan}</span>}
          </div>
          {acc.display_name !== acc.email && acc.email && <div className="acc-sub">{acc.email}</div>}
          {!acc.email && acc.login_label && <div className="acc-sub">{acc.login_label}</div>}
          {acc.organization && <div className="acc-org">{acc.organization}</div>}
        </div>
      </div>

      <div className="provider-usage">
        <div className="usage-bar-head">
          <span className="usage-card-title">{t.account.quota}</span>
          <button className="icon-btn" data-tip={t.account.refresh} aria-label={t.account.refresh} disabled={refreshing || err === "unsupported" || (!payload.usage_supported && !usage)} onClick={onRefresh}>
            <RefreshIcon spinning={refreshing} />
          </button>
        </div>
        {usage ? (
          <>
            {usage.lanes.map((lane, i) => (
              <UsageBar key={`${lane.kind}-${i}`} lane={lane} label={laneLabel(lane.kind, t)} />
            ))}
            {usage.note && <div className="usage-extra">{usage.note}</div>}
            {err === "error" && <div className="usage-stale">{t.account.refreshFailed}</div>}
          </>
        ) : !payload.usage_supported || err === "unsupported" ? (
          <div className="usage-stale">{t.account.usageUnsupported}</div>
        ) : err === "error" ? (
          <div className="usage-stale">{t.account.usageUnavailable}</div>
        ) : (
          <div className="usage-stale">{t.account.loading}</div>
        )}
        {/* 贴纸配额显示开关 */}
        <div className="usage-sticker-row">
          <span className="usage-sticker-label">{t.settings.showQuotaOnSticker}</span>
          <Switch checked={inQuota} onChange={onToggleQuota} />
        </div>
      </div>
    </div>
  );
}

function AccountSection() {
  const t = useT();
  // 读取/写入应用设置（用于贴纸配额开关）
  const [settings, patchSettings] = useSettingsState();
  const [payloads, setPayloads] = useState<ProviderAccountPayload[]>([]);
  // usageMap: provider key → 最新 ProviderUsage（缓存先填，联网值覆盖）
  const [usageMap, setUsageMap] = useState<Record<string, ProviderUsage>>({});
  const [refreshingSet, setRefreshingSet] = useState<Set<string>>(new Set());
  // errMap: provider key → 错误类型（unsupported/error/null）
  const [errMap, setErrMap] = useState<Record<string, "unsupported" | "error" | null>>({});

  // 切换某 provider 在贴纸配额列表中的开关状态
  const toggleQuotaProvider = (provider: string) => {
    if (!settings) return;
    const list = settings.sticker_quota_providers ?? [];
    const next = list.includes(provider)
      ? list.filter((p) => p !== provider)
      : [...list, provider];
    patchSettings({ sticker_quota_providers: next });
  };

  const doRefresh = (provider: string) => {
    setRefreshingSet((s) => new Set([...s, provider]));
    setErrMap((m) => ({ ...m, [provider]: null }));
    refreshUsage(provider)
      .then((u) => {
        setUsageMap((m) => ({ ...m, [provider]: u }));
      })
      .catch((e) => {
        const unsupported = String(e).includes("USAGE_UNSUPPORTED");
        setErrMap((m) => ({ ...m, [provider]: unsupported ? "unsupported" : "error" }));
      })
      .finally(() => {
        setRefreshingSet((s) => { const n = new Set(s); n.delete(provider); return n; });
      });
  };

  useEffect(() => {
    // 先从 getAccounts 拿缓存数据快速渲染，再对每个 usage_supported provider 联网刷新
    getAccounts()
      .then((ps) => {
        setPayloads(ps);
        // 用缓存 usage 预填
        const initial: Record<string, ProviderUsage> = {};
        ps.forEach((p) => { if (p.usage) initial[p.provider] = p.usage; });
        setUsageMap(initial);
        // 对支持用量的 provider 发起联网刷新
        ps.filter((p) => p.usage_supported).forEach((p) => doRefresh(p.provider));
      })
      .catch(() => {});
  }, []);

  // 有账号的 provider 列表；没有则显示未登录占位
  const activePayloads = payloads.filter((p) => p.account != null);

  if (payloads.length === 0) {
    // 初始加载中
    return (
      <div className="row-card provider-card">
        <div className="acc-block">
          <div className="acc-avatar acc-avatar-empty"><IconUser /></div>
          <div className="acc-info">
            <div className="acc-name">{t.account.loading}</div>
          </div>
        </div>
      </div>
    );
  }

  if (activePayloads.length === 0) {
    // 所有 provider 均未登录
    return (
      <div className="row-card provider-card">
        <div className="acc-block">
          <div className="acc-avatar acc-avatar-empty"><IconUser /></div>
          <div className="acc-info">
            <div className="acc-name">{t.account.notLoggedIn}</div>
            <div className="acc-sub acc-sub-wrap">{t.account.notLoggedInDesc}</div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <>
      {activePayloads.map((p) => (
        <ProviderCard
          key={p.provider}
          payload={p}
          usage={usageMap[p.provider] ?? null}
          err={errMap[p.provider] ?? null}
          onRefresh={() => doRefresh(p.provider)}
          refreshing={refreshingSet.has(p.provider)}
          settings={settings}
          onToggleQuota={() => toggleQuotaProvider(p.provider)}
        />
      ))}
    </>
  );
}

function Switch({ checked, onChange, disabled }: { checked: boolean; onChange: () => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      className={"pswitch" + (checked ? " on" : "")}
      onClick={onChange}
    >
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
  // WebView 内容无法超出原生窗口 → 按钮靠近窗口底部、下方放不下时向上翻转弹出。
  const [pos, setPos] = useState<{ top?: number; bottom?: number; right: number }>({ top: 0, right: 0 });
  const ref = useRef<HTMLDivElement>(null);
  const btnRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const close = () => setOpen(false);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    window.addEventListener("resize", close);
    // 菜单是 fixed 定位、坐标在打开时一次性测量；滚动 .main-body 后会与按钮错位 → 滚动即关（capture 捕获内层滚动）。
    window.addEventListener("scroll", close, true);
    document.addEventListener("keydown", onKey); // Esc 关闭
    return () => {
      document.removeEventListener("mousedown", onDoc);
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", close, true);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);
  const toggle = () => {
    if (!open) {
      const r = btnRef.current?.getBoundingClientRect();
      if (r) {
        const right = Math.max(0, window.innerWidth - r.right);
        // 估算菜单高（项高约 30px + 容器内边距），下方放不下且上方空间更充裕时向上弹。
        const estHeight = options.length * 30 + 10;
        const fitsBelow = r.bottom + 6 + estHeight <= window.innerHeight;
        if (!fitsBelow && r.top > window.innerHeight - r.bottom) {
          setPos({ bottom: window.innerHeight - r.top + 6, right });
        } else {
          setPos({ top: r.bottom + 6, right });
        }
      }
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
        <div className="dd-menu" role="listbox" style={{ position: "fixed", top: pos.top, bottom: pos.bottom, right: pos.right }}>
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
  const t = useT();
  const [autostart, setAutostart] = useState(false);
  const [settings, patch] = useSettingsState();
  const [availTerms, setAvailTerms] = useState<ResumeTerminal[] | null>(null);
  // dev 下开机自启会注册调试二进制(开机连不上 dev server → 白屏)，故禁用此开关，仅安装版可用。
  const autostartDisabled = import.meta.env.DEV;
  useEffect(() => {
    if (!autostartDisabled) invoke<boolean>("get_autostart").then(setAutostart).catch(() => {});
    availableTerminals().then(setAvailTerms).catch(() => setAvailTerms([]));
  }, [autostartDisabled]);
  const toggleAutostart = () => {
    if (autostartDisabled) return;
    const next = !autostart;
    setAutostart(next);
    invoke("set_autostart", { enabled: next }).catch(() => setAutostart(!next));
  };
  const hideDays = settings?.archive_hide_days ?? 0;
  const notifyOn = settings?.notifications_enabled ?? true;
  const previewOn = settings?.preview_enabled ?? true;
  const changeHideDays = (days: number) => patch({ archive_hide_days: days });
  const toggleNotify = () => patch({ notifications_enabled: !notifyOn });
  const togglePreview = () => patch({ preview_enabled: !previewOn });
  // 终端选项按平台给，再用后端探测到的「本机实际可用」列表过滤（未装的不列出）。
  const platformOpts = IS_MAC ? RESUME_TERM_OPTIONS_MAC : resumeTermOptionsWin(t);
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
            <div className="row-label">{t.settings.autostart}</div>
            <div className="row-desc">{t.settings.autostartDesc}</div>
          </div>
          <Switch checked={autostart} onChange={toggleAutostart} disabled={autostartDisabled} />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.notify}</div>
            <div className="row-desc">{t.settings.notifyDesc}</div>
          </div>
          <Switch checked={notifyOn} onChange={toggleNotify} />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.preview}</div>
            <div className="row-desc">{t.settings.previewDesc}</div>
          </div>
          <Switch checked={previewOn} onChange={togglePreview} />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.terminalOpen}</div>
            <div className="row-desc">{t.settings.terminalOpenDesc}</div>
          </div>
          <Dropdown
            value={settings?.terminal_open_mode ?? "card"}
            options={[
              { value: "card" as const, label: t.settings.openModeCard },
              { value: "button" as const, label: t.settings.openModeButton },
            ]}
            onChange={(v: TerminalOpenMode) => patch({ terminal_open_mode: v })}
          />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.language}</div>
            <div className="row-desc">{t.settings.languageDesc}</div>
          </div>
          <Dropdown
            value={settings?.language ?? "auto"}
            options={[
              { value: "auto" as const, label: t.settings.langAuto },
              { value: "zh" as const, label: "中文" },
              { value: "en" as const, label: "English" },
            ]}
            onChange={(v) => patch({ language: v })}
          />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.archiveHide}</div>
            <div className="row-desc">{t.settings.archiveHideDesc}</div>
          </div>
          <Dropdown value={hideDays} options={hideOptions(t)} onChange={changeHideDays} />
        </div>
        {showTermRow && (
          <div className="row">
            <div className="row-text">
              <div className="row-label">{t.settings.resumeTerm}</div>
              <div className="row-desc">{t.settings.resumeTermDesc}</div>
            </div>
            <Dropdown value={resumeTerm} options={termOptions} onChange={changeResumeTerm} />
          </div>
        )}
      </div>
      <div className="sec-hint">{t.settings.moreSoon}</div>
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

// 贴纸颜色色板：一排圆色块（鲜亮代表色），选中加高亮描边圈；点选即换。语义上单选，用 radiogroup/radio。
function SwatchPicker({
  value,
  onChange,
  label,
  names,
}: {
  value: string;
  onChange: (v: string) => void;
  label: string;
  names: Record<string, string>;
}) {
  return (
    <div className="swatches" role="radiogroup" aria-label={label}>
      {STICKER_COLOR_KEYS.map((k) => (
        <button
          type="button"
          role="radio"
          aria-checked={k === value}
          key={k}
          className={"swatch" + (k === value ? " sel" : "")}
          style={{ background: STICKER_COLORS[k].swatch }}
          data-tip={names[k] ?? k}
          aria-label={names[k] ?? k}
          onClick={() => onChange(k)}
        />
      ))}
    </div>
  );
}

const themeOptions = (t: Dict): { value: ThemeMode; label: string }[] => [
  { value: "dark", label: t.settings.themeDark },
  { value: "light", label: t.settings.themeLight },
  { value: "system", label: t.settings.themeSystem },
];
const stickerStyleOptions = (t: Dict): { value: StickerStyle; label: string }[] => [
  { value: "elevated", label: t.settings.styleElevated },
  { value: "flat", label: t.settings.styleFlat },
];
const densityOptions = (t: Dict): { value: number; label: string }[] => [
  { value: 90, label: t.settings.densityCompact },
  { value: 100, label: t.settings.densityNormal },
  { value: 112, label: t.settings.densityLoose },
];
const OPACITY_MIN = 25;
const OPACITY_MAX = 100;

function AppearanceSection() {
  const t = useT();
  const [settings, patch] = useSettingsState();
  const theme = settings?.theme ?? "dark";
  const opacity = settings?.opacity ?? 94;
  const uiScale = settings?.ui_scale ?? 100;
  const stickerStyle = settings?.sticker_style ?? "elevated";
  const stickerColor = settings?.sticker_color ?? "classic";
  // 钳到 [0,100]：手改 settings.json 为越界值时，避免算出负/超界的 linear-gradient 填充宽度。
  const fill = Math.max(0, Math.min(100, ((opacity - OPACITY_MIN) / (OPACITY_MAX - OPACITY_MIN)) * 100));
  return (
    <>
      <div className="row-card">
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.theme}</div>
            <div className="row-desc">{t.settings.themeDesc}</div>
          </div>
          <Segmented value={theme} options={themeOptions(t)} onChange={(v) => patch({ theme: v })} label={t.settings.theme} />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.density}</div>
            <div className="row-desc">{t.settings.densityDesc}</div>
          </div>
          <Segmented value={uiScale} options={densityOptions(t)} onChange={(v) => patch({ ui_scale: v })} label={t.settings.density} />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.stickerStyle}</div>
            <div className="row-desc">{t.settings.stickerStyleDesc}</div>
          </div>
          <Segmented value={stickerStyle} options={stickerStyleOptions(t)} onChange={(v) => patch({ sticker_style: v })} label={t.settings.stickerStyle} />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.stickerColor}</div>
            <div className="row-desc">{t.settings.stickerColorDesc}</div>
          </div>
          <SwatchPicker value={stickerColor} onChange={(v) => patch({ sticker_color: v })} label={t.settings.stickerColor} names={t.settings.colorNames} />
        </div>
        <div className="row row-col">
          <div className="row-head">
            <div className="row-text">
              <div className="row-label">{t.settings.opacity}</div>
              <div className="row-desc">{t.settings.opacityDesc}</div>
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
            aria-label={t.settings.opacity}
          />
        </div>
      </div>
      <div className="sec-hint">{t.settings.appearanceHint}</div>
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
  const t = useT();
  const [version, setVersion] = useState("");
  const [triggered, setTriggered] = useState(false);

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  // 主窗安装失败会广播 update-failed：复位按钮允许重试（cancelled 标记防 resolve 前卸载的泄漏）。
  useEffect(() => {
    let cancelled = false;
    let un: (() => void) | undefined;
    try {
      listen("update-failed", () => setTriggered(false))
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

  const onAvailable = () => {
    setTriggered(true);
    emit("trigger-update").catch(() => {});
  };
  const updateBtn =
    status === "available"
      ? { label: triggered ? t.about.updating : t.about.updateTo(newVersion ?? ""), onClick: onAvailable, disabled: triggered, primary: true }
      : { label: status === "checking" ? t.about.checking : t.about.checkUpdate, onClick: recheck, disabled: status === "checking", primary: false };

  const verText = `v${version || "—"}`;
  const verStatus =
    status === "available" ? t.about.foundNew(newVersion ?? "") : status === "latest" ? t.about.upToDate : "";
  const verSub = verStatus ? `${verText} · ${verStatus}` : verText;

  return (
    <>
      <div className="row-card">
        <div className="row">
          <div className="row-icon"><img className="pmark" src={logoUrl} width={38} height={38} alt="" /></div>
          <div className="row-text">
            <div className="row-label">{t.about.versionInfo}</div>
            <div className="row-desc">{verSub}</div>
          </div>
          <button className={"sbtn" + (updateBtn.primary ? " primary" : "")} disabled={updateBtn.disabled} onClick={updateBtn.onClick}>
            {updateBtn.label}
          </button>
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.about.homepage}</div>
            <div className="row-desc">{REPO}</div>
          </div>
          <button className="sbtn" onClick={() => openExt(REPO_URL)}>
            {t.about.open}
          </button>
        </div>
      </div>

      <p className="about-blurb">{t.about.blurb}</p>

      <div className="about-foot">
        <a onClick={() => openExt(REPO_URL + "/issues")}>{t.about.feedback}</a>
        <span className="dot">·</span>
        <a onClick={() => openExt(REPO_URL + "/releases")}>{t.about.changelog}</a>
        <div className="copy">MIT License · © 2026 larrygogo</div>
      </div>
    </>
  );
}

export function About() {
  const t = useT();
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
            <span>{t.settings.nav.general}</span>
          </button>
          <button className={"nav-item" + (sec === "appearance" ? " on" : "")} onClick={() => setSec("appearance")}>
            <IconAppearance />
            <span>{t.settings.nav.appearance}</span>
          </button>
          <button className={"nav-item" + (sec === "account" ? " on" : "")} onClick={() => setSec("account")}>
            <IconUser />
            <span>{t.settings.nav.account}</span>
          </button>
          <button className={"nav-item" + (sec === "about" ? " on" : "")} onClick={() => setSec("about")}>
            <IconInfo />
            <span>{t.settings.nav.about}</span>
            {status === "available" && <span className="nav-tag">{t.settings.updateTag}</span>}
          </button>
        </nav>
      </aside>

      <main className="main">
        <div className="main-bar" data-tauri-drag-region>
          <button className="winclose" data-tip={t.settings.close} onClick={close} aria-label={t.settings.close}>
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
