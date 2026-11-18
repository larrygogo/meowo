// 设置 › 代码托管：两组。
// - Git：Meowo 实际调用的 git 程序与全局提交身份（GitToolCard，后端 git_tool.rs）。
// - 平台账号：github.com 与其他平台在同一个列表、同一个添加流程里（后端 forge_accounts.rs）。Meowo 不调用本机 gh
//   （leader 2026-11-18「取消gh」），github.com 也只走这里加的账号；没有「关闭」开关，不加账号就不发请求。
//
// - 账号列表：平台图标、主机、账号名，令牌失效时标出来并给「重新添加」，可以退出。
// - 添加：在卡片里展开（不弹窗，与设置页其他分区同一套行式卡片），选平台 → 主机 → 浏览器登录（只有 github.com，
//   GitHub 设备码）或访问令牌。令牌框旁写明要勾的权限、给生成页链接；后端先用令牌问一次「我是谁」，通过了才存进
//   系统凭据库。gitlab.com 不做浏览器登录（leader 2026-11-18「取消gitlab」），与自管 GitLab 一样用令牌。
// - 体验对照 JetBrains 的 GitHub / GitLab 账号设置（按主机列账号、Log In with Token）与 GitHub Desktop 的
//   企业版账号；这里不让用户手选「用哪个账号」，按仓库远端的主机自动选。
// - 令牌只在后端；这里拿到的只有平台、主机、账号名与要不要重新添加。
import { useCallback, useEffect, useRef, useState } from "react";
import { useFlash } from "../../hooks/useFlash";
import { listen } from "@tauri-apps/api/event";
import {
  FORGE_LOGIN_EVENT,
  GITHUB_CHANGED_EVENT,
  forgeAccountAdd,
  forgeAccountRemove,
  forgeAccounts,
  forgeLoginCancel,
  forgeLoginStart,
  openLink,
  type ForgeAccountDto,
  type ForgeAccountsDto,
  type ForgeDeviceCodeDto,
  type ForgeLoginFinished,
  type ForgePlatform,
} from "../../api";
import { appConfirm } from "../../confirm";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { busyProps } from "../../busyButton";
import { Icon, type Glyph } from "../../icons/Icon";
import { icForgeBitbucket, icForgeGitea, icForgeGitee, icForgeGithub, icForgeGitlab } from "../../icons/glyphs.settings";
import { GitToolCard } from "./GitToolCard";
import { SettingsError } from "./widgets";

const IS_MAC = typeof navigator !== "undefined" && /Mac/i.test(navigator.userAgent);
const PLATFORMS: ForgePlatform[] = ["github", "gitlab", "gitea", "bitbucket", "gitee"];
/** 主机固定的平台（只有一个公共站点）。 */
const FIXED_HOST: ReadonlySet<ForgePlatform> = new Set<ForgePlatform>(["bitbucket", "gitee"]);
/** 选平台时主机框的默认值：公共站点直接填好（GitHub 填 github.com，企业版改成自己的域名）。 */
const DEFAULT_HOST: Record<ForgePlatform, string> = { github: "github.com", gitlab: "gitlab.com", gitea: "codeberg.org", bitbucket: "bitbucket.org", gitee: "gitee.com" };

export function platformGlyph(platform: string): Glyph {
  switch (platform) {
    case "gitlab": return icForgeGitlab;
    case "gitea": return icForgeGitea;
    case "bitbucket": return icForgeBitbucket;
    case "gitee": return icForgeGitee;
    default: return icForgeGithub;
  }
}

/** 用户输入的主机：去掉协议、路径与末尾斜杠，小写（后端再校验一次）。 */
export function cleanHost(text: string): string {
  return text.trim().replace(/^https?:\/\//i, "").split("/")[0].toLowerCase();
}

/** 设备码一次性代码还剩几分钟（至少 1）。 */
export function minutesLeft(expiresAt: number, now: number): number {
  return Math.max(1, Math.ceil((expiresAt - now) / 60_000));
}

/** 能用浏览器登录（设备码）：只有 github.com。 */
export function deviceLogin(platform: ForgePlatform, host: string): boolean {
  return platform === "github" && cleanHost(host) === "github.com";
}

/** 生成令牌的网页（预填名字与权限的能填就填）。 */
export function tokenPage(platform: ForgePlatform, host: string): string | null {
  const h = cleanHost(host);
  switch (platform) {
    case "gitlab": return h ? `https://${h}/-/user_settings/personal_access_tokens?name=Meowo&scopes=api` : null;
    case "gitea": return h ? `https://${h}/user/settings/applications` : null;
    case "bitbucket": return "https://id.atlassian.com/manage-profile/security/api-tokens";
    case "gitee": return "https://gitee.com/profile/personal_access_tokens";
    case "github": return h ? `https://${h}/settings/tokens/new?description=Meowo&scopes=repo,read:org` : null;
  }
}

type Draft = { platform: ForgePlatform; host: string; token: string; user: string; method: "token" | "device" };

/** 设置 › 代码托管：Git 组 + 平台账号组。 */
export function CodeHostingSection() {
  const t = useT();
  return (
    <>
      <div className="sec-caption forge-first-caption">{t.codeHosting.gitCaption}</div>
      <GitToolCard />
      <div className="sec-caption">{t.codeHosting.accountsCaption}</div>
      <ForgeAccountList />
    </>
  );
}

export function ForgeAccountList() {
  const t = useT();
  const f = t.forgeAccounts;
  const [status, setStatus] = useState<ForgeAccountsDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState<"add" | "device" | "remove" | null>(null);
  const { shown: copied, flash: flashCopied, fx: copiedFx } = useFlash();
  const alive = useRef(true);
  useEffect(() => () => { alive.current = false; }, []);

  const reload = useCallback(() => {
    forgeAccounts()
      .then((s) => { if (alive.current) setStatus(s); })
      .catch((e) => { if (alive.current) setError(formatBackendError(e, t.locale)); });
  }, [t.locale]);

  useEffect(() => {
    reload();
    const disposers: Array<Promise<() => void>> = [];
    try {
      disposers.push(listen(GITHUB_CHANGED_EVENT, () => reload()));
      disposers.push(listen<ForgeLoginFinished>(FORGE_LOGIN_EVENT, (event) => {
        const { state, error: reason } = event.payload;
        // 结束了就收起表单：过期、拒绝的说明显示在列表底下（表单开着时看不到）。
        if (state !== "error") setDraft(null);
        if (state === "expired") setNotice(t.forgeAccounts.expired);
        else if (state === "denied") setNotice(t.forgeAccounts.denied);
        else if (state === "error") setError(t.forgeAccounts.loginFailed(formatBackendError(reason ?? "", t.locale)));
        reload();
      }));
    } catch { /* 非 Tauri 环境 */ }
    return () => { disposers.forEach((p) => void p.then((off) => off()).catch(() => {})); };
  }, [reload, t]);

  const accounts = status?.accounts ?? [];
  const pending = status?.pending ?? null;
  const vault = IS_MAC ? f.vaultMac : f.vaultWindows;

  const draftFor = (platform: ForgePlatform, host: string): Draft => ({
    platform, host, token: "", user: "", method: deviceLogin(platform, host) ? "device" : "token",
  });

  const startAdd = (platform: ForgePlatform = "github", host?: string) => {
    setError(null);
    setNotice(null);
    setDraft(draftFor(platform, host ?? DEFAULT_HOST[platform]));
  };

  const pickPlatform = (platform: ForgePlatform) => {
    if (!draft) return;
    setDraft(draftFor(platform, DEFAULT_HOST[platform]));
  };

  const verify = () => {
    if (!draft || busy) return;
    if (!cleanHost(draft.host) || !draft.token.trim()) return;
    setBusy("add");
    setError(null);
    forgeAccountAdd(draft.platform, cleanHost(draft.host), draft.token.trim(), draft.platform === "bitbucket" ? draft.user.trim() || null : null)
      .then((s) => {
        setStatus(s);
        setDraft(null);
      })
      .catch((e) => setError(formatBackendError(e, t.locale)))
      .finally(() => setBusy(null));
  };

  const startDevice = () => {
    if (busy) return;
    setBusy("device");
    setError(null);
    setNotice(null);
    forgeLoginStart()
      .then((code) => setStatus((s) => (s ? { ...s, pending: code } : s)))
      .catch((e) => { if (String(e) !== "github/cancelled") setError(f.loginFailed(formatBackendError(e, t.locale))); })
      .finally(() => setBusy(null));
  };

  const cancelDevice = () => {
    setStatus((s) => (s ? { ...s, pending: null } : s));
    void forgeLoginCancel().catch(() => {}).finally(reload);
  };

  const copy = (code: ForgeDeviceCodeDto) =>
    navigator.clipboard?.writeText(code.userCode).then(() => { if (alive.current) flashCopied(); }).catch(() => {}) ?? Promise.resolve();

  const signOut = (account: ForgeAccountDto) => {
    if (busy) return;
    const platform = f.platformNames[account.platform] ?? account.platform;
    const text = account.oauth ? f.signOutConfirmDevice : f.signOutConfirm(platform, account.host);
    void appConfirm(text, { title: f.signOut, confirmLabel: f.signOut, danger: true }).then((yes) => {
      if (!yes) return;
      setBusy("remove");
      forgeAccountRemove(account.platform, account.host)
        .then((s) => setStatus(s))
        .catch((e) => setError(formatBackendError(e, t.locale)))
        .finally(() => setBusy(null));
    });
  };

  const accountRow = (account: ForgeAccountDto) => (
    <div className="row forge-account" key={`${account.platform}/${account.host}`} data-testid={`forge-account-${account.platform}-${account.host}`}>
      <span className="forge-mark" aria-hidden><Icon glyph={platformGlyph(account.platform)} size={16} /></span>
      <div className="row-text">
        <div className="row-label">{account.name ? `${account.name} · ${account.login}` : account.login}</div>
        <div className="row-desc is-full">{`${f.platformNames[account.platform] ?? account.platform} · ${account.host} · ${account.oauth ? f.viaDevice : f.viaToken}`}</div>
        {account.needsRelogin && <div className="row-desc is-full github-warn">{f.reloginDesc}</div>}
      </div>
      <div className="github-row-actions">
        {account.needsRelogin && (
          <button type="button" className="sbtn btn-primary" onClick={() => startAdd(account.platform as ForgePlatform, account.host)}>{f.readd}</button>
        )}
        <button type="button" className="sbtn" {...busyProps(busy === "remove", () => signOut(account))}>{f.signOut}</button>
      </div>
    </div>
  );

  const deviceBlock = (code: ForgeDeviceCodeDto) => (
    <div className="row github-device" data-testid="forge-device">
      <div className="row-text">
        <div className="row-label">{f.codeLabel}</div>
        <div className="github-code" data-testid="forge-code">{code.userCode}</div>
        <div className="row-desc is-full">{f.codeHint(minutesLeft(code.expiresAt, Date.now()))}</div>
        <div className="github-actions">
          <button type="button" className="sbtn btn-primary" onClick={() => { void copy(code); void openLink(code.verificationUriComplete ?? code.verificationUri).catch(() => {}); }}>{f.copyAndOpen}</button>
          <button type="button" className={"sbtn fb-press" + copiedFx} onClick={() => void copy(code)}>{copied ? f.copied : f.copyCode}</button>
          <button type="button" className="sbtn" onClick={cancelDevice}>{f.cancel}</button>
        </div>
        <div className="row-desc github-waiting" role="status"><span className="github-dot" aria-hidden />{f.waiting}</div>
      </div>
    </div>
  );

  const addForm = (d: Draft) => {
    const page = tokenPage(d.platform, d.host);
    const canDevice = deviceLogin(d.platform, d.host);
    const device = d.method === "device" && canDevice;
    const canVerify = !!cleanHost(d.host) && !!d.token.trim();
    return (
      <div className="forge-add" data-testid="forge-add">
        <div className="row forge-field">
          <div className="row-label" id="forge-platform-label">{f.platform}</div>
          <PlatformPicker value={d.platform} labelledBy="forge-platform-label" names={f.platformNames} onChange={pickPlatform} />
        </div>
        <div className="row forge-field">
          <label className="row-label" htmlFor="forge-host">{f.host}</label>
          <input id="forge-host" className="ns-input" type="text" autoComplete="off" spellCheck={false}
            value={d.host} placeholder={f.hostPlaceholder[d.platform]} disabled={FIXED_HOST.has(d.platform)}
            onChange={(e) => setDraft({ ...d, host: e.target.value, method: deviceLogin(d.platform, e.target.value) ? d.method : "token" })} />
          {(FIXED_HOST.has(d.platform) || d.platform === "github") && <div className="row-desc is-full forge-field-note">{f.hostFixed[d.platform]}</div>}
        </div>
        {device ? (
          <div className="row forge-device-row">
            <div className="row-text">
              <div className="row-label">{f.deviceTitle}</div>
              <div className="row-desc is-full">{f.deviceDesc}</div>
            </div>
            <div className="github-row-actions">
              <button type="button" className="sbtn" onClick={() => setDraft(null)}>{f.cancel}</button>
              <button type="button" className="sbtn" data-testid="forge-use-token" onClick={() => setDraft({ ...d, method: "token" })}>{f.useToken}</button>
              <button type="button" className="sbtn btn-primary" data-testid="forge-device-start" {...busyProps(busy === "device", startDevice)}>{busy === "device" ? f.signingIn : f.deviceStart}</button>
            </div>
          </div>
        ) : (
          <>
            {d.platform === "bitbucket" && (
              <div className="row forge-field">
                <label className="row-label" htmlFor="forge-user">{f.email}</label>
                <input id="forge-user" className="ns-input" type="email" autoComplete="off" spellCheck={false}
                  value={d.user} placeholder={f.emailPlaceholder} onChange={(e) => setDraft({ ...d, user: e.target.value })} />
              </div>
            )}
            <div className="row forge-field">
              <label className="row-label" htmlFor="forge-token">{f.token}</label>
              <input id="forge-token" className="ns-input" type="password" autoComplete="off" spellCheck={false}
                value={d.token} placeholder={f.tokenPlaceholder} data-testid="forge-token"
                onChange={(e) => setDraft({ ...d, token: e.target.value })}
                onKeyDown={(e) => { if (e.key === "Enter") verify(); }} />
              <div className="row-desc is-full forge-field-note">
                {f.scopes[d.platform]}
                {page && (
                  <> <button type="button" className="scm-link-btn forge-token-link" onClick={() => void openLink(page).catch(() => {})}>{f.createToken}</button></>
                )}
              </div>
            </div>
            <div className="row forge-actions">
              {canDevice && <button type="button" className="sbtn forge-alt" data-testid="forge-use-device" onClick={() => setDraft({ ...d, method: "device" })}>{f.useDevice}</button>}
              <button type="button" className="sbtn" onClick={() => setDraft(null)}>{f.cancel}</button>
              <button type="button" className="sbtn btn-primary" data-testid="forge-verify" aria-disabled={busy !== null || !canVerify || undefined}
                aria-busy={busy === "add" || undefined} onClick={() => { if (canVerify) verify(); }}>
                {busy === "add" ? f.verifying : f.verify}
              </button>
            </div>
          </>
        )}
      </div>
    );
  };

  const supported = status?.credentialSupported ?? true;
  return (
    <>
      <div className="row-card" data-testid="forge-accounts">
        {accounts.map(accountRow)}
        {pending && deviceBlock(pending)}
        {!pending && draft && addForm(draft)}
        {!pending && !draft && accounts.length === 0 && (
          <div className="row" data-testid="forge-empty">
            <div className="row-text">
              <div className="row-label">{f.empty}</div>
              <div className="row-desc is-full">{notice ?? (supported ? f.emptyDesc : f.unsupported)}</div>
            </div>
            {supported && <button type="button" className="sbtn btn-primary" data-testid="forge-add-start" onClick={() => startAdd()}>{f.add}</button>}
          </div>
        )}
        {!pending && !draft && accounts.length > 0 && supported && (
          <div className="row forge-add-row">
            {notice && <div className="row-text"><div className="row-desc is-full">{notice}</div></div>}
            <button type="button" className="sbtn" data-testid="forge-add-start" onClick={() => startAdd()}>{f.add}</button>
          </div>
        )}
      </div>
      <div className="sec-hint">{f.note}{accounts.length > 0 ? `${t.locale === "zh" ? "" : " "}${f.tokenDesc(vault)}` : ""}</div>
      <SettingsError preformatted error={error} onDismiss={() => setError(null)} />
    </>
  );
}

/// 选平台：两列，每块是平台图标 + 名字（分段控件在 620 宽的设置窗里放不下，名字会被截断）。
/// 语义是单选（radiogroup / radio），选中只用底色区分；方向键在各块之间移动并选中，同分段控件。
function PlatformPicker({ value, names, labelledBy, onChange }: {
  value: ForgePlatform;
  names: Record<string, string>;
  labelledBy: string;
  onChange: (p: ForgePlatform) => void;
}) {
  return (
    <div className="forge-platforms" role="radiogroup" aria-labelledby={labelledBy}>
      {PLATFORMS.map((p, i) => (
        <button key={p} type="button" role="radio" aria-checked={p === value} tabIndex={p === value ? 0 : -1}
          className="forge-platform" onClick={() => onChange(p)}
          onKeyDown={(e) => {
            const step = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
            if (!step) return;
            e.preventDefault();
            const next = (i + step + PLATFORMS.length) % PLATFORMS.length;
            onChange(PLATFORMS[next]);
            e.currentTarget.parentElement?.querySelectorAll<HTMLElement>("[role=radio]")[next]?.focus();
          }}>
          <span className="forge-mark" aria-hidden><Icon glyph={platformGlyph(p)} size={16} /></span>
          <span className="forge-platform-name">{names[p] ?? p}</span>
        </button>
      ))}
    </div>
  );
}
