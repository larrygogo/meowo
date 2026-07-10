import { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getSettings, setSettings, availableTerminals, listAgents, agentName, installAgent, type AgentId, type AgentDescriptor, type Settings, type ThemeMode, type ResumeTerminal, type TerminalOpenMode, type CardMenuMode, type StickerStyle, type InstallDone } from "../api";
import { getAccounts, refreshUsage, checkProviderHooks, repairProviderHooks, loginAgent, cancelLogin, agentPathGap, addAgentToUserPath, type ProviderAccountPayload, type ProviderUsage, type UsageLane, type HooksStatus, type LoginDone } from "../api";
import { agentAssets } from "../providers";
import { STICKER_COLORS, STICKER_COLOR_KEYS } from "../appearance";
import { useUpdate, type UpdateStatus } from "../useUpdate";
import { useT, repairFailMessage } from "../i18n";
import logoUrl from "../../src-tauri/icons/128x128.png";
import type { Dict } from "../i18n/zh";

const hideOptions = (t: Dict) => [
  { value: 0, label: t.settings.hideNever },
  { value: 1, label: t.settings.hideDays(1) },
  { value: 7, label: t.settings.hideDays(7) },
  { value: 30, label: t.settings.hideDays(30) },
];

const REPO = "github.com/larrygogo/meowo";
const REPO_URL = "https://github.com/larrygogo/meowo";
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
  card_menu_mode: "button",
  preview_enabled: true,
  sticker_style: "elevated",
  sticker_color: "classic",
  // 首帧占位（get_settings() resolve 前）。真实默认值由后端 settings 给，前端不据此做任何判断。
  sticker_quota_providers: ["claude"],
  default_agent: "claude",
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
  { value: "wezterm", label: "WezTerm" },
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

function IconDownload() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <polyline points="7 10 12 15 17 10" />
      <line x1="12" y1="15" x2="12" y2="3" />
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

// note 是后端机器哨兵串（claude 发 "extra_usage_enabled"、codex 发 "credits:45.5"），
// 映射为本地化文案；未知格式原样显示以向后兼容。
function renderNote(note: string, t: Dict): string {
  if (note === "extra_usage_enabled") return t.account.extraUsage;
  if (note.startsWith("credits:")) return t.account.credits(note.slice("credits:".length));
  return note;
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

// 单个 provider 卡片：安装/登录/用量三态。已装且登录 = 现有账号信息 + 用量泳道 + 刷新按钮 + 贴纸显示开关；
// 已装未登录 = 提示语；未装 = 一键安装按钮。
function ProviderCard({ provider, name, installed, payload, usage, err, onRefresh, onInstalled, onLoggedIn, refreshing, settings, onToggleQuota }: {
  provider: AgentId;
  /** 展示名，来自后端 list_agents()（产品名，不翻译）。 */
  name: string;
  /** null = 安装状态检测中（listAgents() 尚未 resolve），此时不渲染未安装/已安装的判定分支。 */
  installed: boolean | null;
  payload: ProviderAccountPayload | null;
  usage: ProviderUsage | null;
  err: "unsupported" | "error" | null;
  onRefresh: () => void;
  /** 后台安装成功后重查安装检测（令卡片转「已装」）。 */
  onInstalled: () => void;
  /** 登录成功后重查账号（令卡片转「已登录」并显示身份/用量）。 */
  onLoggedIn: () => void;
  refreshing: boolean;
  /** 当前应用设置，用于读取 sticker_quota_providers 开关态。 */
  settings: Settings | null;
  /** 切换本 provider 的贴纸配额显示开关。 */
  onToggleQuota: () => void;
}) {
  const t = useT();
  const assets = agentAssets(provider);
  const acc = payload?.account ?? null;

  // 后台安装态：idle=未装可点 / installing=转圈+本地化「安装中…」/ error=失败可重试。
  // 不透传安装脚本的英文原始输出，进度只用 i18n 文案（随界面语言）。
  const [installState, setInstallState] = useState<"idle" | "installing" | "error">("idle");
  // 安装失败时的日志落点（后端把脚本输出重定向到该文件）。只展示路径，不透传英文原文。
  const [installLog, setInstallLog] = useState<string | null>(null);
  // 后端在**跑脚本之前**就失败时的诊断（如引导脚本被 Cloudflare 人机校验拦截）。这是我们自己写的
  // 中文诊断，不是脚本的英文输出，故直接展示——此时还没有日志文件，不给这句话用户就一点线索都没有。
  const [installMsg, setInstallMsg] = useState<string | null>(null);
  // 装好了但 bin 目录不在持久 PATH 上时，这里是那个目录——终端里敲命令会找不到。
  // 官方安装器不保证写 PATH（claude 在 Windows 上只打印一行提示就 exit 0），而 meowo 启动
  // agent 走绝对路径、察觉不到，故须显式检测并给用户一键写入。
  const [pathGapDir, setPathGapDir] = useState<string | null>(null);
  const [addingPath, setAddingPath] = useState(false);
  const [pathMsg, setPathMsg] = useState<string | null>(null);
  const [hooksStatus, setHooksStatus] = useState<HooksStatus | null>(null);
  const [repairingHooks, setRepairingHooks] = useState(false);
  const [repairMsg, setRepairMsg] = useState<string | null>(null);
  // 登录态：waiting=已拉起终端、等 login-done；msg=超时/失败提示。
  const [loginBusy, setLoginBusy] = useState(false);
  const [loginMsg, setLoginMsg] = useState<string | null>(null);
  // 本次等待是否由用户主动取消。login-done 只带 ok:bool，分不出「超时」与「取消」。
  const cancelledRef = useRef(false);
  // 刚装完（本次会话内）→ 把「登录」按钮标为下一步，把「装完 → 登录」串成一条链路。
  const [justInstalled, setJustInstalled] = useState(false);
  // onInstalled/onLoggedIn 每次渲染新建，用 ref 存最新，事件订阅只依赖 provider、不反复重订。
  const onInstalledRef = useRef(onInstalled);
  onInstalledRef.current = onInstalled;
  const onLoggedInRef = useRef(onLoggedIn);
  onLoggedInRef.current = onLoggedIn;

  useEffect(() => {
    let cancelled = false;
    checkProviderHooks(provider)
      .then((st) => { if (!cancelled) setHooksStatus(st); })
      .catch(() => {});
    // 挂载即查：早就装好、但从来没进过 PATH 的用户（本 bug 的多数受害者）也要看到提示，
    // 不能只在「本次装完」时查。后端对未安装的 agent 返回 null，无需先判 installed。
    agentPathGap(provider)
      .then((d) => { if (!cancelled) setPathGapDir(d); })
      .catch(() => {});
    return () => { cancelled = true; };
  }, [provider]);

  /** 把 agent 的 bin 目录写进用户级 PATH。成功后清掉提示条，并告知需重开终端。 */
  const addPath = () => {
    if (addingPath) return;
    setAddingPath(true);
    setPathMsg(null);
    addAgentToUserPath(provider)
      .then(() => {
        setPathGapDir(null);
        setPathMsg(t.account.pathAdded);
      })
      .catch(() => setPathMsg(t.account.pathAddFailed))
      .finally(() => setAddingPath(false));
  };

  const repairHooks = () => {
    if (repairingHooks) return;
    setRepairingHooks(true);
    setRepairMsg(null);
    repairProviderHooks(provider)
      .then((res) => {
        setHooksStatus(res.status);
        // 修复后仍非 installed → 接线没真正生效；按后端回传的 reason 给出精准提示
        // （如 kimi 未登录 → 「请先登录」），不再静默吞掉。
        setRepairMsg(res.status === "installed" ? null : repairFailMessage(t, res.reason));
      })
      .catch(() => setRepairMsg(repairFailMessage(t, null)))
      .finally(() => setRepairingHooks(false));
  };

  const startInstall = () => {
    setInstallState("installing");
    setInstallMsg(null);
    setInstallLog(null);
    // 后端在 spawn 之前的失败（取不到脚本 / 被 CF 拦）走这里，错误串是我们自己的中文诊断。
    // 此前是 `.catch(() => setInstallState("error"))`，把它整个丢掉了——而此时还没有日志文件，
    // 用户只会看到一句通用的「安装失败」，一点线索都没有。
    installAgent(provider).catch((e) => {
      setInstallState("error");
      setInstallMsg(String(e));
    });
  };

  /** 拉起交互式登录。成功 spawn 后不清 busy——等 login-done（或 5 分钟超时 / 用户取消）才落回。 */
  const startLogin = () => {
    if (loginBusy) return;
    setLoginBusy(true);
    setLoginMsg(t.account.loggingIn); // 按钮此时是「取消等待」，等待态由这行文字承载
    loginAgent(provider).catch(() => {
      setLoginBusy(false);
      setLoginMsg(t.account.loginFailed);
    });
  };

  /**
   * 取消等待。终端可能已经被关掉（用户手动关、崩溃、agent 自己退出），而后端只轮询账号文件，
   * 要 5 分钟才超时——这五分钟里按钮一直不可点。
   *
   * 不检测「终端还活着吗」：`wt.exe` 拉起窗口后自身立即退出，真正跑登录的是它的孙进程；
   * 而 `powershell -NoExit` 又会一直活着。三种终端行为不一致，靠监视进程只会时灵时不灵。
   *
   * 收尾由后端 emit `login-done`（它会再查一次账号，真登上了就报 ok:true），故这里不清 busy。
   */
  const cancelLoginWait = () => {
    if (!loginBusy) return;
    // 后端的 login-done 只带 ok:bool，分不出「超时」还是「被取消」。发起方自己记一笔，
    // 好让 handler 给出准确的提示（取消 ≠ 没检测到登录完成）。
    cancelledRef.current = true;
    cancelLogin(provider).catch(() => {
      // 命令本身失败（不该发生）：至少别把按钮永久卡在等待态。
      cancelledRef.current = false;
      setLoginBusy(false);
      setLoginMsg(t.account.loginCancelled);
    });
  };

  useEffect(() => {
    // 只关心装完结果；进度不透传英文，无需订阅 install-progress。
    const unD = listen<InstallDone>("install-done", (e) => {
      if (e.payload.provider !== provider) return;
      setInstallLog(e.payload.logPath);
      // 脚本确实跑起来了 → 失败原因在日志里，清掉「跑之前」那条诊断，免得两种失败串台。
      setInstallMsg(null);
      if (e.payload.ok) {
        setInstallState("idle");
        setJustInstalled(true); // 装完通常尚未登录 → 高亮「登录」作为下一步
        onInstalledRef.current();
        // 「退出码 0」不等于「装好就能用」：claude 的安装器不写 PATH 也照样 exit 0。
        agentPathGap(provider).then(setPathGapDir).catch(() => {});
        // 后端装完顺手接了 hooks（best-effort）——重查一下，接上了就让「未接入」提示条自动消失。
        // 装完常常还没登录（kimi 的 config.toml 尚不存在），此时多半仍未接上，那就等登录后再接。
        checkProviderHooks(provider).then(setHooksStatus).catch(() => {});
      } else {
        setInstallState("error");
      }
    });
    // 登录在 detach 的外部终端里完成，拿不到退出码——后端轮询账号解析结果后发 login-done。
    const unL = listen<LoginDone>("login-done", (e) => {
      if (e.payload.provider !== provider) return;
      const cancelled = cancelledRef.current;
      cancelledRef.current = false;
      setLoginBusy(false);
      if (e.payload.ok) {
        // 取消时后端会再查一次账号——用户可能已经在终端里登完了，只是嫌等得慢。
        setLoginMsg(null);
        setJustInstalled(false);
        onLoggedInRef.current(); // 重查账号 → 卡片转「已登录」并显示身份/用量
        // 登录后配置文件才生成，是三家都接得上 hooks 的时机——后端已顺手接了，这里重查让提示条消失。
        checkProviderHooks(provider).then(setHooksStatus).catch(() => {});
      } else if (cancelled) {
        setLoginMsg(t.account.loginCancelled); // 取消 ≠ 没检测到登录完成
      } else {
        setLoginMsg(t.account.loginTimeout); // 超时 ≠ 登录失败：用户可能中途放弃了
      }
    });
    return () => {
      unD.then((f) => f());
      unL.then((f) => f());
    };
  }, [provider, t]);

  // 当前 provider 是否在贴纸配额列表中
  const inQuota = settings?.sticker_quota_providers?.includes(provider) ?? false;

  // 安装态优先：未安装时一律按未安装展示（即使本地缓存了旧账号信息），
  // 只有「已安装且账号存在」才展示登录身份与用量。
  const isInstalled = installed === true;
  const isLoggedIn = isInstalled && acc != null;
  const statusBadge = !isInstalled
    ? installed === false
      ? t.account.notInstalled
      : null
    : acc
    ? null
    : t.account.notLoggedIn;
  // 只显示邮箱：显示名 + 邮箱 + 组织三段拼起来又长又重复（个人账号的组织名就是
  // 「<邮箱>'s Organization」）。邮箱本身已足够标识「登录的是哪个账号」。
  // 回退链兜住没有邮箱的登录方式（如 codex 的 API key，只有 login_label）。
  const desc = isLoggedIn
    ? acc.email ?? acc.display_name ?? acc.login_label ?? ""
    : installed === false
    ? t.account.installHint
    : isInstalled
    ? t.account.notLoggedInHint
    : "";

  return (
    <div className="row-card provider-card" data-testid={"agent-card-" + provider}>
      <div className="provider-card-head">
        <div className={"provider-card-icon" + (assets.needsTile ? " provider-card-icon-tile" : "")}>
          <assets.Icon />
        </div>
        <div className="provider-card-title">
          <div className="provider-card-title-row">
            <span className="provider-name">{name}</span>
            {isLoggedIn && acc?.plan && <span className="provider-badge provider-badge-plan">{acc.plan}</span>}
            {statusBadge && <span className={"provider-badge" + (installed === false ? " provider-badge-off" : "")}>{statusBadge}</span>}
          </div>
          {/* 账号信息紧贴标题下方。邮箱可能很长，单行省略；title 属性兜住完整值。 */}
          {desc && (
            <div className="provider-card-desc" title={desc} data-testid={"agent-desc-" + provider}>
              {desc}
            </div>
          )}
        </div>
        {installed === false &&
          (installState === "installing" ? (
            <div className="agent-install-progress" data-testid={"agent-installing-" + provider}>
              <RefreshIcon spinning />
              <span className="agent-install-step">{t.account.installing}</span>
            </div>
          ) : (
            <button
              type="button"
              className="provider-card-action provider-card-action-primary"
              data-testid={"agent-install-" + provider}
              onClick={startInstall}
            >
              <IconDownload />
              {installState === "error" ? t.account.installRetry : t.account.install}
            </button>
          ))}
        {isInstalled && !isLoggedIn && (
          <button
            type="button"
            className={"provider-card-action" + (justInstalled ? " provider-card-action-primary" : "")}
            data-testid={"agent-login-" + provider}
            // 等待中不再是死按钮：终端可能已被关掉（用户手动关/崩溃），而后端只轮询账号文件，
            // 要 5 分钟才超时。点它即取消等待，立刻落回可点状态。
            onClick={loginBusy ? cancelLoginWait : startLogin}
            title={loginBusy ? t.account.cancelLogin : undefined}
          >
            {loginBusy ? t.account.cancelLogin : t.account.login}
          </button>
        )}
        {isInstalled && hooksStatus && (hooksStatus === "missing" || hooksStatus === "unknown") && (
          <button
            type="button"
            className="provider-card-action"
            data-testid={"agent-repair-hooks-" + provider}
            onClick={repairHooks}
            disabled={repairingHooks}
          >
            {repairingHooks ? t.newSession.repairingHooks : t.newSession.repairHooks}
          </button>
        )}
      </div>

      {installed === false && installState === "error" && (
        <div className="provider-card-body agent-install-error" data-testid={"agent-install-error-" + provider}>
          {/* 有后端诊断（被 CF 拦、取不到脚本）就显示它；否则脚本跑失败了，给通用文案 + 日志路径。 */}
          {installMsg ?? t.account.installFailed}
          {installLog && (
            <div className="agent-install-log" data-testid={"agent-install-log-" + provider}>
              {t.account.installLogHint(installLog)}
            </div>
          )}
        </div>
      )}

      {/* 装好了却不在 PATH 上：终端里敲不出来。给出目录 + 一键写入用户级 PATH。 */}
      {isInstalled && pathGapDir && (
        <div className="provider-card-body agent-path-gap" data-testid={"agent-path-gap-" + provider}>
          <span className="agent-path-gap-text">{t.account.pathGap(pathGapDir)}</span>
          <button
            type="button"
            className="provider-card-action"
            data-testid={"agent-add-path-" + provider}
            onClick={addPath}
            disabled={addingPath}
          >
            {addingPath ? t.account.addingToPath : t.account.addToPath}
          </button>
        </div>
      )}

      {pathMsg && (
        <div className="provider-card-body agent-path-msg" data-testid={"agent-path-msg-" + provider}>
          {pathMsg}
        </div>
      )}

      {loginMsg && (
        <div className="provider-card-body agent-install-error" data-testid={"agent-login-error-" + provider}>
          {loginMsg}
        </div>
      )}

      {repairMsg && (
        <div className="provider-card-body agent-install-error" data-testid={"agent-repair-failed-" + provider}>
          {repairMsg}
        </div>
      )}


      {isLoggedIn && (
        <div className="provider-usage">
          <div className="usage-bar-head">
            <span className="usage-card-title">{t.account.quota}</span>
            <button className="icon-btn" data-tip={t.account.refresh} aria-label={t.account.refresh} disabled={refreshing || err === "unsupported" || (!(payload?.usage_supported ?? false) && !usage)} onClick={onRefresh}>
              <RefreshIcon spinning={refreshing} />
            </button>
          </div>
          {usage ? (
            <>
              {usage.lanes.map((lane, i) => (
                <UsageBar key={`${lane.kind}-${i}`} lane={lane} label={laneLabel(lane.kind, t)} />
              ))}
              {usage.note && <div className="usage-extra">{renderNote(usage.note, t)}</div>}
              {err === "error" && <div className="usage-stale">{t.account.refreshFailed}</div>}
            </>
          ) : !(payload?.usage_supported ?? false) || err === "unsupported" ? (
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
      )}
    </div>
  );
}

export function AccountSection() {
  // 读取/写入应用设置（用于贴纸配额开关）
  const [settings, patchSettings] = useSettingsState();
  const [payloads, setPayloads] = useState<ProviderAccountPayload[]>([]);
  // usageMap: provider key → 最新 ProviderUsage（缓存先填，联网值覆盖）
  const [usageMap, setUsageMap] = useState<Record<string, ProviderUsage>>({});
  const [refreshingSet, setRefreshingSet] = useState<Set<string>>(new Set());
  // errMap: provider key → 错误类型（unsupported/error/null）
  const [errMap, setErrMap] = useState<Record<string, "unsupported" | "error" | null>>({});
  // agents: 后端下发的 agent 名单（含展示名与安装态）。前端不再自己维护这份名单。
  // 初值 null = 检测中：首帧不判定任何一张卡为未安装，避免 listAgents() resolve 前误闪「未安装 + 安装按钮」。
  const [agents, setAgents] = useState<AgentDescriptor[] | null>(null);
  const installed = agents === null ? null : new Set(agents.filter((a) => a.installed).map((a) => a.id));
  // 重查 agent 名单（安装态会变）。挂载、窗口聚焦、后台安装成功各处复用。
  const refreshInstalled = () => {
    listAgents().then(setAgents).catch(() => {});
  };
  useEffect(() => { refreshInstalled(); }, []);
  useEffect(() => {
    const onFocus = () => refreshInstalled();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, []);

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
    const startedAt = Date.now();
    refreshUsage(provider)
      .then((u) => {
        setUsageMap((m) => ({ ...m, [provider]: u }));
      })
      .catch((e) => {
        const unsupported = String(e).includes("USAGE_UNSUPPORTED");
        setErrMap((m) => ({ ...m, [provider]: unsupported ? "unsupported" : "error" }));
      })
      .finally(() => {
        // 最短转 500ms：本地(codex)/缓存(60s 内)刷新近乎瞬时，否则 spinner 一闪即逝、看不见动画。
        const wait = Math.max(0, 500 - (Date.now() - startedAt));
        setTimeout(() => {
          setRefreshingSet((s) => { const n = new Set(s); n.delete(provider); return n; });
        }, wait);
      });
  };

  // 先从 getAccounts 拿缓存数据快速渲染，再对每个 usage_supported provider 联网刷新。
  // 挂载时与登录成功后各调一次（登录成功前该 provider 的 account 为 null，卡片显示「未登录」）。
  const loadAccounts = () => {
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
  };
  useEffect(() => { loadAccounts(); }, []);

  // 以后端下发的 agent 名单为骨架遍历（而非只 getAccounts 返回的有账号项），
  // 每张卡按 installed/payload 自行渲染未装/未登录/已登录三态。
  return (
    <>
      {(agents ?? []).map(({ id: p, display_name }) => {
        const payload = payloads.find((x) => x.provider === p) ?? null;
        return (
          <ProviderCard
            key={p}
            provider={p}
            name={display_name}
            installed={installed === null ? null : installed.has(p)}
            payload={payload}
            usage={usageMap[p] ?? null}
            err={errMap[p] ?? null}
            onRefresh={() => doRefresh(p)}
            onInstalled={refreshInstalled}
            onLoggedIn={loadAccounts}
            refreshing={refreshingSet.has(p)}
            settings={settings}
            onToggleQuota={() => toggleQuotaProvider(p)}
          />
        );
      })}
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
  const [agents, setAgents] = useState<AgentDescriptor[]>([]);
  const availAgents = agents.filter((a) => a.installed).map((a) => a.id);
  // dev 下开机自启会注册调试二进制(开机连不上 dev server → 白屏)，故禁用此开关，仅安装版可用。
  const autostartDisabled = import.meta.env.DEV;
  useEffect(() => {
    if (!autostartDisabled) invoke<boolean>("get_autostart").then(setAutostart).catch(() => {});
    availableTerminals().then(setAvailTerms).catch(() => setAvailTerms([]));
  }, [autostartDisabled]);
  useEffect(() => {
    listAgents().then(setAgents).catch(() => {});
  }, []);
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
  // 默认 Agent 下拉：选项以已装 agent 为主；若保存值不在已装列表里（未装/尚未探测完成），
  // 在最前面补一项，避免 Dropdown 内部 find 不到导致按钮标签空白。
  const defaultAgent = settings?.default_agent ?? SETTINGS_DEFAULTS.default_agent;
  const opt = (p: AgentId) => ({ value: p, label: agentName(agents, p) });
  const defaultAgentOptions = availAgents.includes(defaultAgent)
    ? availAgents.map(opt)
    : [opt(defaultAgent), ...availAgents.map(opt)];
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
            <div className="row-label">{t.settings.cardMenu}</div>
            <div className="row-desc">{t.settings.cardMenuDesc}</div>
          </div>
          <Dropdown
            value={settings?.card_menu_mode ?? "button"}
            options={[
              { value: "context" as const, label: t.settings.cardMenuContext },
              { value: "button" as const, label: t.settings.cardMenuButton },
            ]}
            onChange={(v: CardMenuMode) => patch({ card_menu_mode: v })}
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
            <div className="row-label">{t.settings.defaultAgent}</div>
            <div className="row-desc">{t.settings.defaultAgentDesc}</div>
          </div>
          <Dropdown
            value={defaultAgent}
            options={defaultAgentOptions}
            onChange={(v) => patch({ default_agent: v })}
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
          tabIndex={k === value ? 0 : -1}
          key={k}
          className={"swatch" + (k === value ? " sel" : "") + (k === "neutral" ? " swatch-none" : "")}
          style={{ background: STICKER_COLORS[k].swatch }}
          data-tip={names[k] ?? k}
          aria-label={names[k] ?? k}
          onClick={() => onChange(k)}
          onKeyDown={(e) => {
            const handledKeys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", " ", "Enter"];
            if (!handledKeys.includes(e.key)) return;
            e.preventDefault();

            const cur = STICKER_COLOR_KEYS.indexOf(k);
            const next =
              e.key === "Home"
                ? 0
                : e.key === "End"
                  ? STICKER_COLOR_KEYS.length - 1
                  : e.key === "ArrowLeft" || e.key === "ArrowUp"
                    ? (cur - 1 + STICKER_COLOR_KEYS.length) % STICKER_COLOR_KEYS.length
                    : (cur + 1) % STICKER_COLOR_KEYS.length;

            const nextKey = STICKER_COLOR_KEYS[next];
            if (nextKey) onChange(nextKey);

            const radios = Array.from(e.currentTarget.parentElement?.querySelectorAll<HTMLElement>("[role=radio]") ?? []);
            radios[next]?.focus();
          }}
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
const fontSizeOptions = (t: Dict): { value: number; label: string }[] => [
  { value: 90, label: t.settings.fontSizeSmall },
  { value: 100, label: t.settings.fontSizeNormal },
  { value: 112, label: t.settings.fontSizeLarge },
];

// 三等分离散滑块（字体大小 小/中/大）：轨道 + 滑钮 + 底部标签。
function FontSizeSlider({
  value,
  options,
  onChange,
  label,
}: {
  value: number;
  options: { value: number; label: string }[];
  onChange: (v: number) => void;
  label: string;
}) {
  const index = Math.max(0, options.findIndex((o) => o.value === value));
  return (
    <div className="dslider" role="radiogroup" aria-label={label}>
      <div className="dslider-track">
        <div className="dslider-knob-wrap">
          <div className="dslider-knob" style={{ left: `${(index / (options.length - 1)) * 100}%` }} />
        </div>
        {options.map((o) => (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={o.value === value}
            className="dslider-point"
            onClick={() => onChange(o.value)}
          />
        ))}
      </div>
      <div className="dslider-labels">
        {options.map((o) => (
          <span key={o.value} className="dslider-label">{o.label}</span>
        ))}
      </div>
    </div>
  );
}
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
            <div className="row-label">{t.settings.fontSize}</div>
            <div className="row-desc">{t.settings.fontSizeDesc}</div>
          </div>
          <FontSizeSlider value={uiScale} options={fontSizeOptions(t)} onChange={(v) => patch({ ui_scale: v })} label={t.settings.fontSize} />
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
}: {
  status: UpdateStatus;
  newVersion: string | null;
}) {
  const t = useT();
  const [version, setVersion] = useState("");

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  // 「检查更新」与「更新到 vX」都直接打开更新窗口——检查/下载/安装全在那边完成并可视反馈
  // （内联 recheck 在检查失败时界面毫无动静）。本节的后台检查只驱动按钮文案与导航角标。
  // 旧的 trigger-update/update-failed 跨窗口协议已废除：曾因两窗状态分歧把按钮锁死在「更新中…」。
  const openUpdater = () => invoke("open_update_window").catch(() => {});
  const updateBtn =
    status === "available"
      ? { label: t.about.updateTo(newVersion ?? ""), primary: true }
      : { label: t.about.checkUpdate, primary: false };

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
          <button className={"sbtn" + (updateBtn.primary ? " primary" : "")} onClick={openUpdater}>
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
  const { status, version: newVersion } = useUpdate();

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
            <AboutSection status={status} newVersion={newVersion} />
          )}
        </div>
      </main>
    </div>
  );
}
