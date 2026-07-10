import { type ReactElement, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import {
  type ProviderKey,
  type HooksStatus,
  type LoginDone,
  PROVIDER_KEYS,
  newSession,
  recentCwds,
  checkProviderHooks,
  repairProviderHooks,
  getSettings,
  availableAgents,
  getAccounts,
  loginAgent,
  isLoggedIn,
} from "../api";
import { providerConfig } from "../providers";
import { useT, repairFailMessage } from "../i18n";

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

/** 统一路径分隔符：Windows 路径用反斜杠，Unix 路径用正斜杠。
 *  用于消除 URL 参数/前端输入与后端数据库中 cwd 的斜杠方向不一致。 */
function normalizePath(p: string): string {
  if (!p) return p;
  if (/^[A-Za-z]:/.test(p)) {
    return p.replace(/\//g, "\\");
  }
  return p.replace(/\\/g, "/");
}

/** 用于去重的路径 key：Windows 路径忽略大小写。 */
function pathKey(p: string): string {
  return /^[A-Za-z]:/.test(p) ? p.toLowerCase() : p;
}

/** 独立窗口页（label="new-session"）：新建一个全新会话。成功后 emit 通知主看板弹 toast 并自关。 */

const qs = new URLSearchParams(window.location.search);
const initialCwd = normalizePath(qs.get("cwd") ?? "");
const initialProvider = (qs.get("provider") as ProviderKey | null) ?? null;

export function NewSessionPanel(): ReactElement {
  const t = useT();
  const [cwd, setCwd] = useState(initialCwd);
  const [provider, setProvider] = useState<ProviderKey>(initialProvider ?? "claude");
  const [recent, setRecent] = useState<string[]>([]);
  const [hooks, setHooks] = useState<Record<string, HooksStatus>>({});
  const [busy, setBusy] = useState(false);
  const [repairing, setRepairing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [avail, setAvail] = useState<ProviderKey[] | null>(null);
  // null = 尚未拿到账号（或取不到）→ 不显示未登录提示，避免误闪/误报。
  const [loggedIn, setLoggedIn] = useState<Record<string, boolean> | null>(null);
  const [loginBusy, setLoginBusy] = useState(false);

  useEffect(() => {
    // 窗口已开时从另一张卡片再点「新建会话」：后端发 ns-prefill 更新表单（不重开窗口）。
    const un = listen<{ cwd?: string | null; provider?: string | null }>("ns-prefill", (e) => {
      if (e.payload.cwd != null) setCwd(normalizePath(e.payload.cwd));
      if (e.payload.provider != null) setProvider(e.payload.provider as ProviderKey);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    // 若从会话卡片菜单带 provider 参数打开，保留该参数；否则回退到设置里的默认 agent。
    if (!initialProvider) {
      getSettings()
        .then((s) => setProvider(s.default_agent))
        .catch(() => {});
    }
    recentCwds(8)
      .then((list) => {
        // 后端按原始字符串去重；同一目录可能因历史数据斜杠方向不同而重复。
        // 前端 normalize 后再按大小写不敏感（Windows）去重一次。
        const seen = new Set<string>();
        return list
          .map(normalizePath)
          .filter((p) => {
            const key = pathKey(p);
            if (seen.has(key)) return false;
            seen.add(key);
            return true;
          });
      })
      .then(setRecent)
      .catch(() => {});
    PROVIDER_KEYS.forEach((p) =>
      checkProviderHooks(p)
        .then((st) => setHooks((h) => ({ ...h, [p]: st })))
        .catch(() => {}),
    );
    // 命令失败时按 spec §5 宁可多列（回退全量 PROVIDER_KEYS）也不空列表——空列表会显示「未检测到已安装」并禁用启动。
    availableAgents().then(setAvail).catch(() => setAvail([...PROVIDER_KEYS]));
    // 登录态：账号能解析出来就算已登录。取不到就保持 null（不提示），宁可不打扰也不误报未登录。
    getAccounts()
      .then((rows) => {
        const m: Record<string, boolean> = {};
        for (const p of PROVIDER_KEYS) m[p] = isLoggedIn(rows.find((r) => r.provider === p));
        setLoggedIn(m);
      })
      .catch(() => setLoggedIn(null));
  }, []);

  // 登录在 detach 的外部终端里完成，拿不到退出码——后端轮询账号解析结果，完成/超时后发 login-done。
  useEffect(() => {
    const un = listen<LoginDone>("login-done", (e) => {
      if (e.payload.provider !== provider) return;
      setLoginBusy(false);
      if (e.payload.ok) {
        setLoggedIn((m) => ({ ...(m ?? {}), [e.payload.provider]: true }));
        setError(null);
      } else {
        setError(t.newSession.loginTimeout); // 超时 ≠ 登录失败：用户可能中途放弃了
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, [provider, t]);

  // default_agent 若未装，则退到首个已装 agent（avail 加载后校正）
  useEffect(() => {
    if (avail && avail.length > 0 && !avail.includes(provider)) setProvider(avail[0]);
  }, [avail, provider]);

  function closeWin() {
    getCurrentWindow().close();
  }

  async function pickDir() {
    const picked = await open({ directory: true });
    if (typeof picked === "string") setCwd(normalizePath(picked));
  }

  async function launch() {
    if (!cwd.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      await newSession(cwd.trim(), provider);
      closeWin();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  async function repairHooks() {
    if (repairing) return;
    setRepairing(true);
    setError(null);
    try {
      const res = await repairProviderHooks(provider);
      setHooks((h) => ({ ...h, [provider]: res.status }));
      // 修复后仍非 installed → 接线没真正生效，别让用户以为「点了没反应」；
      // 按后端 reason 给出精准提示（如 kimi 未登录 → 「请先登录」）。
      if (res.status !== "installed") setError(repairFailMessage(t, res.reason));
    } catch (e) {
      setError(String(e));
    } finally {
      setRepairing(false);
    }
  }

  /** 拉起交互式登录。成功 spawn 后不清 busy——等 login-done 事件（或超时）才落回。 */
  async function doLogin() {
    if (loginBusy) return;
    setLoginBusy(true);
    setError(null);
    try {
      await loginAgent(provider);
    } catch (e) {
      setError(String(e));
      setLoginBusy(false);
    }
  }

  // 输入框内容实时过滤最近项：空 / 已选中某项（完全匹配）时显示全部，输入片段时按 名+路径 过滤。
  // 比较前统一 normalize 斜杠方向，避免 C:/proj 与 C:\proj 因分隔符不同而无法高亮/匹配。
  const cwdNorm = normalizePath(cwd.trim());
  const q = cwdNorm.toLowerCase();
  const shownRecent =
    !q || recent.some((r) => r.toLowerCase() === q)
      ? recent
      : recent.filter((r) => r.toLowerCase().includes(q));
  const warn = hooks[provider] === "missing" || hooks[provider] === "unknown";
  // 已装但未登录才提示（loggedIn 为 null = 拿不到账号，不打扰）。
  const needLogin = loggedIn?.[provider] === false;

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
                    className={"ns-recent-item" + (cwdNorm === r ? " is-on" : "")}
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
          {avail && avail.length === 0 ? (
            <div className="ns-warn" data-testid="ns-no-agents">
              {t.newSession.noAgents}
            </div>
          ) : (
            <div className="ns-agents">
              {(avail ?? []).map((p) => {
                const cfg = providerConfig(p);
                return (
                  <button
                    key={p}
                    type="button"
                    data-testid={"ns-agent-" + p}
                    className={"ns-agent" + (provider === p ? " is-on" : "")}
                    onClick={() => setProvider(p)}
                  >
                    <cfg.Icon />
                    <span>{cfg.label(t)}</span>
                  </button>
                );
              })}
            </div>
          )}
          {avail && avail.length > 0 && warn && (
            <div className="ns-warn" data-testid="ns-hooks-warn">
              <span>{hooks[provider] === "unknown" ? t.newSession.hooksUnknown : t.newSession.hooksMissing}</span>
              <button
                type="button"
                className="ns-repair"
                data-testid="ns-repair-hooks"
                onClick={repairHooks}
                disabled={repairing}
              >
                {repairing ? t.newSession.repairingHooks : t.newSession.repairHooks}
              </button>
            </div>
          )}
          {avail && avail.length > 0 && needLogin && (
            <div className="ns-warn" data-testid="ns-login-warn">
              <span>{t.newSession.notLoggedIn}</span>
              <button
                type="button"
                className="ns-repair"
                data-testid="ns-login"
                onClick={doLogin}
                disabled={loginBusy}
              >
                {loginBusy ? t.newSession.loggingIn : t.newSession.login}
              </button>
            </div>
          )}
        </div>

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
          disabled={!cwd.trim() || busy || (avail?.length ?? 0) === 0}
          onClick={launch}
        >
          {busy ? t.newSession.launching : t.newSession.launch}
        </button>
      </div>
    </div>
  );
}
