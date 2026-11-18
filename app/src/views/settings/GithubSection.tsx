// 设置 › GitHub：PR 角标、开 PR、合并、评审评论用哪种方式连 GitHub（后端 github.rs）。
//
// - 连接方式四选一（自动 / Meowo 登录 / 本机 gh / 关闭），下面一行写明「正在使用」的实际方式与账号。
// - Meowo 登录走 GitHub 设备码：点登录 → 显示一次性代码（大号等宽，可复制）→ 复制并打开
//   github.com/login/device → 等授权（后端轮询，结果经 github-login-finished 推回）→ 可随时取消。
//   体验对照 VS Code / GitHub Desktop 的「Sign in with GitHub」。
// - 令牌只在后端的系统凭据库里；这里拿到的状态只有方式、账号展示字段、进行中的代码。
// - 头像不加载（CSP 只放行本地图片，HTTP 也只许发往 GitHub API），用账号首字母做头像。
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  GITHUB_CHANGED_EVENT,
  GITHUB_LOGIN_EVENT,
  githubLoginCancel,
  githubLoginStart,
  githubLogout,
  githubSetMode,
  githubStatus,
  openLink,
  type GithubConnectionDto,
  type GithubConnectionMode,
  type GithubDeviceCodeDto,
  type GithubLoginFinished,
} from "../../api";
import { appConfirm } from "../../confirm";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { busyProps } from "../../busyButton";
import { Segmented, SettingsError } from "./widgets";

const IS_MAC = typeof navigator !== "undefined" && /Mac/i.test(navigator.userAgent);
const MODES: GithubConnectionMode[] = ["auto", "meowo", "gh", "off"];
const asMode = (m: string | undefined): GithubConnectionMode => (MODES.includes(m as GithubConnectionMode) ? (m as GithubConnectionMode) : "auto");

/** 代码还剩几分钟（向上取整，至少 1）。 */
export function minutesLeft(expiresAt: number, now: number): number {
  return Math.max(1, Math.ceil((expiresAt - now) / 60_000));
}

export function GithubSection() {
  const t = useT();
  const [status, setStatus] = useState<GithubConnectionDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState<"mode" | "login" | "logout" | null>(null);
  const [copied, setCopied] = useState(false);
  const alive = useRef(true);
  useEffect(() => () => { alive.current = false; }, []);

  const reload = useCallback(() => {
    githubStatus()
      .then((s) => { if (alive.current) setStatus(s); })
      .catch((e) => { if (alive.current) setError(formatBackendError(e, t.locale)); });
  }, [t.locale]);

  useEffect(() => {
    reload();
    const disposers: Array<Promise<() => void>> = [];
    try {
      disposers.push(listen(GITHUB_CHANGED_EVENT, () => reload()));
      disposers.push(listen<GithubLoginFinished>(GITHUB_LOGIN_EVENT, (event) => {
        const { state, error: reason } = event.payload;
        if (state === "expired") setNotice(t.github.expired);
        else if (state === "denied") setNotice(t.github.denied);
        else if (state === "error") setError(t.github.loginFailed(formatBackendError(reason ?? "", t.locale)));
        reload();
      }));
    } catch { /* 非 Tauri 环境 */ }
    return () => { disposers.forEach((p) => void p.then((off) => off()).catch(() => {})); };
  }, [reload, t]);

  const mode = asMode(status?.mode);
  const pending = status?.pending ?? null;
  const account = status?.account ?? null;

  const changeMode = (next: GithubConnectionMode) => {
    if (busy || next === mode) return;
    setBusy("mode");
    setError(null);
    githubSetMode(next)
      .then((s) => setStatus(s))
      .catch((e) => setError(formatBackendError(e, t.locale)))
      .finally(() => setBusy(null));
  };

  const signIn = () => {
    if (busy) return;
    setBusy("login");
    setError(null);
    setNotice(null);
    githubLoginStart()
      .then((code) => setStatus((s) => (s ? { ...s, pending: code } : s)))
      .catch((e) => setError(t.github.loginFailed(formatBackendError(e, t.locale))))
      .finally(() => setBusy(null));
  };

  const cancel = () => {
    setStatus((s) => (s ? { ...s, pending: null } : s));
    void githubLoginCancel().catch(() => {}).finally(reload);
  };

  const copy = (code: GithubDeviceCodeDto) =>
    navigator.clipboard?.writeText(code.userCode).then(() => {
      setCopied(true);
      window.setTimeout(() => { if (alive.current) setCopied(false); }, 1500);
    }).catch(() => {}) ?? Promise.resolve();

  const copyAndOpen = (code: GithubDeviceCodeDto) => {
    void copy(code);
    void openLink(code.verificationUri).catch(() => {});
  };

  const signOut = () => {
    if (busy) return;
    void appConfirm(t.github.signOutConfirm, { title: t.github.signOut, confirmLabel: t.github.signOut, danger: true }).then((yes) => {
      if (!yes) return;
      setBusy("logout");
      githubLogout()
        .then((s) => setStatus(s))
        .catch((e) => setError(formatBackendError(e, t.locale)))
        .finally(() => setBusy(null));
    });
  };

  const modeOptions = [
    { value: "auto" as const, label: t.github.modeAuto },
    { value: "meowo" as const, label: t.github.modeMeowo },
    { value: "gh" as const, label: t.github.modeGh },
    { value: "off" as const, label: t.github.modeOff },
  ];
  const activeText = !status
    ? ""
    : status.active === "meowo"
      ? t.github.activeMeowo(account?.login ?? null)
      : status.active === "off"
        ? t.github.activeOff
        : t.github.activeGh(status.ghInstalled);

  const accountRow = () => {
    if (!status) return null;
    if (pending) {
      return (
        <div className="row github-device" data-testid="github-device">
          <div className="row-text">
            <div className="row-label">{t.github.codeLabel}</div>
            <div className="github-code" data-testid="github-code">{pending.userCode}</div>
            <div className="row-desc is-full">{t.github.codeHint(minutesLeft(pending.expiresAt, Date.now()))}</div>
            <div className="github-actions">
              <button type="button" className="sbtn btn-primary" onClick={() => copyAndOpen(pending)}>{t.github.copyAndOpen}</button>
              <button type="button" className="sbtn" onClick={() => void copy(pending)}>{copied ? t.github.copied : t.github.copyCode}</button>
              <button type="button" className="sbtn" onClick={cancel}>{t.github.cancel}</button>
            </div>
            <div className="row-desc github-waiting" role="status"><span className="github-dot" aria-hidden />{t.github.waiting}</div>
          </div>
        </div>
      );
    }
    if (account) {
      const vault = IS_MAC ? t.github.vaultMac : t.github.vaultWindows;
      return (
        <div className="row" data-testid="github-account">
          <span className="github-avatar" aria-hidden>{account.login.slice(0, 1).toUpperCase()}</span>
          <div className="row-text">
            <div className="row-label">{account.name ? `${account.name} · ${account.login}` : account.login}</div>
            <div className={"row-desc is-full" + (status.needsRelogin ? " github-warn" : "")}>
              {status.needsRelogin ? t.github.reloginDesc : t.github.signedInDesc(vault)}
            </div>
          </div>
          <div className="github-row-actions">
            {status.needsRelogin && (
              <button type="button" className="sbtn btn-primary" {...busyProps(busy === "login", signIn)}>
                {busy === "login" ? t.github.signingIn : t.github.relogin}
              </button>
            )}
            <button type="button" className="sbtn" {...busyProps(busy === "logout", signOut)}>{t.github.signOut}</button>
          </div>
        </div>
      );
    }
    if (!status.credentialSupported) {
      return (
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.github.account}</div>
            <div className="row-desc is-full">{t.github.unsupported}</div>
          </div>
        </div>
      );
    }
    return (
      <div className="row" data-testid="github-signed-out">
        <div className="row-text">
          <div className="row-label">{t.github.account}</div>
          <div className="row-desc is-full">{notice ?? t.github.signedOutDesc}</div>
        </div>
        <button type="button" className="sbtn btn-primary" {...busyProps(busy === "login", signIn)}>
          {busy === "login" ? t.github.signingIn : t.github.signIn}
        </button>
      </div>
    );
  };

  return (
    <>
      <div className="row-card">
        <div className="row github-mode-row">
          <div className="row-text">
            <div className="row-label">{t.github.mode}</div>
            <div className="row-desc is-full">{t.github.modeDesc[mode]}</div>
          </div>
          <Segmented value={mode} options={modeOptions} onChange={changeMode} label={t.github.mode} />
        </div>
        <div className="row">
          <div className="row-text"><div className="row-label">{t.github.active}</div></div>
          <span className="github-active" data-testid="github-active">{activeText}</span>
        </div>
      </div>
      <div className="sec-caption">{t.github.accountCaption}</div>
      <div className="row-card">{accountRow()}</div>
      <div className="sec-hint">{t.github.note}</div>
      <SettingsError preformatted error={error} onDismiss={() => setError(null)} />
    </>
  );
}
