import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { useFlash } from "../hooks/useFlash";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useAgentListRefresh } from "../useAgents";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { availableTerminals, exportDiagLog, leadWorkflowInstalled, setLeadWorkflow, teamWorkflowState, setTeamWorkflow, type TeamWorkflowStateDto, leadProviderInstallations, setLeadProviderEnabled, type LeadProviderInstallationDto, listAgents, agentName, type AgentId, type AgentDescriptor, type ThemeMode, type ResumeTerminal, type SessionOpenIn, type CardMenuMode, type StickerStyle, type TerminalLineHeight, type ChatContentWidth, type IslandCompactStyle } from "../api";
import { useUpdate, type UpdateStatus } from "../useUpdate";
import { useShowWhenReady } from "../useShowWhenReady";
import { languageOptions, useT } from "../i18n";
import { formatBackendError } from "../i18n/errors";
import { appAlert } from "../confirm";
import logoUrl from "../../src-tauri/icons/128x128.png";
import type { Dict } from "../i18n/zh";
import { SETTINGS_DEFAULTS, useSettingsState } from "./settings/state";
import { Switch, Segmented, SwatchPicker, FontSizeSlider, SettingsError, SavedNote, useSavedFlash } from "./settings/widgets";
import { Dropdown } from "./menu";
import { AccountSection, type AgentView } from "./settings/AccountSection";
import { NotifySection } from "./settings/NotifySection";
import { loadStoredOpts, saveStoredOpts } from "./useNewSessionForm";
import { ProxyRows } from "./settings/NetworkSection";
import { RemoteAccessCard } from "./settings/RemoteAccessCard";
import { CodeHostingSection } from "./settings/ForgeAccountsSection";
import { applySettingsFilter, firstHitControl } from "./settings/searchFilter";
import { useEscClose } from "../hooks/useEscClose";
import { displayKeys, KeyCombos, shortcutGroups } from "./chat/WindowModals";
import { SETTINGS_SECTION_KEY } from "../settingsNav";
import { Icon } from "../icons/Icon";
import { icBell, icClose, icContrast, icSearch, icSettings } from "../icons/glyphs";
import { icChevronRight } from "../icons/glyphs.scm";
import { TeamSection } from "./settings/TeamSection";
import { icBot, icCards, icGitPr, icInfo, icKeyboard, icSmartphone } from "../icons/glyphs.settings";
import { busyProps } from "../busyButton";

const REPO = "github.com/larrygogo/meowo";
const REPO_URL = "https://github.com/larrygogo/meowo";
const SITE = "meowo.io";
const SITE_URL = "https://meowo.io";
const openExt = (url: string) => invoke("open_url", { url }).catch((e) => console.error("[about] 打开链接失败：", url, e));

// 分区按「用户要找的事」分（设置页精简 2026-11-19，对照 macOS 系统设置 / Linear / Raycast 的分组）：
// 「网络」整页只有代理一项，并进通用；「扩展」和账号各有一个同样的 Agent 下拉，并进 Agent 页（id 沿用
// account，外部入口 openSettingsAt("account") 不用改）；四个提醒事件、提示音、任务栏、手机推送原来散在
// 通用 / 外观 / 手机远程三处，集中成「通知」。「手机远程」仍单独成区（对照 Kimi「远程控制」）。
export type Section = "general" | "appearance" | "notify" | "sessions" | "account" | "github" | "remote" | "shortcuts" | "about";
const SECTION_ORDER: Section[] = ["general", "appearance", "notify", "sessions", "account", "github", "remote", "shortcuts", "about"];


// 打开未连接会话用的终端：按平台给不同选项。WKWebView 的 UA 含 "Mac"/"Win"，与 main.tsx 同步判定一致。
const IS_MAC = typeof navigator !== "undefined" && /Mac/i.test(navigator.userAgent);
const IS_WIN = typeof navigator !== "undefined" && /Win/i.test(navigator.userAgent);
const RESUME_TERM_OPTIONS_MAC: { value: ResumeTerminal; label: string }[] = [
  { value: "terminal", label: "Terminal" },
  { value: "iterm", label: "iTerm2" },
  { value: "ghostty", label: "Ghostty" },
];
const resumeTermOptionsWin = (t: Dict): { value: ResumeTerminal; label: string }[] => [
  { value: "wt", label: "Windows Terminal" },
  { value: "wezterm", label: "WezTerm" },
  { value: "powershell", label: "PowerShell" },
  { value: "cmd", label: t.settings.cmdPrompt },
];


function IconGear() {
  return (
    <Icon glyph={icSettings} size={16} />
  );
}
function IconInfo() {
  return (
    <Icon glyph={icInfo} size={16} />
  );
}

// 机器人徽标：这一分区管的是各家 AI Agent，比人像更贴切。
function IconAgent() {
  return (
    <Icon glyph={icBot} size={16} />
  );
}

// 叠放的圆角卡片：会话分区管的是会话与看板卡片。
function IconCards() {
  return (
    <Icon glyph={icCards} size={16} />
  );
}

// 半填充对比圆：外观/主题的经典图标。
function IconAppearance() {
  return (
    <Icon glyph={icContrast} size={16} />
  );
}

// 键盘：快捷键分区。
function IconKeyboard() {
  return (
    <Icon glyph={icKeyboard} size={16} />
  );
}

// 手机：手机远程。
function IconPhone() {
  return (
    <Icon glyph={icSmartphone} size={16} />
  );
}

// 铃铛：通知。
function IconBell() {
  return (
    <Icon glyph={icBell} size={16} />
  );
}

// 拉取请求：代码托管分区（导航键沿用 github）放 Git 程序与提交身份、各平台账号（github.com 与其他平台在同一个列表）。
function IconGithub() {
  return (
    <Icon glyph={icGitPr} size={16} />
  );
}

// 快捷键：与其它分区同一套行式卡片——说明在左、键帽在右，分「窗口」「输入框」两组。
// 此前直接塞进 `?` 速查表的两栏清单（键位右对齐挤成一列、说明跟在后面），在设置页里
// 读起来是倒的（用户实拍「好丑」）。数据与速查表共用 shortcutGroups。
function ShortcutsSection() {
  const t = useT();
  return (
    <>
      {shortcutGroups(t).map((group, index) => (
        // Fragment 而非外包 div：设置搜索要求分组标题与卡片直接挂在分区下（searchFilter）。
        <Fragment key={group.title}>
          <div className={"sec-caption" + (index === 0 ? " is-first" : "")}>{group.title}</div>
          <div className="row-card shortcut-card">
            {group.rows.map(([keys, desc]) => (
              // Esc 一键两用（有审批时拒绝、没有时中断），键位当 key 会撞。
              <div className="row" key={`${keys} ${desc}`}>
                <div className="row-text"><div className="row-label">{desc}</div></div>
                <KeyCombos keys={keys} />
              </div>
            ))}
          </div>
        </Fragment>
      ))}
      <div className="sec-hint">{t.chat.shortcutTerminalNote(displayKeys("Ctrl"), IS_MAC ? "⌃" : "Ctrl")}</div>
    </>
  );
}

// 每个账号读取实际安装状态；磁盘资产存在不能推导自动续跑已可用。
export function LeadProviderRows({ onInstalledChange }: { onInstalledChange?: (installed: boolean) => void } = {}) {
  const t = useT();
  const [rows, setRows] = useState<LeadProviderInstallationDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const busyRef = useRef(false);
  const request = useRef(0);
  const mounted = useRef({ active: false });
  const reload = useCallback(() => {
    if (busyRef.current) return;
    const generation = ++request.current;
    const lifecycle = mounted.current;
    leadProviderInstallations().then((states) => {
      // 只认数组：旧后端或未知返回形状时按「没有」处理，不让设置窗整页渲染崩掉（合并 main 后的搜索测试暴露）。
      if (lifecycle.active && generation === request.current) { setRows(Array.isArray(states) ? states : []); setError(null); }
    }).catch((reason) => {
      if (lifecycle.active && generation === request.current) setError(formatBackendError(reason, t.locale));
    });
  }, [t.locale]);
  useEffect(() => {
    const lifecycle = { active: true };
    mounted.current = lifecycle;
    reload();
    // 本轮闭包持有独立取消标记，语言切换/StrictMode重新挂载不能复活旧请求。
    return () => { lifecycle.active = false; };
  }, [reload]);
  useAgentListRefresh(reload);
  // 切去终端装好 CLI 再切回来是常见用法：窗口聚焦就重查，不用找「刷新」。
  useEffect(() => {
    window.addEventListener("focus", reload);
    return () => window.removeEventListener("focus", reload);
  }, [reload]);
  useEffect(() => { onInstalledChange?.((rows ?? []).some((row) => row.installed)); }, [rows, onInstalledChange]);
  const change = async (row: LeadProviderInstallationDto, enabled: boolean) => {
    if (busyRef.current) return;
    busyRef.current = true;
    request.current++;
    const key = JSON.stringify([row.provider, row.profileId]);
    setBusy(key); setError(null);
    try {
      const updated = await setLeadProviderEnabled(row.provider, row.profileId, enabled);
      if (mounted.current.active) setRows((previous) => previous?.map((item) => item.provider === updated.provider && item.profileId === updated.profileId ? updated : item) ?? [updated]);
    } catch (reason) {
      if (mounted.current.active) setError(formatBackendError(reason, t.locale));
    } finally {
      busyRef.current = false;
      if (mounted.current.active) setBusy(null);
    }
  };
  return <>
    {/* 表头行：「其他 CLI」+ 一句说明 + 刷新；各家是它名下的 .row-sub（设置搜索按组显隐）。 */}
    <div className="row">
      <div className="row-text">
        <div className="row-label">{t.settings.leadProviders}</div>
        <div className="row-desc">{t.settings.leadProvidersDesc}</div>
      </div>
      <button type="button" className="sbtn" {...busyProps(busy !== null, reload)}>{t.settings.leadRefresh}</button>
    </div>
    {rows === null && !error && <div className="row row-sub"><div className="row-text"><div className="row-desc">{t.settings.leadLoading}</div></div></div>}
    {(rows ?? []).map((row) => {
      const key = JSON.stringify([row.provider, row.profileId]);
      const title = row.profileId ? `${row.name} · ${row.profileName ?? row.profileId}` : row.name;
      const status = row.managed ? row.installed ? row.automaticContinue ? t.settings.leadInstalled : t.settings.leadInstalledNoContinue : t.settings.leadNeedsConfiguration : t.settings.leadNotInstalled;
      return <div className="row row-sub" key={key}>
        <div className="row-text"><div className="row-label">{title}</div><div className="row-desc">{status}</div>
          {row.automaticContinue && (row.notes ?? []).map((note) => <div className="row-desc" key={note}>{formatBackendError(note, t.locale)}</div>)}
          {row.reason && <div className="row-desc">{formatBackendError(row.reason, t.locale)}</div>}
        </div>
        {row.managed && <button type="button" className="sbtn" {...busyProps(busy !== null, () => void change(row, false))} aria-label={`${t.settings.leadUninstall} ${title}`}>{t.settings.leadUninstall}</button>}
        {!row.installed && <button type="button" className="sbtn" disabled={!row.configured || !row.supported} {...busyProps(busy !== null, () => void change(row, true))} aria-label={`${t.settings.leadInstall} ${title}`}>{busy === key ? t.settings.leadWorking : row.managed ? t.settings.leadRepair : t.settings.leadInstall}</button>}
      </div>;
    })}
    {/* 放在各家行之后：夹在表头和子行之间会断开设置搜索的分组。 */}
    <SettingsError error={error} onDismiss={() => setError(null)} preformatted />
  </>;
}

// 通用：应用级设置（语言、自启、更新、网络代理、运行、Lead）。通知搬去了「通知」分区，
// 会话与卡片的行为在 SessionsSection。
export function GeneralSection({ searching = false, onOpenTeam }: { searching?: boolean; onOpenTeam?: () => void } = {}) {
  const t = useT();
  const [autostart, setAutostart] = useState(false);
  // autostart 是独立 invoke（不走 patch）：失败曾静默回滚，开关弹回去零解释。
  const [autostartErr, setAutostartErr] = useState(false);
  const [settings, patch, patchError, clearPatchError] = useSettingsState();
  // 代理保存失败（ProxyRows 经 onError 交上来）：和同页其他失败放同一处。
  const [proxyError, setProxyError] = useState<string | null>(null);
  // dev 下开机自启会注册调试二进制(开机连不上 dev server → 白屏)，故禁用此开关，仅安装版可用。
  const autostartDisabled = import.meta.env.DEV;
  useEffect(() => {
    if (!autostartDisabled) invoke<boolean>("get_autostart").then(setAutostart).catch(() => {});
  }, [autostartDisabled]);
  const toggleAutostart = () => {
    if (autostartDisabled) return;
    const next = !autostart;
    setAutostart(next);
    setAutostartErr(false);
    invoke("set_autostart", { enabled: next }).catch(() => {
      setAutostart(!next);
      setAutostartErr(true);
    });
  };
  const autoUpdateOn = settings?.auto_update_enabled ?? true;
  const keepAwakeOn = settings?.keep_awake_while_running ?? true;
  const browserAgentOn = settings?.browser_agent ?? true;
  const leadAwakeOn = settings?.keep_awake_for_lead_gate ?? true;
  const [leadOn, setLeadOn] = useState<boolean | null>(null);
  // 其他 CLI 里有一家装着 Lead 也算「有关卡」：和改前一样，两者都没装时不给「有关卡待拍板时」这一行。
  const [providerLeadOn, setProviderLeadOn] = useState(false);
  const [leadError, setLeadError] = useState<string | null>(null);
  useEffect(() => {
    leadWorkflowInstalled().then(setLeadOn).catch(() => setLeadOn(false));
  }, []);
  const toggleLead = () => {
    if (leadOn === null) return;
    const next = !leadOn;
    setLeadError(null);
    setLeadWorkflow(next)
      .then(() => setLeadOn(next))
      .catch((e) => setLeadError(formatBackendError(e, t.locale)));
  };
  return (
    <>
      {/* 第一张卡是「装好就很少再动」的应用级设置；网络代理原先独占「网络」分区，并进来占一行
          （设置页精简 2026-11-19，对照 macOS 系统设置把网络放进通用这类低频项的做法）。 */}
      <div className="row-card">
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.language}</div>
            <div className="row-desc">{t.settings.languageDesc}</div>
          </div>
          <Dropdown
            value={settings?.language ?? "auto"}
            options={languageOptions(t)}
            onChange={(v) => patch({ language: v })}
          />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.autostart}</div>
            <div className="row-desc">{t.settings.autostartDesc}</div>
          </div>
          <Switch checked={autostart} onChange={toggleAutostart} disabled={autostartDisabled} label={t.settings.autostart} />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.autoUpdate}</div>
            <div className="row-desc">{t.settings.autoUpdateDesc}</div>
          </div>
          <Switch checked={autoUpdateOn} onChange={() => patch({ auto_update_enabled: !autoUpdateOn })} label={t.settings.autoUpdate} />
        </div>
      </div>

      <div className="sec-caption">{t.settings.runGroup}</div>
      <div className="row-card">
        {/* 原「对话窗口功能」总开关（轻量模式）2026-11-20 删除：leader 决定产品只有完整工作台一种形态。 */}
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.browserAgent}</div>
            <div className="row-desc">{t.settings.browserAgentDesc}</div>
          </div>
          <Switch checked={browserAgentOn} onChange={() => patch({ browser_agent: !browserAgentOn })} label={t.settings.browserAgent} />
        </div>
        {/* 「防止电脑休眠」一组（对照 Kimi「防止休眠」，见 power.rs）：此前「运行时防止休眠」在运行组、
            「关卡待拍板时防止休眠」藏在 Lead 组且只在装了 Lead 后才出现，同一件事分两处。合成表头 + 两个子开关，
            字段不变。Linux 后端未实装，不给无效开关。 */}
        {(IS_WIN || IS_MAC) && <>
          <div className="row row-col">
            <div className="row-text">
              <div className="row-label">{t.settings.keepAwakeGroup}</div>
              <div className="row-desc">{t.settings.keepAwakeGroupDesc}</div>
            </div>
          </div>
          <div className="row row-sub">
            <div className="row-text"><div className="row-label">{t.settings.keepAwakeRunning}</div></div>
            <Switch checked={keepAwakeOn} onChange={() => patch({ keep_awake_while_running: !keepAwakeOn })} label={`${t.settings.keepAwakeGroup} · ${t.settings.keepAwakeRunning}`} />
          </div>
          {/* 显示条件与改前一致（main：装了 Lead 才出现）：没装时这一组只剩一个子开关。 */}
          {(leadOn || providerLeadOn) && <div className="row row-sub">
            <div className="row-text"><div className="row-label">{t.settings.keepAwakeLeadGate}</div></div>
            <Switch checked={leadAwakeOn} onChange={() => patch({ keep_awake_for_lead_gate: !leadAwakeOn })} label={`${t.settings.keepAwakeGroup} · ${t.settings.keepAwakeLeadGate}`} />
          </div>}
        </>}
      </div>

      {/* Lead 工作流（2026-11-09）：开关状态以「是否真的装着」为准。Claude Code 是开关，其他 CLI 一家一行
          装 / 卸，表头行带「刷新」。此前那一行只有一段说明和一颗孤零零的「刷新」，看不出是在管什么。 */}
      <div className="sec-caption">{t.settings.leadGroup}</div>
      <div className="row-card">
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.leadWorkflow}</div>
            <div className="row-desc">{t.settings.leadWorkflowDesc}</div>
            {leadError && <div className="row-desc is-error" role="alert">{leadError}</div>}
          </div>
          <Switch checked={!!leadOn} onChange={toggleLead} label={t.settings.leadWorkflow} />
        </div>
        <TeamRow leadOn={leadOn} onOpenTeam={onOpenTeam} />
        <LeadProviderRows onInstalledChange={setProviderLeadOn} />
      </div>

      {/* 网络代理放在通用页最后（审查 2026-11-19）：首屏留给语言、自启、更新和「运行」组；
          代理是装好后很少再动的一项，自定义地址和各 Agent 的单独设置都挂在它名下。 */}
      <div className="sec-caption">{t.settings.networkGroup}</div>
      <div className="row-card">
        <ProxyRows expandAll={searching} onError={setProxyError} />
      </div>
      {/* T-6（2026-11-12 走查）：此前是单独一套红字、没有 ×，与下一行 SettingsError 两种样式（7S-7 漏的一处）。
          失败时开关已弹回原值，「重试」就是再拨一次。 */}
      <SettingsError
        preformatted
        error={autostartErr ? t.settings.autostartFailed : null}
        action={{ label: t.settings.retry, onClick: toggleAutostart }}
        onDismiss={() => setAutostartErr(false)}
      />
      <SettingsError preformatted error={patchError} onDismiss={clearPatchError} />
      <SettingsError preformatted error={proxyError} onDismiss={() => setProxyError(null)} />
    </>
  );
}

/// 团队一行（team.rs）：Lead 的扩展，Lead 关着时不可用；关掉 Lead 时后端会把团队一起卸掉，所以跟着 leadOn 重读。
/// 只有开关，没有按 CLI 分行：第一版只支持 Claude Code（团队规则 R12），说明里写明。
function TeamRow({ leadOn, onOpenTeam }: { leadOn: boolean | null; onOpenTeam?: () => void }) {
  const t = useT();
  const [state, setState] = useState<TeamWorkflowStateDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    teamWorkflowState().then(setState).catch(() => setState(null));
  }, [leadOn]);
  const toggle = () => {
    if (!state) return;
    setError(null);
    setTeamWorkflow(!state.installed)
      .catch((e) => setError(formatBackendError(e, t.locale)))
      .finally(() => teamWorkflowState().then(setState).catch(() => {}));
  };
  const blocked = !state?.leadInstalled;
  // 另一份 team 插件在加载时开关点了也只会被后端拒绝，提示已经写在行里：直接置灰，免得同一句话再以报错出现一次。
  const conflict = !!state?.conflict && !state.installed;
  return (
    <>
    <div className="row row-sub">
      <div className="row-text">
        <div className="row-label">{t.settings.team}</div>
        <div className="row-desc">{t.settings.teamDesc}</div>
        {state && blocked && <div className="row-desc">{t.settings.teamNeedsLead}</div>}
        {/* 路径是用户要去改的东西，不能被三行钳制吃掉（验收台截图里被截成「CLAUDE_CODE_PLUGIN_DIR…」）。 */}
        {state?.conflict && <div className="row-desc is-error is-full" role="alert">{t.settings.teamConflict(state.conflict)}</div>}
        {state?.personalConfigError && <div className="row-desc is-error is-full" role="alert">{t.settings.teamConfigBroken(state.personalConfigError)}</div>}
        {!!state?.blockedFiles?.length && <div className="row-desc is-error is-full">{t.settings.teamBlocked(state.blockedFiles!.join("、"))}</div>}
        {error && <div className="row-desc is-error" role="alert">{error}</div>}
      </div>
      <Switch checked={!!state?.installed} onChange={toggle} disabled={!state || conflict || (blocked && !state.installed)} label={t.settings.team} />
    </div>
    {/* 团队设置子页入口（设计稿 designer-1 entry-dark）：装好之后才有，从右边推入、左上返回。 */}
    {state?.installed && onOpenTeam && (
      <button type="button" className="row row-sub row-link" onClick={onOpenTeam}>
        <div className="row-text">
          <div className="row-label">{t.settings.teamSettings}</div>
          <div className="row-desc">{t.settings.teamSettingsDesc}</div>
        </div>
        <Icon glyph={icChevronRight} size={14} />
      </button>
    )}
    </>
  );
}

// 会话：新建/打开/归档等会话行为，外加看板卡片的展示与交互（独立成第二张卡）。
/// 默认 Agent 的启动选项默认值（与新建会话的选项记忆同一份，见 useNewSessionForm）。
function LaunchDefaultRows({ agent }: { agent: AgentDescriptor }) {
  const t = useT();
  const [stored, setStored] = useState(() => loadStoredOpts()[agent.id] ?? {});
  useEffect(() => setStored(loadStoredOpts()[agent.id] ?? {}), [agent.id]);
  return (
    <>
      {(agent.launch_options ?? []).map((option) => {
        const optionLabel = t.newSession.launchOption[option.id] ?? option.id;
        return (
          <div className="row" key={option.id}>
            <div className="row-text">
              <div className="row-label">{t.newSession.defaultOf(optionLabel)}</div>
              <div className="row-desc">{t.settings.launchDefaultDesc(agent.display_name)}</div>
            </div>
            <Dropdown
              value={stored[option.id] ?? option.default}
              options={option.choices.map((choice) => ({
                value: choice.id,
                label: t.newSession.launchChoice[`${option.id}.${choice.id}`] ?? choice.label,
                risky: choice.risk,
                sub: t.newSession.launchChoiceDesc[`${option.id}.${choice.id}`]
                  ?? (choice.risk ? t.newSession.riskyChoiceSub : undefined),
              }))}
              onChange={(value) => {
                const next = { ...stored, [option.id]: value };
                setStored(next);
                saveStoredOpts(agent.id, next);
              }}
            />
          </div>
        );
      })}
    </>
  );
}

export function SessionsSection() {
  const t = useT();
  const [settings, patch, patchError, clearPatchError] = useSettingsState();
  const [availTerms, setAvailTerms] = useState<ResumeTerminal[] | null>(null);
  const [agents, setAgents] = useState<AgentDescriptor[]>([]);
  const availAgents = agents.filter((a) => a.installed).map((a) => a.id);
  useEffect(() => {
    availableTerminals().then(setAvailTerms).catch(() => setAvailTerms([]));
  }, []);
  const reloadAgents = () => {
    listAgents().then(setAgents).catch(() => {});
  };
  // worktree 分支前缀：本地草稿，磁盘值变了（首帧拉回 / 别处改动 / 保存成功）再同步。
  const diskPrefix = settings?.worktree?.branch_prefix ?? "";
  const [prefixDraft, setPrefixDraft] = useState(diskPrefix);
  useEffect(() => setPrefixDraft(diskPrefix), [diskPrefix]);
  const prefixSaved = useSavedFlash();
  const commitPrefix = (raw: string) => {
    const value = raw.trim();
    if (value === diskPrefix) return;
    void patch({ worktree: { location: settings?.worktree?.location ?? "sibling", branch_prefix: value || null, trusted: settings?.worktree?.trusted ?? {} } })
      .then((err) => { if (!err) prefixSaved.flash(); });
  };
  useEffect(reloadAgents, []);
  useAgentListRefresh(reloadAgents); // 装完新 agent 立刻反映
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
            <div className="row-label">{t.settings.defaultAgent}</div>
            <div className="row-desc">{t.settings.defaultAgentDesc}</div>
          </div>
          <Dropdown
            value={defaultAgent}
            options={defaultAgentOptions}
            onChange={(v) => patch({ default_agent: v })}
          />
        </div>
        {/* 新建会话的默认权限 / 模型（对照 Kimi「Agent 默认值」）：此前只在新建时悄悄记住上一次
            的选择，设置里看不到也改不了。按默认 Agent 声明的启动选项逐项给。 */}
        {(() => {
          const descriptor = agents.find((agent) => agent.id === defaultAgent);
          return descriptor ? <LaunchDefaultRows agent={descriptor} /> : null;
        })()}
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.sessionOpenIn}</div>
            <div className="row-desc">{t.settings.sessionOpenInDesc}</div>
          </div>
          <Dropdown
            value={settings?.session_open_in ?? "terminal"}
            options={[
              { value: "chat" as const, label: t.settings.sessionOpenInChat },
              { value: "terminal" as const, label: t.settings.sessionOpenInTerminal },
            ]}
            onChange={(v: SessionOpenIn) => patch({ session_open_in: v })}
          />
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

      {/* 包 5：「在新 worktree 里开」建在哪、分支名用什么开头——两项管同一件事，成一组（设置页精简 2026-11-19）。 */}
      <div className="sec-caption">{t.settings.worktreeGroup}</div>
      <div className="row-card">
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.wtnew.settingsLocation}</div>
            <div className="row-desc">{t.wtnew.settingsLocationDesc}</div>
          </div>
          <Dropdown
            value={settings?.worktree?.location === "meowo" ? "meowo" : "sibling"}
            options={[
              { value: "sibling", label: t.wtnew.locationSibling },
              { value: "meowo", label: t.wtnew.locationMeowo },
            ]}
            onChange={(v: string) => patch({ worktree: { location: v, branch_prefix: settings?.worktree?.branch_prefix ?? null, trusted: settings?.worktree?.trusted ?? {} } })}
          />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.wtnew.settingsPrefix}</div>
            <div className="row-desc">{t.wtnew.settingsPrefixDesc}</div>
          </div>
          <input
            // 受控草稿（不再用 key 重挂）：此前靠「值变了就换 key」跟上异步拉回的设置，可保存成功
            // 也会换 key，回车提交后输入框整个重挂，焦点掉到 body（T-5）。
            className="ns-input wt-prefix-input"
            aria-label={t.wtnew.settingsPrefix}
            value={prefixDraft}
            placeholder="meowo/"
            onChange={(e) => { setPrefixDraft(e.target.value); prefixSaved.hide(); }}
            onBlur={(e) => commitPrefix(e.currentTarget.value)}
            // T-5（2026-11-12 走查）：此前只在失焦时保存，回车毫无反应，用户分不清改动生效没有。
            // 回车同样提交、焦点留在框里。
            onKeyDown={(e) => { if (e.key === "Enter") commitPrefix(e.currentTarget.value); }}
          />
          <SavedNote shown={prefixSaved.shown} fx={prefixSaved.fx} />
        </div>
      </div>

      {/* 「列表」组：原「会话列表」（只剩会话菜单一行）和「已完成会话」（只有去会话管理一行）两个单行分组合并。
          「对话预览」「打开方式」只有已下线的贴纸在读（UI 调研 shell #2），关卡 A 定案删掉，字段留在 settings.json 照常读写。
          已完成会话只给一个入口，去会话管理页（预选「已完成」）——那里能按工作区、时间筛，批量恢复。
          轻量模式下的设置页内列表（ArchivedSessions）随 2026-11-20 去掉轻量模式删除。 */}
      <div className="sec-caption">{t.settings.listGroup}</div>
      <div className="row-card">
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
            <div className="row-label">{t.settings.archivedSessions}</div>
            <div className="row-desc">{t.settings.archivedInAdminDesc}</div>
          </div>
          <button type="button" className="sbtn" onClick={() => void invoke("open_session_admin").catch(() => {})}>
            {t.settings.archivedOpenAdmin}
          </button>
        </div>
      </div>
      <SettingsError preformatted error={patchError} onDismiss={clearPatchError} />
    </>
  );
}

const themeOptions = (t: Dict): { value: ThemeMode; label: string }[] => [
  { value: "dark", label: t.settings.themeDark },
  { value: "light", label: t.settings.themeLight },
  { value: "system", label: t.settings.themeSystem },
];
const ISLAND_COMPACT_STYLES: IslandCompactStyle[] = ["counts", "focus", "minimal"];
const ISLAND_ALERT_SECONDS = [4, 6, 10];

const stickerStyleOptions = (t: Dict): { value: StickerStyle; label: string }[] => [
  { value: "elevated", label: t.settings.styleElevated },
  { value: "flat", label: t.settings.styleFlat },
];
const fontSizeOptions = (t: Dict): { value: number; label: string }[] => [
  { value: 100, label: t.settings.fontSizeSmall },
  { value: 112, label: t.settings.fontSizeNormal },
  { value: 124, label: t.settings.fontSizeLarge },
];

const OPACITY_MIN = 25;
const OPACITY_MAX = 100;

const TERM_FONT_MIN = 10;
const TERM_FONT_MAX = 18;
// 回滚缓冲滑杆范围（后端兜底钳到 500–50000，见 settings.rs）。
const TERM_SCROLLBACK_MIN = 1000;
const TERM_SCROLLBACK_MAX = 20000;
const lineHeightOptions = (t: Dict): { value: TerminalLineHeight; label: string }[] => [
  { value: "compact", label: t.settings.lineCompact },
  { value: "normal", label: t.settings.lineNormal },
  { value: "relaxed", label: t.settings.lineRelaxed },
];
const chatWidthOptions = (t: Dict): { value: ChatContentWidth; label: string }[] => [
  { value: "fixed", label: t.settings.chatWidthFixed },
  { value: "full", label: t.settings.chatWidthFull },
];

/** 滑杆草稿：拖动期间只更新本地显示，松手（pointerup）或键盘调节静默 240ms 后才提交。
 *  逐像素 onChange 直发 patch = 每 px 一轮完整的 set_settings 链（全量写盘 + 读写 claude
 *  配置文件 + proxy-applied/settings-changed 双广播），拖一次滑杆等于几十轮流程，低端机
 *  与杀软环境下可感知卡顿。 */
function useSliderDraft(commit: (v: number) => void) {
  const [draft, setDraft] = useState<number | null>(null);
  const timer = useRef<number | undefined>(undefined);
  // 待提交的草稿值。键盘方向键路径没有 pointerup,靠 240ms 兜底 timer 提交——若在兜底
  // 落地前整窗卸载(Esc 关设置页),clearTimeout 会把这次调节静默丢掉。卸载时有 pending
  // 就立即 flush,commit 经 ref 取最新闭包。
  const pendingRef = useRef<number | null>(null);
  const commitRef = useRef(commit);
  commitRef.current = commit;
  useEffect(() => () => {
    window.clearTimeout(timer.current);
    if (pendingRef.current != null) {
      const v = pendingRef.current;
      pendingRef.current = null;
      commitRef.current(v);
    }
  }, []);
  const change = (v: number) => {
    setDraft(v);
    pendingRef.current = v;
    window.clearTimeout(timer.current);
    // 键盘方向键调节没有 pointerup：静默 240ms 后兜底提交。
    timer.current = window.setTimeout(() => { pendingRef.current = null; commit(v); setDraft(null); }, 240);
  };
  const release = (v: number) => {
    window.clearTimeout(timer.current);
    pendingRef.current = null;
    commit(v);
    setDraft(null);
  };
  return { draft, change, release };
}

function AppearanceSection() {
  const t = useT();
  const [settings, patch, patchError, clearPatchError] = useSettingsState();
  // 占位一律读 SETTINGS_DEFAULTS：曾在此手写 `?? 94` / `?? "elevated"` 字面量，与真实默认
  // （settings.rs：100 / "flat"）不符——设置页每次打开都先高亮错误档位再跳正确值。
  const theme = settings?.theme ?? SETTINGS_DEFAULTS.theme;
  const opacity = settings?.opacity ?? SETTINGS_DEFAULTS.opacity;
  const uiScale = settings?.ui_scale ?? SETTINGS_DEFAULTS.ui_scale;
  const stickerStyle = settings?.sticker_style ?? SETTINGS_DEFAULTS.sticker_style;
  const islandCompact = settings?.island_compact ?? SETTINGS_DEFAULTS.island_compact;
  const islandStrip = settings?.island_strip ?? SETTINGS_DEFAULTS.island_strip;
  const islandAutoStrip = settings?.island_auto_strip ?? SETTINGS_DEFAULTS.island_auto_strip;
  const islandAlertSeconds = settings?.island_alert_seconds ?? SETTINGS_DEFAULTS.island_alert_seconds;
  const stickerColor = settings?.sticker_color ?? SETTINGS_DEFAULTS.sticker_color;
  const termFont = settings?.terminal_font_size ?? SETTINGS_DEFAULTS.terminal_font_size;
  const termLine = settings?.terminal_line_height ?? SETTINGS_DEFAULTS.terminal_line_height;
  const termBack = settings?.terminal_scrollback ?? SETTINGS_DEFAULTS.terminal_scrollback;
  const chatWidth = settings?.chat_content_width ?? SETTINGS_DEFAULTS.chat_content_width;
  const turnFolding = settings?.chat_turn_folding ?? SETTINGS_DEFAULTS.chat_turn_folding;
  // 滑杆草稿（拖动中本地显示，松手才提交）：见 useSliderDraft。
  const opacityDraft = useSliderDraft((v) => void patch({ opacity: v }));
  const termFontDraft = useSliderDraft((v) => void patch({ terminal_font_size: v }));
  const termBackDraft = useSliderDraft((v) => void patch({ terminal_scrollback: v }));
  const opacityShown = opacityDraft.draft ?? opacity;
  const termFontShown = termFontDraft.draft ?? termFont;
  const termBackShown = termBackDraft.draft ?? termBack;
  // 钳到 [0,100]：手改 settings.json 为越界值时，避免算出负/超界的 linear-gradient 填充宽度。
  const fill = Math.max(0, Math.min(100, ((opacityShown - OPACITY_MIN) / (OPACITY_MAX - OPACITY_MIN)) * 100));
  const termFill = Math.max(0, Math.min(100, ((termFontShown - TERM_FONT_MIN) / (TERM_FONT_MAX - TERM_FONT_MIN)) * 100));
  const backFill = Math.max(0, Math.min(100, ((termBackShown - TERM_SCROLLBACK_MIN) / (TERM_SCROLLBACK_MAX - TERM_SCROLLBACK_MIN)) * 100));
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
            <div className="row-label">{t.settings.chatWidth}</div>
            <div className="row-desc">{t.settings.chatWidthDesc}</div>
          </div>
          <Segmented value={chatWidth} options={chatWidthOptions(t)} onChange={(v) => patch({ chat_content_width: v })} label={t.settings.chatWidth} />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.turnFolding}</div>
            <div className="row-desc">{t.settings.turnFoldingDesc}</div>
          </div>
          <Switch checked={turnFolding} onChange={() => patch({ chat_turn_folding: !turnFolding })} label={t.settings.turnFolding} />
        </div>
      </div>

      {/* 贴纸专属的一组（风格、配色、不透明度）随贴纸一起删了：Windows 2026-11-05 先删，macOS
          同日也换成灵动岛。settings 字段留着只为读旧配置不报错。 */}

      {/* 灵动岛：Windows 2026-11-04 起、macOS 2026-11-05 起（替换贴纸）。极简模式只有 Windows 有：
          macOS 有刘海时刘海本身就是那一条，没刘海时岛平时藏在菜单栏图标里。 */}
      {(IS_WIN || IS_MAC) && <>
        <div className="sec-caption">{t.settings.islandGroup}</div>
        <div className="row-card">
          {IS_WIN && <div className="row">
            <div className="row-text">
              <div className="row-label">{t.settings.islandStrip}</div>
              <div className="row-desc">{t.settings.islandStripDesc}</div>
            </div>
            <Switch checked={islandStrip} onChange={() => patch({ island_strip: !islandStrip })} label={t.settings.islandStrip} />
          </div>}
          {/* 最大化窗口在前台时自动收细条（关卡 A 定案，i6）。只有 Windows：macOS 岛钉在刘海/菜单栏，不挡标题栏。 */}
          {IS_WIN && <div className="row">
            <div className="row-text">
              <div className="row-label">{t.settings.islandAutoStrip}</div>
              <div className="row-desc">{t.settings.islandAutoStripDesc}</div>
            </div>
            <Switch checked={islandAutoStrip} onChange={() => patch({ island_auto_strip: !islandAutoStrip })} label={t.settings.islandAutoStrip} />
          </div>}
          <div className="row">
            <div className="row-text">
              <div className="row-label">{t.settings.islandCompact}</div>
              <div className="row-desc">{t.settings.islandCompactDesc}</div>
            </div>
            <Segmented
              value={islandCompact}
              options={ISLAND_COMPACT_STYLES.map((value) => ({ value, label: t.settings.islandCompactOptions[value] }))}
              onChange={(v) => patch({ island_compact: v })}
              label={t.settings.islandCompact}
            />
          </div>
          <div className="row">
            <div className="row-text">
              <div className="row-label">{t.settings.islandAlertSeconds}</div>
              <div className="row-desc">{t.settings.islandAlertSecondsDesc}</div>
            </div>
            <Segmented
              value={islandAlertSeconds}
              options={ISLAND_ALERT_SECONDS.map((value) => ({ value, label: t.settings.islandSeconds(value) }))}
              onChange={(v) => patch({ island_alert_seconds: v })}
              label={t.settings.islandAlertSeconds}
            />
          </div>
          {/* 「自动弹出提醒」四个事件搬去「通知」页的矩阵（灵动岛弹出那一列，字段仍是 island_alerts）；
              「提醒停留」管的是岛自己的样子，留在这里。 */}
        </div>
      </>}

      <div className="sec-caption">{t.settings.terminalGroup}</div>
      <div className="row-card">
        <div className="row row-col">
          <div className="row-head">
            <div className="row-text">
              <div className="row-label">{t.settings.termFontSize}</div>
              <div className="row-desc">{t.settings.termFontSizeDesc}</div>
            </div>
            <span className="row-val">{termFontShown}px</span>
          </div>
          <input
            type="range"
            className="slider"
            min={TERM_FONT_MIN}
            max={TERM_FONT_MAX}
            value={termFontShown}
            style={{ background: `linear-gradient(90deg, var(--cc-accent) ${termFill}%, var(--cc-border) ${termFill}%)` }}
            onChange={(e) => termFontDraft.change(Number(e.target.value))}
            onPointerUp={(e) => termFontDraft.release(Number((e.target as HTMLInputElement).value))}
            aria-label={t.settings.termFontSize}
          />
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.termLineHeight}</div>
            <div className="row-desc">{t.settings.termLineHeightDesc}</div>
          </div>
          <Segmented value={termLine} options={lineHeightOptions(t)} onChange={(v) => patch({ terminal_line_height: v })} label={t.settings.termLineHeight} />
        </div>
        <div className="row row-col">
          <div className="row-head">
            <div className="row-text">
              <div className="row-label">{t.settings.termScrollback}</div>
              <div className="row-desc">{t.settings.termScrollbackDesc}</div>
            </div>
            <span className="row-val">{termBackShown}</span>
          </div>
          <input
            type="range"
            className="slider"
            min={TERM_SCROLLBACK_MIN}
            max={TERM_SCROLLBACK_MAX}
            step={500}
            value={termBackShown}
            style={{ background: `linear-gradient(90deg, var(--cc-accent) ${backFill}%, var(--cc-border) ${backFill}%)` }}
            onChange={(e) => termBackDraft.change(Number(e.target.value))}
            onPointerUp={(e) => termBackDraft.release(Number((e.target as HTMLInputElement).value))}
            aria-label={t.settings.termScrollback}
          />
        </div>
      </div>
      {/* 页脚「更改即时生效」删了：拨了就变，界面自己已经说明了（设置页精简 2026-11-19）。 */}
      <SettingsError preformatted error={patchError} onDismiss={clearPatchError} />
    </>
  );
}

export function AboutSection({
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
  const openUpdater = () => invoke("open_update_window").catch((e) => {
    // 点了「检查更新」毫无反应是最糟的失败：console 留线索 + 兜底错误框（S-14 范式）。
    console.error("[about] 打开更新窗口失败：", e);
    void appAlert(formatBackendError(e, t.locale), { title: t.about.checkUpdate });
  });
  const hasUpdate = status === "available" || status === "downloading" || status === "ready";
  // 7S-3：三个态此前都压成一句「发现新版本」+ 一颗「更新到 vX」——正在下载和已经下完
  // 是用户要做的事完全不同的两件事（等 vs 点）。
  const updateBtn =
    status === "ready" ? { label: t.about.installUpdate, primary: true }
      : hasUpdate ? { label: t.about.updateTo(newVersion ?? ""), primary: true }
      : { label: t.about.checkUpdate, primary: false };

  // 诊断日志导出：保存框选位置，成功后按钮短暂显示「已导出」；失败走错误框（取消保存框不算失败）。
  const { shown: diagExported, flash: flashDiagExported, fx: diagFx } = useFlash();
  const exportDiag = async () => {
    const stamp = new Date().toISOString().slice(0, 10);
    const path = await saveDialog({ defaultPath: `meowo-diag-${stamp}.txt`, filters: [{ name: "Text", extensions: ["txt", "log"] }] }).catch(() => null);
    if (!path) return;
    try {
      await exportDiagLog(path);
      flashDiagExported();
    } catch (e) {
      void appAlert(formatBackendError(e, t.locale), { title: t.about.diagLog });
    }
  };

  const verText = `v${version || "—"}`;
  // checking/error 也要有话说：曾经两态都落空串——打开「关于」十秒空窗、检查失败一片安静。
  // unknown（关了自动更新、从未检查）只显示版本号，不谎报「已是最新」。
  const verStatus =
    status === "downloading" ? t.about.downloadingUpdate
      : status === "ready" ? t.about.updateReady
      : hasUpdate ? t.about.foundNew(newVersion ?? "")
      : status === "latest" ? t.about.upToDate
      : status === "checking" ? t.about.checking
      : status === "error" ? t.about.checkFailed
      : "";
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
          <button className={"sbtn" + (updateBtn.primary ? " btn-primary" : "")} onClick={openUpdater}>
            {updateBtn.label}
          </button>
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.about.website}</div>
            <div className="row-desc">{SITE}</div>
          </div>
          {/* 7S-9：官网原是本页唯一的 primary——有更新时页面上就有两颗主按钮抢视线，
              而「装更新」才是那一刻真正要做的事。官网降回普通按钮。 */}
          <button className="sbtn" onClick={() => openExt(SITE_URL)}>
            {t.about.open}
          </button>
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.about.source}</div>
            <div className="row-desc">{REPO}</div>
          </div>
          <button className="sbtn" onClick={() => openExt(REPO_URL)}>
            {t.about.open}
          </button>
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.about.guide}</div>
            {/* 7S-9：副标此前直接借用引导页大标题「欢迎使用 Meowo」——在「关于」页里
                读起来像在欢迎一个已经用了很久的人。说清这颗按钮会干什么。 */}
            <div className="row-desc">{t.about.guideDesc}</div>
          </div>
          <button className="sbtn" onClick={() => void invoke("open_onboarding").catch((e) => console.error("[about] 打开引导窗口失败：", e))}>
            {t.about.open}
          </button>
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.about.diagLog}</div>
            <div className="row-desc">{t.about.diagLogDesc}</div>
          </div>
          <button className={"sbtn fb-press" + diagFx} onClick={() => void exportDiag()}>
            {diagExported ? t.about.diagLogExported : t.about.diagLogExport}
          </button>
        </div>
      </div>

      <p className="about-blurb">{t.about.blurb}</p>

      <div className="about-foot">
        {/* 链接外观（去按钮默认样式、强调色、浅色主题的青柠下划线）在 styles.css 的 `.about-foot button`。 */}
        <button
          type="button"
          onClick={() => openExt(REPO_URL + "/issues")}
        >
          {t.about.feedback}
        </button>
        <span className="dot" aria-hidden="true" />
        <button
          type="button"
          onClick={() => openExt(REPO_URL + "/releases")}
        >
          {t.about.changelog}
        </button>
        <div className="copy">MIT License · © 2026 larrygogo</div>
      </div>
    </>
  );
}

/** 通用页里看哪一层：主列表，或「团队设置」子页（team_personal.rs）。和 Agent 页的分段一样记在分区键里。 */
type GeneralView = "main" | "team";

const NAV_ICONS: Record<Section, () => React.JSX.Element> = {
  general: IconGear, appearance: IconAppearance, notify: IconBell, sessions: IconCards, account: IconAgent,
  github: IconGithub, remote: IconPhone, shortcuts: IconKeyboard, about: IconInfo,
};

/** 记住的 / 外部入口给的分区 id → 新结构里的位置。旧 id 不能落空白页：「网络」并进了通用，
 *  「扩展」成了 Agent 页的一段（localStorage 里可能还存着改版前停留的分区）。 */
export function resolveSection(raw: string | null): { sec: Section; agentView?: AgentView; generalView?: GeneralView } | null {
  switch (raw) {
    case "team": return { sec: "general", generalView: "team" };
    case "general": case "appearance": case "notify": case "sessions": case "github": case "remote": case "shortcuts": case "about":
      return { sec: raw };
    case "account": return { sec: "account", agentView: "account" };
    case "extensions": return { sec: "account", agentView: "extensions" };
    case "network": return { sec: "general" };
    default: return null;
  }
}

export function About() {
  const t = useT();
  // 窗口以 visible:false 创建（window.rs），首帧渲染后再显示，消除打开瞬间的白框闪烁。
  useShowWhenReady();
  // 分区记忆：每次开窗回 general 让最常改的分区（外观/Agent）永远多点一次。
  const [initial] = useState(() => {
    try { return resolveSection(localStorage.getItem(SETTINGS_SECTION_KEY)); } catch { return null; }
  });
  const [sec, setSec] = useState<Section>(initial?.sec ?? "general");
  // Agent 页当前看哪一段。记在同一个分区键里（extensions = Agent 页的扩展段），重开窗口回到原处。
  const [agentView, setAgentView] = useState<AgentView>(initial?.agentView ?? "account");
  const [generalView, setGeneralView] = useState<GeneralView>(initial?.generalView ?? "main");
  const pickGeneralView = (view: GeneralView) => {
    setGeneralView(view);
    try { localStorage.setItem(SETTINGS_SECTION_KEY, view === "team" ? "team" : "general"); } catch { /* 隐私模式禁写 */ }
  };
  // 记住的是团队子页，但团队已经关了（或在别处卸了）：回到通用主页，不停在一个没有入口的页面上。
  useEffect(() => {
    if (generalView !== "team") return;
    let live = true;
    teamWorkflowState()
      .then((s) => { if (live && !s.installed) pickGeneralView("main"); })
      .catch(() => {});
    return () => { live = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- pickGeneralView 每次渲染新建，只看子页切换
  }, [generalView]);
  const pickAgentView = (view: AgentView) => {
    setAgentView(view);
    try { localStorage.setItem(SETTINGS_SECTION_KEY, view === "extensions" ? "extensions" : "account"); } catch { /* 隐私模式禁写 */ }
  };
  // S-2：访问过的分区保持挂载（hidden 隐藏代替卸载）——此前 main-body 带 key={sec} 整树
  // 重挂，每次切回账号分区都重发联网配额查询、切回会话分区重拉归档首页。未访问的分区
  // 仍不挂载：开窗就把配额/agent 名单全查一遍是白付的启动成本。
  const [visited, setVisited] = useState<ReadonlySet<Section>>(() => new Set([sec]));
  const pickSec = (next: Section) => {
    setSec(next);
    setVisited((prev) => (prev.has(next) ? prev : new Set(prev).add(next)));
    const stored = next === "account" && agentView === "extensions" ? "extensions" : next === "general" && generalView === "team" ? "team" : next;
    try { localStorage.setItem(SETTINGS_SECTION_KEY, stored); } catch { /* 隐私模式禁写 */ }
  };
  // 别的窗口经 openSettingsAt 指定分区时，设置窗若已开着只会被聚焦——跟着 storage 事件切过去
  // （出错卡的「去登录」要直达账号页，落在上次停留的分区等于让人再找一遍）。
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key !== SETTINGS_SECTION_KEY) return;
      const next = resolveSection(event.newValue);
      if (next) {
        setSec(next.sec);
        if (next.agentView) setAgentView(next.agentView);
        setGeneralView(next.generalView ?? "main");
        setVisited((prev) => (prev.has(next.sec) ? prev : new Set(prev).add(next.sec)));
      }
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);
  const [query, setQuery] = useState("");
  const searching = query.trim() !== "";
  const close = () => getCurrentWindow().close().catch(() => {});
  // 弹出式任务窗的基本预期：Esc 关窗（输入框内让位）。在子页里先返回上一层，和搜索框里 Esc 先清词同一个思路。
  const inTeamPage = sec === "general" && generalView === "team" && !searching;
  useEscClose(() => (inTeamPage ? pickGeneralView("main") : close()));
  // 设置窗口也服从自动更新开关；关闭时不做后台检查，用户仍可从「关于」手动打开更新窗口检查。
  const { status, version: newVersion } = useUpdate({ automatic: true });

  // S-1 设置搜索：30+ 设置项零搜索曾让「在贴纸显示配额」这类深埋项无从找起。
  // 非空查询时挂载全部分区、按行文本过滤（逻辑在 settings/searchFilter.ts，可单测）；
  // MutationObserver 兜底异步内容（agent 名单/配额加载完成后新出现的行也要被过滤）。
  const navOn = (s: Section) => !searching && sec === s;
  // 7B-7：设置窗此前没有 Ctrl+F（贴纸窗有），搜索框只能用鼠标点到。
  const searchInputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey || e.code !== "KeyF") return;
      e.preventDefault();
      searchInputRef.current?.focus();
      searchInputRef.current?.select();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  const [noHits, setNoHits] = useState(false);
  const bodyRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const body = bodyRef.current;
    if (!body) return;
    const navLabels: Record<Section, string> = {
      general: t.settings.nav.general,
      appearance: t.settings.nav.appearance,
      notify: `${t.settings.nav.notify} ${t.settings.navSearchExtra.notify}`,
      sessions: t.settings.nav.sessions,
      account: `${t.settings.nav.account} ${t.settings.navSearchExtra.account}`,
      github: `${t.settings.nav.github} ${t.settings.navSearchExtra.github}`,
      remote: t.settings.nav.remote,
      shortcuts: `${t.settings.nav.shortcuts} ${t.settings.navSearchExtra.shortcuts}`,
      about: t.settings.nav.about,
    };
    const apply = () => {
      const anyVisible = applySettingsFilter(body, query, navLabels);
      setNoHits(query.trim() !== "" && !anyVisible);
    };
    apply();
    if (!searching) return;
    const observer = new MutationObserver(apply);
    observer.observe(body, { childList: true, subtree: true, characterData: true });
    return () => observer.disconnect();
  }, [query, searching, t]);

  return (
    <div className="settings">
      <aside className="side">
        <div className="side-top" data-tauri-drag-region />
        {/* S-1：设置搜索入口。命中过滤分区与行（见 settings/searchFilter.ts）；
            输入框里的 Esc 先清词——useEscClose 对输入框让位，不会误关窗。 */}
        <div className="side-search">
          <Icon glyph={icSearch} size={12} />
          <input
            ref={searchInputRef}
            value={query}
            placeholder={t.settings.searchPlaceholder}
            aria-label={t.settings.searchPlaceholder}
            onChange={(e) => {
              // IME 组词中的中间态（拼音串）不触发过滤：未提交的组词会把设置行滤没。
              // 受控输入在 composition 期间不会被 React 回填，组词过程不受影响。
              if ((e.nativeEvent as InputEvent).isComposing) return;
              setQuery(e.target.value);
            }}
            onKeyDown={(e) => {
              // T-4（2026-11-12）：Enter / ↓ 直接进第一条命中行的控件，不用 Tab 穿过整排导航。
              if ((e.key === "Enter" || e.key === "ArrowDown") && searching && !(e.nativeEvent as KeyboardEvent).isComposing) {
                const target = bodyRef.current ? firstHitControl(bodyRef.current) : null;
                if (target) { e.preventDefault(); target.focus(); }
                return;
              }
              if (e.key !== "Escape") return;
              if (query) {
                setQuery("");
              } else {
                // 空词时 Esc 是死键（useEscClose 对输入框让位）：交还焦点，让它放行关窗。
                e.currentTarget.blur();
              }
            }}
          />
          {/* 7B-7：清词此前只有 Esc 一条路（且要焦点在框里）。补一颗 ×，与贴纸搜索同款。 */}
          {query && (
            <button
              type="button"
              className="side-search-x"
              aria-label={t.settings.searchClear}
              data-tip={t.settings.searchClear}
              onClick={() => { setQuery(""); searchInputRef.current?.focus(); }}
            >×</button>
          )}
        </div>
        <nav className="side-nav">
          {/* 搜索态点分区 = 清词并跳转（搜索结果已把各分区平铺，点导航即退出搜索）。
              搜索态不高亮任何一项（navOn）：右侧是各分区平铺的结果或「没有匹配」，
              这时还亮着进搜索前的那一项，和右侧内容对不上（实拍 settingswin-noresults）。 */}
          {SECTION_ORDER.filter((item) => item !== "about").map((item) => {
            const NavIcon = NAV_ICONS[item];
            return (
              <button key={item} className={"nav-item" + (navOn(item) ? " on" : "")} aria-current={navOn(item) ? "page" : undefined} onClick={() => { setQuery(""); pickSec(item); }}>
                <NavIcon />
                <span>{t.settings.nav[item]}</span>
              </button>
            );
          })}
          <button className={"nav-item" + (navOn("about") ? " on" : "")} aria-current={navOn("about") ? "page" : undefined} onClick={() => { setQuery(""); pickSec("about"); }}>
            <IconInfo />
            <span>{t.settings.nav.about}</span>
            {(status === "available" || status === "downloading" || status === "ready") && (
              <span className="nav-tag">{t.settings.updateTag}</span>
            )}
          </button>
        </nav>
      </aside>

      <main className="main">
        {/* 不放页标题（用户试过顶部标题两版都嫌突兀）：在哪一页由左栏高亮说明，右侧直接是内容。
            搜索时各分区平铺，才由分区内的 .sec-title 当分隔。 */}
        <div className="main-bar" data-tauri-drag-region>
          <button className="winclose" data-tip={t.settings.close} onClick={close} aria-label={t.settings.close}>
            <Icon glyph={icClose} size={14} />
          </button>
        </div>
        <div className={"main-body" + (searching ? " is-searching" : "")} ref={bodyRef}>
          {/* T-4：无结果时给一个下一步（清除搜索并回到搜索框），而不是只有一行字。 */}
          {searching && noHits && (
            <div className="settings-noresults">
              <span>{t.settings.searchNoResults}</span>
              <button type="button" className="sbtn" onClick={() => { setQuery(""); searchInputRef.current?.focus(); }}>
                {t.settings.searchClear}
              </button>
            </div>
          )}
          {/* 搜索态挂载全部分区并平铺（data-sec 供过滤识别导航名命中），
              各分区的行级显隐由 searchFilter 直写 style.display（hidden 归 React 管）。
              代价：连带挂载 AccountSection 会触发配额联网扇出——这是 S-2「未访问不挂载」
              与搜索可达性之间的有意取舍：搜索必须能命中未访问分区的行，就只能全挂。
              不是遗漏，勿当 bug「修」掉。 */}
          {SECTION_ORDER.filter((s) => searching || visited.has(s)).map((s) => (
            <div key={s} className="main-sec" data-sec={s} hidden={searching ? false : s !== sec}>
              {/* 页标题（对照 Kimi 设置每页顶部的标题）：此前右侧只有一摞卡片，在哪一页只能回头
                  看左栏高亮；搜索结果平铺各分区时它也是分隔。 */}
              {!(s === "general" && inTeamPage) && <h1 className="sec-title">{t.settings.nav[s]}</h1>}
              {s === "general" ? (
                inTeamPage ? (
                  <TeamSection onBack={() => pickGeneralView("main")} />
                ) : (
                  <GeneralSection searching={searching} onOpenTeam={() => pickGeneralView("team")} />
                )
              ) : s === "appearance" ? (
                <AppearanceSection />
              ) : s === "notify" ? (
                <NotifySection />
              ) : s === "sessions" ? (
                <SessionsSection />
              ) : s === "account" ? (
                <AccountSection view={agentView} onViewChange={pickAgentView} showAll={searching} />
              ) : s === "github" ? (
                <CodeHostingSection />
              ) : s === "remote" ? (
                <RemoteAccessCard />
              ) : s === "shortcuts" ? (
                <ShortcutsSection />
              ) : (
                <AboutSection status={status} newVersion={newVersion} />
              )}
            </div>
          ))}
        </div>
      </main>
    </div>
  );
}
