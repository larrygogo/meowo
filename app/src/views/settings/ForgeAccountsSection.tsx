// 设置 › 代码托管账号：github.com 沿用「GitHub 连接方式」（GithubSection），其余平台按主机加账号
// （后端 forge_accounts.rs）。
//
// - 账号列表：平台图标、主机、账号名，令牌失效时标出来并给「重新添加」，可以退出。
// - 添加：在卡片里展开（不弹窗，与设置页其他分区同一套行式卡片），选平台 → 主机（自建时）→ 浏览器登录
//   （gitlab.com 且打包了 Meowo 的 GitLab 应用时）或访问令牌。令牌框旁写明要勾的权限、给生成页链接；
//   后端先用令牌问一次「我是谁」，通过了才存进系统凭据库。
// - 体验对照 JetBrains 的 GitHub / GitLab 账号设置（按主机列账号、Log In with Token）与 GitHub Desktop 的
//   企业版账号；这里不让用户手选「用哪个账号」，按仓库远端的主机自动选。
// - 令牌只在后端；这里拿到的只有平台、主机、账号名与要不要重新添加。
import { useCallback, useEffect, useRef, useState } from "react";
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
import { icForgeBitbucket, icForgeGitea, icForgeGithub, icForgeGitlab } from "../../icons/glyphs.settings";
import { GithubSection, minutesLeft } from "./GithubSection";
import { SettingsError } from "./widgets";

const IS_MAC = typeof navigator !== "undefined" && /Mac/i.test(navigator.userAgent);
const PLATFORMS: ForgePlatform[] = ["gitlab", "gitea", "bitbucket", "github"];
/** 选平台时主机框的默认值：公共站点直接填好，企业版留空让用户填。 */
const DEFAULT_HOST: Record<ForgePlatform, string> = { gitlab: "gitlab.com", gitea: "codeberg.org", bitbucket: "bitbucket.org", github: "" };

export function platformGlyph(platform: string): Glyph {
  switch (platform) {
    case "gitlab": return icForgeGitlab;
    case "gitea": return icForgeGitea;
    case "bitbucket": return icForgeBitbucket;
    default: return icForgeGithub;
  }
}

/** 用户输入的主机：去掉协议、路径与末尾斜杠，小写（后端再校验一次）。 */
export function cleanHost(text: string): string {
  return text.trim().replace(/^https?:\/\//i, "").split("/")[0].toLowerCase();
}

/** 生成令牌的网页（预填名字与权限的能填就填）。 */
export function tokenPage(platform: ForgePlatform, host: string): string | null {
  const h = cleanHost(host);
  switch (platform) {
    case "gitlab": return h ? `https://${h}/-/user_settings/personal_access_tokens?name=Meowo&scopes=api` : null;
    case "gitea": return h ? `https://${h}/user/settings/applications` : null;
    case "bitbucket": return "https://id.atlassian.com/manage-profile/security/api-tokens";
    case "github": return h ? `https://${h}/settings/tokens/new?description=Meowo&scopes=repo` : null;
  }
}

type Draft = { platform: ForgePlatform; host: string; token: string; user: string; method: "token" | "device" };

export function ForgeAccountsSection() {
  const t = useT();
  return (
    <>
      <div className="sec-caption forge-first-caption">{t.forgeAccounts.githubCaption}</div>
      <GithubSection />
      <div className="sec-caption">{t.forgeAccounts.othersCaption}</div>
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
  const [copied, setCopied] = useState(false);
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
        if (state === "ok") setDraft(null);
        else if (state === "expired") setNotice(t.forgeAccounts.expired);
        else if (state === "denied") setNotice(t.forgeAccounts.denied);
        else if (state === "error") setError(t.forgeAccounts.loginFailed(formatBackendError(reason ?? "", t.locale)));
        reload();
      }));
    } catch { /* 非 Tauri 环境 */ }
    return () => { disposers.forEach((p) => void p.then((off) => off()).catch(() => {})); };
  }, [reload, t]);

  const accounts = status?.accounts ?? [];
  const pending = status?.pending ?? null;
  const vault = IS_MAC ? t.github.vaultMac : t.github.vaultWindows;
  const deviceOk = (d: Draft) => !!status?.gitlabDevice && d.platform === "gitlab" && cleanHost(d.host) === "gitlab.com";

  const startAdd = (platform: ForgePlatform = "gitlab", host?: string) => {
    setError(null);
    setNotice(null);
    const next: Draft = { platform, host: host ?? DEFAULT_HOST[platform], token: "", user: "", method: "token" };
    setDraft({ ...next, method: deviceOk(next) ? "device" : "token" });
  };

  const pickPlatform = (platform: ForgePlatform) => {
    if (!draft) return;
    const next: Draft = { ...draft, platform, host: DEFAULT_HOST[platform], token: "", user: "" };
    setDraft({ ...next, method: deviceOk(next) ? "device" : "token" });
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
      .catch((e) => { if (String(e) !== "forge/cancelled") setError(f.loginFailed(formatBackendError(e, t.locale))); })
      .finally(() => setBusy(null));
  };

  const cancelDevice = () => {
    setStatus((s) => (s ? { ...s, pending: null } : s));
    void forgeLoginCancel().catch(() => {}).finally(reload);
  };

  const copy = (code: ForgeDeviceCodeDto) =>
    navigator.clipboard?.writeText(code.userCode).then(() => {
      setCopied(true);
      window.setTimeout(() => { if (alive.current) setCopied(false); }, 1500);
    }).catch(() => {}) ?? Promise.resolve();

  const signOut = (account: ForgeAccountDto) => {
    if (busy) return;
    const platform = f.platformNames[account.platform] ?? account.platform;
    void appConfirm(f.signOutConfirm(platform, account.host), { title: f.signOut, confirmLabel: f.signOut, danger: true }).then((yes) => {
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
        <div className="row-desc is-full">{`${f.platformNames[account.platform] ?? account.platform} · ${account.host}`}</div>
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
          <button type="button" className="sbtn" onClick={() => void copy(code)}>{copied ? t.github.copied : t.github.copyCode}</button>
          <button type="button" className="sbtn" onClick={cancelDevice}>{f.cancel}</button>
        </div>
        <div className="row-desc github-waiting" role="status"><span className="github-dot" aria-hidden />{f.waiting}</div>
      </div>
    </div>
  );

  const addForm = (d: Draft) => {
    const page = tokenPage(d.platform, d.host);
    const device = d.method === "device" && deviceOk(d);
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
            value={d.host} placeholder={f.hostPlaceholder[d.platform]} disabled={d.platform === "bitbucket"}
            onChange={(e) => setDraft({ ...d, host: e.target.value, method: "token" })} />
          {d.platform === "bitbucket" && <div className="row-desc is-full forge-field-note">{f.hostFixed}</div>}
        </div>
        {device ? (
          <div className="row">
            <div className="row-text">
              <div className="row-label">{f.deviceTitle}</div>
              <div className="row-desc is-full">{f.deviceDesc}</div>
            </div>
            <div className="github-row-actions">
              <button type="button" className="sbtn" onClick={() => setDraft(null)}>{f.cancel}</button>
              <button type="button" className="sbtn" onClick={() => setDraft({ ...d, method: "token" })}>{f.useToken}</button>
              <button type="button" className="sbtn btn-primary" {...busyProps(busy === "device", startDevice)}>{busy === "device" ? t.github.signingIn : f.deviceStart}</button>
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
              {deviceOk(d) && <button type="button" className="sbtn forge-alt" onClick={() => setDraft({ ...d, method: "device" })}>{f.useDevice}</button>}
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
            {supported && <button type="button" className="sbtn btn-primary" onClick={() => startAdd()}>{f.add}</button>}
          </div>
        )}
        {!pending && !draft && accounts.length > 0 && supported && (
          <div className="row forge-add-row">
            {notice && <div className="row-text"><div className="row-desc is-full">{notice}</div></div>}
            <button type="button" className="sbtn" data-testid="forge-add-start" onClick={() => startAdd()}>{f.add}</button>
          </div>
        )}
      </div>
      <div className="sec-hint">{f.note}{accounts.length > 0 ? ` ${f.tokenDesc(vault)}` : ""}</div>
      <SettingsError preformatted error={error} onDismiss={() => setError(null)} />
    </>
  );
}

/// 选平台：两列四块，每块是平台图标 + 名字（四段的分段控件在 620 宽的设置窗里放不下，名字会被截断）。
/// 语义是单选（radiogroup / radio），选中只用底色区分；方向键在四块之间移动并选中，同分段控件。
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
