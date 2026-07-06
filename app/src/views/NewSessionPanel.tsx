import { type ReactElement, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  type ProviderKey,
  type HooksStatus,
  type ResumeTerminal,
  PROVIDER_KEYS,
  newSession,
  recentCwds,
  checkProviderHooks,
  availableTerminals,
  getSettings,
} from "../api";
import { providerConfig } from "../providers";
import { Dropdown } from "../Dropdown";
import { useT } from "../i18n";

function FolderIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
    </svg>
  );
}

/** 独立窗口页（label="new-session"）：新建一个全新会话。成功后 emit 通知主看板弹 toast 并自关。 */
export function NewSessionPanel(): ReactElement {
  const t = useT();
  const [cwd, setCwd] = useState("");
  const [provider, setProvider] = useState<ProviderKey>("claude");
  const [terminal, setTerminal] = useState<ResumeTerminal | "">("");
  const [terms, setTerms] = useState<ResumeTerminal[]>([]);
  const [recent, setRecent] = useState<string[]>([]);
  const [hooks, setHooks] = useState<Record<string, HooksStatus>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getSettings()
      .then((s) => {
        setProvider(s.default_agent);
        setTerminal(s.resume_terminal);
      })
      .catch(() => {});
    recentCwds(4).then(setRecent).catch(() => {});
    availableTerminals().then(setTerms).catch(() => {});
    PROVIDER_KEYS.forEach((p) =>
      checkProviderHooks(p)
        .then((st) => setHooks((h) => ({ ...h, [p]: st })))
        .catch(() => {}),
    );
  }, []);

  function closeWin() {
    getCurrentWindow().close();
  }

  async function pickDir() {
    const picked = await open({ directory: true });
    if (typeof picked === "string") setCwd(picked);
  }

  async function launch() {
    if (!cwd.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      await newSession(cwd.trim(), provider, terminal || undefined);
      const label = providerConfig(provider).label(t);
      const msg =
        provider === "codex"
          ? t.newSession.launchedCodexToast(label)
          : t.newSession.launchedToast(label);
      await emit("new-session-launched", msg);
      closeWin();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  // 输入框内容实时过滤最近项：空 / 已选中某项（完全匹配）时显示全部，输入片段时按 名+路径 过滤。
  const q = cwd.trim().toLowerCase();
  const shownRecent =
    !q || recent.some((r) => r.toLowerCase() === q)
      ? recent
      : recent.filter((r) => r.toLowerCase().includes(q));
  // 终端类型 key → 友好名（与设置页 About.tsx 的映射一致）。
  const termLabel: Record<ResumeTerminal, string> = {
    wt: "Windows Terminal",
    wezterm: "WezTerm",
    powershell: "PowerShell",
    cmd: t.settings.cmdPrompt,
    terminal: "Terminal",
    iterm: "iTerm2",
  };
  const warn = hooks[provider] === "missing" || hooks[provider] === "unknown";

  return (
    <div className="ns-window">
      <div className="ns-titlebar" data-tauri-drag-region>
        <span className="ns-title">{t.newSession.title}</span>
        <button type="button" className="ns-close" aria-label={t.newSession.cancel} onClick={closeWin}>
          ×
        </button>
      </div>

      <div className="ns-body">
        <label className="ns-field">
          <span className="ns-label">{t.newSession.dir}</span>
          <div className="ns-picker">
            <div className="ns-dir-row">
              <input
                className="ns-input"
                data-testid="ns-dir"
                value={cwd}
                placeholder={t.newSession.dirPlaceholder}
                onChange={(e) => setCwd(e.target.value)}
              />
              <button type="button" className="ns-browse" onClick={pickDir}>
                {t.newSession.browse}
              </button>
            </div>
            {recent.length > 0 && shownRecent.length > 0 && (
              <div className="ns-recent-list">
                {shownRecent.map((r) => (
                  <button
                    key={r}
                    type="button"
                    className={"ns-recent-item" + (cwd.trim() === r ? " is-on" : "")}
                    title={r}
                    onClick={() => setCwd(r)}
                  >
                    <FolderIcon />
                    <span className="ns-recent-name">{r.split(/[\\/]/).filter(Boolean).pop() ?? r}</span>
                    <span className="ns-recent-path">{r}</span>
                  </button>
                ))}
              </div>
            )}
          </div>
        </label>

        <div className="ns-field">
          <span className="ns-label">{t.newSession.agent}</span>
          <div className="ns-agents">
            {PROVIDER_KEYS.map((p) => {
              const cfg = providerConfig(p);
              return (
                <button
                  key={p}
                  type="button"
                  className={"ns-agent" + (provider === p ? " is-on" : "")}
                  onClick={() => setProvider(p)}
                >
                  <cfg.Icon />
                  <span>{cfg.label(t)}</span>
                </button>
              );
            })}
          </div>
          {warn && (
            <div className="ns-warn" data-testid="ns-hooks-warn">
              {hooks[provider] === "unknown" ? t.newSession.hooksUnknown : t.newSession.hooksMissing}
            </div>
          )}
        </div>

        {terms.length >= 2 && (
          <div className="ns-field">
            <span className="ns-label">{t.newSession.terminal}</span>
            <Dropdown
              value={terminal as ResumeTerminal}
              options={terms.map((tm) => ({ value: tm, label: termLabel[tm] }))}
              onChange={(v) => setTerminal(v)}
            />
          </div>
        )}

        {error && (
          <div className="ns-error" data-testid="ns-error">
            {error}
          </div>
        )}
      </div>

      <div className="ns-actions">
        <button type="button" className="ns-btn" onClick={closeWin}>
          {t.newSession.cancel}
        </button>
        <button
          type="button"
          className="ns-btn is-primary"
          data-testid="ns-launch"
          disabled={!cwd.trim() || busy}
          onClick={launch}
        >
          {busy ? t.newSession.launching : t.newSession.launch}
        </button>
      </div>
    </div>
  );
}
