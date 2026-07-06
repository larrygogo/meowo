import { type ReactElement, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
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
import { useT } from "../i18n";

export function NewSessionPanel({
  onClose,
  onLaunched,
}: {
  onClose: () => void;
  onLaunched: (msg: string) => void;
}): ReactElement {
  const t = useT();
  const [cwd, setCwd] = useState("");
  const [provider, setProvider] = useState<ProviderKey>("claude");
  const [terminal, setTerminal] = useState<ResumeTerminal | "">("");
  const [terms, setTerms] = useState<ResumeTerminal[]>([]);
  const [recent, setRecent] = useState<string[]>([]);
  const [hooks, setHooks] = useState<Record<string, HooksStatus>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // 初始：默认 agent/终端来自设置；最近目录、可用终端、各 provider hooks 状态。
  useEffect(() => {
    getSettings().then((s) => {
      setProvider(s.default_agent);
      setTerminal(s.resume_terminal);
    });
    recentCwds(8).then(setRecent).catch(() => {});
    availableTerminals().then(setTerms).catch(() => {});
    PROVIDER_KEYS.forEach((p) =>
      checkProviderHooks(p)
        .then((st) => setHooks((h) => ({ ...h, [p]: st })))
        .catch(() => {}),
    );
  }, []);

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
      onLaunched(msg);
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  const warn = hooks[provider] === "missing" || hooks[provider] === "unknown";

  return (
    <div className="ns-overlay" onMouseDown={onClose}>
      <div className="ns-modal" onMouseDown={(e) => e.stopPropagation()}>
        <div className="ns-title">{t.newSession.title}</div>

        <label className="ns-field">
          <span className="ns-label">{t.newSession.dir}</span>
          <div className="ns-dir-row">
            <input
              className="ns-input"
              data-testid="ns-dir"
              value={cwd}
              placeholder={t.newSession.dirPlaceholder}
              onChange={(e) => setCwd(e.target.value)}
            />
            <button type="button" className="ns-btn" onClick={pickDir}>
              {t.newSession.browse}
            </button>
          </div>
          {recent.length > 0 && (
            <div className="ns-recent">
              <span className="ns-recent-lbl">{t.newSession.recent}</span>
              {recent.map((r) => (
                <button key={r} type="button" className="ns-chip" title={r} onClick={() => setCwd(r)}>
                  {r.split(/[\\/]/).filter(Boolean).pop() ?? r}
                </button>
              ))}
            </div>
          )}
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
          <label className="ns-field">
            <span className="ns-label">{t.newSession.terminal}</span>
            <select
              className="ns-input"
              value={terminal}
              onChange={(e) => setTerminal(e.target.value as ResumeTerminal)}
            >
              {terms.map((tm) => (
                <option key={tm} value={tm}>
                  {tm}
                </option>
              ))}
            </select>
          </label>
        )}

        {error && (
          <div className="ns-error" data-testid="ns-error">
            {error}
          </div>
        )}

        <div className="ns-actions">
          <button type="button" className="ns-btn" onClick={onClose}>
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
    </div>
  );
}
