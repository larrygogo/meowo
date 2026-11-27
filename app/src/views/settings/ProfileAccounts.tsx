// 设置 › Agent › 账号区：照 Orca 的设置 › Accounts（stablyai/orca，
// src/renderer/src/components/settings/accounts/claude/）重做（2026-11-27 用户：「完全参考 Orca 的方式重做，包括界面」）。
//
// 和 Orca 对齐的几条：
// - 头部一行：「账号」+ 说明，右上「添加账号」。添加**直接拉起登录**，不先问名字；登录没完成（取消/超时/拉起失败）
//   就把刚建的空账号撤掉。之前要先起名、建出一个未登录的空账号，再单独点登录，加到一半走开就留下空壳。
// - 「系统默认」单独一张卡：写明是谁（系统默认：邮箱），就是 agent 自己的目录，不能删、不能改名。
// - 托管账号一张卡一个，整卡点击切换；使用中的卡描边加深 + 「使用中」徽章。没登录的卡写「重新登录后才能使用」，
//   而且不让切过去（之前能切到未登录的账号，新会话直接卡在登录页）。
// - 行内三个动作同 Orca：改名（弹窗，留空显示邮箱）、重新登录、移除（确认框说清删什么、不删什么）。
//   Meowo 多一个「用量」：看非活跃账号的额度（Orca 放在状态栏，Meowo 放在这张卡上方的额度区）。
// - 一个都没加时给虚线框空状态；「切换账号时正在运行的会话」是单独的设置行。
//
// 和旧版的取舍：退出登录、合并进默认账号两项从界面拿掉（Orca 没有；退出用「移除」或「重新登录」换号，
// 后端命令保留）。默认账号不再能改名（Orca 的系统默认没有名字，只显示登录邮箱）。
import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { appConfirm } from "../../confirm";
import {
  listProfiles,
  createProfile,
  setActiveProfile,
  applyActiveProfileToSessions,
  sessionsOffActiveProfileCount,
  renameProfile,
  deleteProfile,
  revealTerminalPath,
  archiveProfileSetAside,
  type AgentId,
  type LoginDone,
  type ProfileView,
  type ProfileSetAside,
  type ProfileSwitchRunning,
} from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { Dropdown } from "../menu";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import { ACTIVE_PROFILE_CHANGED } from "../island/useIslandUsage";
import type { LoginOperationState } from "../../hooks/useLoginOperations";
import { Icon } from "../../icons/Icon";
import { icClose, icPencil, icPlus, icRefreshCw, icTrash } from "../../icons/glyphs";
import { AgentAppIcon } from "../chat/AgentAppIcon";
import { isMac, isWindows } from "../../platform";
import { remoteUi } from "../../remoteMode";
import { busyProps } from "../../busyButton";
import { AppModal } from "../AppModal";
import type { ShareTopic } from "../../generated/contracts/ShareTopic";
import { profileIdentity as identity, profileLabel, sameAccountName } from "../../profileLabel";

/** 默认账号在前端的行 key —— 后端给的 id 是 `null`（它不在 settings.profiles 里）。 */
const DEFAULT_KEY = "__default__";

export function ProfileList({ provider, agentName, highlightLogin = false, movesSessionsAcrossAccounts, profileShares, switchRunning, onSwitchRunningChange, onChanged, viewingProfile, onViewUsage, loginState, onStartLogin, onCancelLogin }: {
  provider: AgentId;
  /** agent 的展示名（「用本机当前的 Claude 登录」）。 */
  agentName: string;
  /** 刚装完：系统默认的「登录」升为主按钮，把「装完 → 登录」串成一条链路。 */
  highlightLogin?: boolean;
  /** 切换后要不要处理「运行中的会话」（后端 moves_sessions_across_accounts）。 */
  movesSessionsAcrossAccounts: boolean;
  /** 后端 profile_shares：多账号之间共用的东西。 */
  profileShares: ShareTopic[];
  /** 设置「切换账号时正在运行的会话」：每次问 / 总是重启 / 从不重启（照 Orca）。 */
  switchRunning: ProfileSwitchRunning;
  onSwitchRunningChange: (v: ProfileSwitchRunning) => void;
  onChanged: () => void;
  /** null = 正在看 active；仅决定用量按钮的按下态，不改变 active。 */
  viewingProfile: ProfileView | null;
  onViewUsage: (profile: ProfileView) => void;
  loginState: LoginOperationState | undefined;
  onStartLogin: (profile?: string | null) => Promise<boolean>;
  onCancelLogin: () => Promise<boolean>;
}) {
  const t = useT();
  const a = t.account;
  const [rows, setRows] = useState<ProfileView[] | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const sharesHistory = profileShares.includes("history");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<ProfileView | null>(null);
  // 「添加账号」刚建出、正等着登录的账号 id。ref 是事件回调里的同步真相，state 只管渲染。
  const addingRef = useRef<string | null>(null);
  const [addingId, setAddingId] = useState<string | null>(null);
  const alive = useRef(true);
  // 挂载时要重置为 true：dev 下 StrictMode 会「挂载→卸载→再挂载」，只在卸载里置 false 的话
  // 再挂载后它一直是 false，异步结果全被丢掉（2026-11-27 dev 实例里账号列表一直不出来）。
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);

  const reload = useCallback(() => {
    listProfiles(provider)
      .then((r) => { if (alive.current) { setRows(r); setLoadErr(null); } })
      // 之前吞掉错误、当成空列表：整个账号区只剩一个「添加账号」，看着像账号全丢了。
      .catch((e) => { if (alive.current) setLoadErr(formatBackendError(e, t.locale)); });
  }, [provider, t.locale]);

  useEffect(reload, [reload]);
  // 手机远程切换账号后桌面要跟上（2026-11-15 手机切换上线，独立审查指出列表停在旧的「使用中」）。
  useTauriEvent<string>(ACTIVE_PROFILE_CHANGED, ({ payload }) => {
    if (payload === provider) reload();
  });

  /** 登录没完成：刚建的账号还没登录就撤掉；已经登录上了（取消前一刻刚好登完）就留着。 */
  const rollback = useCallback(async (id: string) => {
    try {
      const latest = await listProfiles(provider);
      const row = latest.find((p) => p.id === id);
      if (row && !row.account) await deleteProfile(provider, id);
      if (alive.current) setNotice(row && !row.account ? a.addRolledBack : null);
    } catch (e) {
      if (alive.current) setErr(formatBackendError(e, t.locale));
    } finally {
      reload();
    }
  }, [provider, a.addRolledBack, t.locale, reload]);

  useTauriEvent<LoginDone>("login-done", ({ payload }) => {
    if (payload.provider !== provider) return;
    const adding = addingRef.current;
    if (adding) {
      addingRef.current = null;
      setAddingId(null);
      // 不信 outcome，按这个账号自己的凭据决定：rollback 只撤还没登录的。老后端取消时查的是
      // 默认账号，默认账号登着就把「取消」报成 success，空账号会留下（2026-11-27 本机实测）。
      void rollback(adding).then(onChanged);
      return;
    }
    reload();
    onChanged();
  });

  const run = async (fn: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true);
    setErr(null);
    setNotice(null);
    try {
      await fn();
      reload();
      onChanged();
    } catch (e) {
      setErr(formatBackendError(e, t.locale));
    } finally {
      setBusy(false);
    }
  };

  const loginPending = loginState?.phase === "pending";

  /** 添加账号 = 建一个没名字的账号 + 立刻在它里面登录（照 Orca 的 claudeAccounts.add）。 */
  const add = async () => {
    if (busy || loginPending || addingRef.current) return;
    setErr(null);
    setNotice(null);
    let id: string;
    try {
      id = await createProfile(provider, "");
    } catch (e) {
      setErr(formatBackendError(e, t.locale));
      return;
    }
    addingRef.current = id;
    setAddingId(id);
    reload();
    try {
      const started = await onStartLogin(id);
      if (!started) throw new Error("login/busy");
    } catch (e) {
      addingRef.current = null;
      setAddingId(null);
      await rollback(id);
      if (alive.current && String(e) !== "Error: login/busy") setErr(`${a.loginFailed}：${formatBackendError(e, t.locale)}`);
    }
  };

  const cancelAdd = () => {
    void onCancelLogin().catch((e) => setErr(formatBackendError(e, t.locale)));
  };

  /** 重新登录 / 登录某一行。**必须带上这一行自己的 id**：漏了它，凭据会写进默认账号。 */
  const signIn = (p: ProfileView) => {
    setErr(null);
    setNotice(null);
    const mine = loginState?.phase === "pending" && loginState.profile === p.id;
    const request = mine ? onCancelLogin() : onStartLogin(p.id);
    void request.catch((e) => setErr(formatBackendError(e, t.locale)));
  };

  /** 移除：连同它的目录一起删。活跃账号也能删，后端把活跃标记落回系统默认。 */
  const remove = async (p: ProfileView) => {
    if (!p.id) return;
    const label = profileLabel(p, t);
    const yes = await appConfirm(sharesHistory ? a.removeAccountConfirmShared(label) : a.removeAccountConfirm(label), {
      title: a.removeAccountTitle,
      danger: true,
      confirmLabel: a.removeAccountConfirmLabel,
    });
    if (!yes) return;
    void run(() => deleteProfile(provider, p.id!));
  };

  /**
   * 切换活跃账号：先切设置，再按「切换账号时正在运行的会话」处理运行中的会话（照 Orca 的同名设置）。
   * 待重启的会话数问后端，不自己翻看板列表：那份列表分页，久未活动的会话翻不到会漏数成 0。
   */
  const switchTo = (p: ProfileView) => {
    if (busy || p.active) return;
    void run(async () => {
      await setActiveProfile(provider, p.id);
      if (!movesSessionsAcrossAccounts || switchRunning === "keep") return;
      const running = await sessionsOffActiveProfileCount(provider);
      if (running === 0) return;
      const ok =
        switchRunning === "restart" ||
        (await appConfirm(a.switchRestartConfirm(running), {
          title: a.switchProfile,
          danger: true,
          confirmLabel: a.switchRestartConfirmLabel,
        }));
      if (!ok) return;
      // 重启失败不连累切换：设置已经写进去了，错误就地显示。
      await applyActiveProfileToSessions(provider).catch((e) => setErr(formatBackendError(e, t.locale)));
    });
  };

  const system = rows?.find((p) => p.id === null);
  const managed = rows?.filter((p) => p.id !== null) ?? [];
  const systemEmail = system ? identity(system) : null;
  const runtime = runtimeLabel();

  const usageButton = (p: ProfileView, key: string) =>
    p.account && (
      <button
        type="button"
        className="acct-act"
        data-testid={"profile-usage-" + provider + "-" + key}
        aria-label={a.viewProfileUsage(profileLabel(p, t))}
        // 只在点开看别的账号时标按下：默认看的就是当前账号，当前行常亮一块底色像是被选中了。
        aria-pressed={viewingProfile ? (viewingProfile.id ?? DEFAULT_KEY) === key : undefined}
        {...busyProps(busy, () => onViewUsage(p))}
      >
        {a.viewUsage}
      </button>
    );

  const signInButton = (p: ProfileView, key: string, label: string, primary = false) => {
    const mine = loginState?.phase === "pending" && loginState.profile === p.id && addingId === null;
    return (
      <button
        type="button"
        className={"acct-act" + (primary && !mine ? " btn-primary" : "")}
        data-testid={"profile-login-" + provider + "-" + key}
        disabled={loginPending && !mine}
        {...busyProps(busy, () => signIn(p))}
      >
        <Icon glyph={icRefreshCw} size={12} className={mine ? "spin" : undefined} />
        {mine ? a.cancelLogin : label}
      </button>
    );
  };

  // 系统默认行：Orca 的写法是「系统默认：邮箱」+ 一句来源说明，没有改名/移除。
  const systemRow = system && (
    <div className={"acct-card" + (system.active ? " is-active" : "")} data-testid={"profile-" + provider + "-" + DEFAULT_KEY}>
      <button
        type="button"
        className="acct-main"
        data-tip={system.active ? undefined : a.switchProfile}
        {...busyProps(busy || system.active, () => switchTo(system))}
      >
        <span className="acct-title-row">
          <span className="acct-name">{systemEmail ? a.systemDefaultNamed(systemEmail) : a.systemDefault}</span>
          {system.active && <span className="acct-badge">{a.activeProfile}</span>}
        </span>
        <span className="acct-desc">{system.account ? a.systemDefaultDesc(agentName) : a.systemDefaultSignedOut(agentName)}</span>
      </button>
      <div className="acct-acts">
        {usageButton(system, DEFAULT_KEY)}
        {!system.account && signInButton(system, DEFAULT_KEY, a.login, highlightLogin)}
      </div>
    </div>
  );

  const managedRow = (p: ProfileView) => {
    const key = p.id!;
    const label = profileLabel(p, t);
    const email = identity(p);
    const needsSignIn = !p.account;
    const adding = addingId === p.id;
    // 起过名的行把登录邮箱放进详情行（名字就是邮箱时不重复）；照 Orca：组织 · 套餐 · 邮箱。
    const namedEmail = p.name.trim() && email && !sameAccountName(p.name, email) ? email : null;
    const detail = needsSignIn
      ? [namedEmail, adding ? a.addWaiting : a.signInRequired].filter(Boolean).join(" · ")
      : [p.account?.organization, p.account?.plan, namedEmail].filter(Boolean).join(" · ") || a.signedIn;
    return (
      <Fragment key={key}>
        <div
          className={"acct-card" + (p.active ? " is-active" : "") + (needsSignIn ? " needs-sign-in" : "")}
          data-testid={"profile-" + provider + "-" + key}
        >
          <button
            type="button"
            className="acct-main"
            data-tip={p.active || needsSignIn ? undefined : a.switchProfile}
            {...busyProps(busy || p.active || needsSignIn, () => switchTo(p))}
          >
            <span className="acct-title-row">
              <span className="acct-name">{label}</span>
              {runtime && <span className="acct-badge acct-badge-runtime">{runtime}</span>}
              {p.active && <span className="acct-badge">{a.activeProfile}</span>}
            </span>
            <span className="acct-desc" data-tip={detail}>{detail}</span>
          </button>
          {!adding && (
            <div className="acct-acts">
              {usageButton(p, key)}
              <button
                type="button"
                className="acct-act"
                data-testid={"profile-rename-" + provider + "-" + key}
                {...busyProps(busy, () => setRenaming(p))}
              >
                <Icon glyph={icPencil} size={12} />
                {a.rename}
              </button>
              {signInButton(p, key, a.signInAgain)}
              <button
                type="button"
                className="acct-act acct-act-danger"
                data-testid={"profile-remove-" + provider + "-" + key}
                {...busyProps(busy, () => void remove(p))}
              >
                <Icon glyph={icTrash} size={12} />
                {a.removeAccount}
              </button>
            </div>
          )}
        </div>
        {p.set_aside?.length > 0 && (
          <ProfileSetAsideNote
            provider={provider}
            profileId={key}
            items={p.set_aside}
            onArchived={() => { setNotice(a.setAsideArchived); reload(); }}
          />
        )}
      </Fragment>
    );
  };

  return (
    <section className="acct-group" data-testid={"profiles-" + provider}>
      {/* 照 Orca 的 SettingsGroup：分区标题和说明在卡片外，卡片里是「账号」块和设置行。 */}
      <div className="acct-group-head">
        <div className="acct-group-title">
          <AgentAppIcon provider={provider} className="acct-group-icon" />
          {agentName}
        </div>
        <div className="acct-group-desc">{a.accountsGroupDesc(agentName)}</div>
      </div>
      <div className="row-card acct-group-card">
        <div className="acct">
          <div className="acct-head">
            <div className="acct-head-text">
              <div className="acct-head-title">{a.profiles}</div>
              <div className="acct-head-sub">{a.accountsSub}</div>
              <div className="acct-head-sub" data-testid={"profile-share-" + provider}>{shareLine(profileShares, t)}</div>
            </div>
            <div className="acct-head-acts">
              <button
                type="button"
                className="acct-add"
                data-testid={"profile-add-" + provider}
                disabled={(loginPending && !addingId) || undefined}
                {...busyProps(busy || addingId !== null, () => void add())}
              >
                <Icon glyph={addingId ? icRefreshCw : icPlus} size={12} className={addingId ? "spin" : undefined} />
                {a.addProfile}
              </button>
              {addingId && (
                <button type="button" className="acct-act" data-testid={"profile-add-cancel-" + provider} onClick={cancelAdd}>
                  <Icon glyph={icClose} size={12} />
                  {a.cancelEdit}
                </button>
              )}
            </div>
          </div>
          {!rows ? (
            loadErr && <div className="agent-install-error">{loadErr}</div>
          ) : (
            <div className="acct-list">
              {systemRow}
              {managed.length === 0 ? (
                <div className="acct-empty" data-testid={"profiles-empty-" + provider}>{a.accountsEmpty(agentName)}</div>
              ) : (
                managed.map(managedRow)
              )}
            </div>
          )}
          {notice && <div className="acct-notice" role="status">{notice}</div>}
          {err && <div className="agent-install-error" role="alert">{err}</div>}
        </div>
        {movesSessionsAcrossAccounts ? (
          <div className="acct-setting-row" data-testid={"profile-switch-running-" + provider}>
            <div className="acct-setting-text">
              <div className="acct-setting-label">{a.switchRunningLabel(agentName)}</div>
              <div className="acct-setting-desc">{a.switchRunningDesc}</div>
            </div>
            <Dropdown
              value={switchRunning}
              options={[
                { value: "ask" as const, label: a.switchRunningAsk },
                { value: "restart" as const, label: a.switchRunningRestart },
                { value: "keep" as const, label: a.switchRunningKeep },
              ]}
              onChange={onSwitchRunningChange}
            />
          </div>
        ) : (
          <div className="acct-setting-row">
            <div className="acct-setting-desc">{a.switchCoverage}</div>
          </div>
        )}
      </div>
      {renaming && (
        <RenameAccountModal
          profile={renaming}
          onClose={() => setRenaming(null)}
          onSubmit={async (name) => {
            await renameProfile(provider, renaming.id, name);
            reload();
            onChanged();
          }}
        />
      )}
    </section>
  );
}

/** 账号行上的运行时徽章（Orca 的「Windows」/「WSL」）。Meowo 只在本机跑，手机远程页不显示。 */
function runtimeLabel(): string | null {
  if (remoteUi()) return null;
  return isWindows() ? "Windows" : isMac() ? "macOS" : null;
}

/** 改名弹窗（照 Orca 的 account-rename-dialog）：标题、说明、「显示名称」输入框；左下「恢复默认」，右下取消/保存。 */
function RenameAccountModal({ profile, onClose, onSubmit }: {
  profile: ProfileView;
  onClose: () => void;
  onSubmit: (name: string) => Promise<void>;
}) {
  const t = useT();
  const a = t.account;
  const email = identity(profile);
  const [value, setValue] = useState(profile.name);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const save = async (name: string) => {
    if (saving) return;
    if (name.trim() === profile.name.trim()) { onClose(); return; }
    setSaving(true);
    setError(null);
    try {
      await onSubmit(name.trim());
      onClose();
    } catch (e) {
      setError(formatBackendError(e, t.locale));
      setSaving(false);
    }
  };
  return (
    <AppModal label={a.renameTitle} onClose={onClose}>
      <div className="chat-modal-title">{a.renameTitle}</div>
      <div className="acct-modal-hint">{email ? a.renameHint(email) : a.renameHintPlain}</div>
      <label className="acct-modal-field">
        <span className="acct-modal-label">{a.renameLabel}</span>
        <input
          className="ns-input acct-modal-input"
          autoFocus
          value={value}
          placeholder={email ?? ""}
          data-testid="profile-rename-input"
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing) void save(value); }}
        />
      </label>
      {error && <div className="chat-modal-error" role="alert">{error}</div>}
      <div className="chat-modal-actions">
        <button
          type="button"
          className="ns-btn btn-ghost acct-modal-reset"
          data-testid="profile-rename-reset"
          disabled={!profile.name.trim() || !email}
          onClick={() => void save("")}
        >
          {a.renameReset}
        </button>
        <button type="button" className="ns-btn" onClick={onClose}>{a.cancelEdit}</button>
        <button type="button" className="ns-btn btn-primary" data-testid="profile-rename-save" {...busyProps(saving, () => void save(value))}>{a.renameSave}</button>
      </div>
    </AppModal>
  );
}

/** 共用项的显示顺序（与后端枚举一致；前端只管出名字）。 */
const SHARE_TOPIC_ORDER: ShareTopic[] = ["rules", "settings", "plugins", "skills", "commands", "mcp", "history"];

/**
 * 「共用什么、各自什么」收成卡片头的第二行说明（Orca 的 Codex 分区也是两行说明，没有标签）。
 * 2026-11-27 三版样稿里选过两排标签；照 Orca 对齐时改成一句话，信息不变。
 */
function shareLine(shares: ShareTopic[], t: ReturnType<typeof useT>): string {
  const shared = SHARE_TOPIC_ORDER.filter((topic) => shares.includes(topic)).map((topic) => t.account.shareTopic[topic]);
  const own = [t.account.ownLogin, t.account.ownQuota, ...(shares.includes("history") ? [] : [t.account.shareTopic.history])];
  return t.account.shareLine(shared, own);
}

/**
 * 和系统默认内容不同、单独留下的条目。Orca 只写日志、界面不提；这里照 Orca 账号列表里提示行的样式
 * （一行灰字 + 幽灵按钮，同它的 systemDefaultIsSaved），给出能点的处理：打开文件夹看看，或改用系统默认
 * （移到归档文件夹，不删）。之前是一大块黄底提醒，账号照常能用却看着像出了错（2026-11-27 用户：「显示一大堆冲突」）。
 */
function ProfileSetAsideNote({ provider, profileId, items, onArchived }: {
  provider: AgentId;
  profileId: string;
  items: ProfileSetAside[];
  onArchived: () => void;
}) {
  const t = useT();
  const a = t.account;
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const names = items.map((item) => item.name.replace(/\.meowo-profile-(merge|conflict)(-\d+)?$/, "")).join("、");
  const reveal = () => {
    setMsg(null);
    revealTerminalPath("", items[0].path).catch((e: unknown) => setMsg(formatBackendError(e, t.locale)));
  };
  const archive = async () => {
    const yes = await appConfirm(a.setAsideArchiveConfirm(items.length, names), {
      title: a.setAsideArchiveTitle,
      confirmLabel: a.setAsideUseDefault,
    });
    if (!yes) return;
    setBusy(true);
    setMsg(null);
    try {
      await archiveProfileSetAside(provider, profileId);
      onArchived();
    } catch (e) {
      setMsg(formatBackendError(e, t.locale));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="acct-aside" data-testid={"profile-aside-" + provider + "-" + profileId}>
      <p className="acct-aside-text" data-tip={names}>
        {a.setAsideLine(items.length)}
        {msg && <span className="acct-aside-err"> {msg}</span>}
      </p>
      <div className="acct-aside-acts">
        {!remoteUi() && (
          <button type="button" className="acct-act" data-testid={"profile-aside-reveal-" + provider + "-" + profileId} onClick={reveal}>
            {a.mcpConfigReveal}
          </button>
        )}
        <button
          type="button"
          className="acct-act"
          data-testid={"profile-aside-archive-" + provider + "-" + profileId}
          {...busyProps(busy, () => void archive())}
        >
          {a.setAsideUseDefault}
        </button>
      </div>
    </div>
  );
}
