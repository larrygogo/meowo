// 设置 › 代码托管 › Git：Meowo 实际调用的 git 程序（路径、版本），与全局提交身份（git config --global user.name /
// user.email）。leader 2026-11-18：「把 git 工具和 git 平台都放在这里」，只做这两项，不做别的 git 配置。
// 找不到 git 时说明影响（提交、推送、改动列表、PR 都用不了）并给官网下载链接。后端 git_tool.rs。
import { useCallback, useEffect, useRef, useState } from "react";
import { useFlash } from "../../hooks/useFlash";
import { gitIdentitySet, gitToolStatus, openLink, type GitToolDto } from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { busyProps } from "../../busyButton";
import { SettingsError } from "./widgets";

export const GIT_DOWNLOAD_URL = "https://git-scm.com/downloads";

export function GitToolCard() {
  const t = useT();
  const g = t.codeHosting;
  const [status, setStatus] = useState<GitToolDto | null>(null);
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [busy, setBusy] = useState<"check" | "save" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { shown: saved, flash: flashSaved, hide: hideSaved } = useFlash();
  const alive = useRef(true);
  useEffect(() => () => { alive.current = false; }, []);

  const apply = useCallback((s: GitToolDto) => {
    setStatus(s);
    setName(s.userName ?? "");
    setEmail(s.userEmail ?? "");
  }, []);

  const check = useCallback(() => {
    setBusy("check");
    gitToolStatus()
      .then((s) => { if (alive.current) apply(s); })
      .catch((e) => { if (alive.current) setError(formatBackendError(e, t.locale)); })
      .finally(() => { if (alive.current) setBusy(null); });
  }, [apply, t.locale]);

  useEffect(() => { check(); }, [check]);

  const dirty = !!status && (name.trim() !== (status.userName ?? "") || email.trim() !== (status.userEmail ?? ""));
  const save = () => {
    if (busy || !dirty) return;
    setBusy("save");
    setError(null);
    hideSaved();
    gitIdentitySet(name, email)
      .then((s) => { if (alive.current) { apply(s); flashSaved(); } })
      .catch((e) => { if (alive.current) setError(formatBackendError(e, t.locale)); })
      .finally(() => { if (alive.current) setBusy(null); });
  };

  const found = status?.found ?? false;
  return (
    <>
      <div className="row-card" data-testid="git-tool">
        <div className="row">
          <div className="row-text">
            <div className="row-label">{g.program}</div>
            {!status ? (
              <div className="row-desc is-full">{g.checking}</div>
            ) : found ? (
              <div className="row-desc is-full git-tool-path" data-testid="git-tool-path">
                {status.version ? g.version(status.version) : g.versionUnknown}
                {status.path && <> · <span className="git-tool-mono">{status.path}</span></>}
              </div>
            ) : (
              <>
                <div className="row-desc is-full github-warn" data-testid="git-tool-missing">{g.missing}</div>
                {status.error && <div className="row-desc is-full git-tool-mono">{status.error}</div>}
              </>
            )}
          </div>
          <div className="github-row-actions">
            {status && !found && (
              <button type="button" className="sbtn btn-primary" onClick={() => void openLink(GIT_DOWNLOAD_URL).catch(() => {})}>{g.download}</button>
            )}
            <button type="button" className="sbtn" {...busyProps(busy === "check", check)}>{g.recheck}</button>
          </div>
        </div>
        {found && (
          <div className="git-identity" data-testid="git-identity">
            <div className="row forge-field">
              <label className="row-label" htmlFor="git-user-name">{g.userName}</label>
              <input id="git-user-name" className="ns-input" type="text" autoComplete="off" spellCheck={false}
                value={name} placeholder={g.userNamePlaceholder} onChange={(e) => { setName(e.target.value); hideSaved(); }}
                onKeyDown={(e) => { if (e.key === "Enter") save(); }} />
            </div>
            <div className="row forge-field">
              <label className="row-label" htmlFor="git-user-email">{g.userEmail}</label>
              <input id="git-user-email" className="ns-input" type="email" autoComplete="off" spellCheck={false}
                value={email} placeholder={g.userEmailPlaceholder} onChange={(e) => { setEmail(e.target.value); hideSaved(); }}
                onKeyDown={(e) => { if (e.key === "Enter") save(); }} />
              <div className="row-desc is-full forge-field-note">{g.identityNote}</div>
            </div>
            <div className="row forge-actions">
              {saved && <span className="row-desc git-tool-saved fb-flash" role="status">{g.saved}</span>}
              <button type="button" className="sbtn btn-primary" data-testid="git-identity-save" aria-disabled={busy !== null || !dirty || undefined}
                aria-busy={busy === "save" || undefined} onClick={save}>
                {busy === "save" ? g.saving : g.save}
              </button>
            </div>
          </div>
        )}
      </div>
      <SettingsError preformatted error={error} onDismiss={() => setError(null)} />
    </>
  );
}
