import { type ReactElement, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import {
  type AgentId,
  type AgentDescriptor,
  type HooksStatus,
  type LoginDone,
  newSession,
  recentCwds,
  checkProviderHooks,
  repairProviderHooks,
  getSettings,
  listAgents,
  agentName,
  getAccounts,
  loginAgent,
  cancelLogin,
  isLoggedIn,
} from "../api";
import { agentAssets } from "../providers";
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
const initialProvider: AgentId | null = qs.get("provider");

export function NewSessionPanel(): ReactElement {
  const t = useT();
  const [cwd, setCwd] = useState(initialCwd);
  // 首帧种子（settings.default_agent resolve 前）。真实默认值由后端给。
  const [provider, setProvider] = useState<AgentId>(initialProvider ?? "claude");
  const [recent, setRecent] = useState<string[]>([]);
  const [hooks, setHooks] = useState<Record<string, HooksStatus>>({});
  const [busy, setBusy] = useState(false);
  const [repairing, setRepairing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // agents: 后端下发的名单（null = 尚未 resolve）。avail = 其中已安装的那些。
  const [agents, setAgents] = useState<AgentDescriptor[] | null>(null);
  const avail = agents === null ? null : agents.filter((a) => a.installed).map((a) => a.id);
  // null = 尚未拿到账号（或取不到）→ 不显示未登录提示，避免误闪/误报。
  const [loggedIn, setLoggedIn] = useState<Record<string, boolean> | null>(null);
  // 正在登录的 provider 集合（而非单个 boolean）：登录期间用户可能切换选中项，事件回来时得认得出
  // 是谁的，否则等待态永远落不回、把别的 agent 的登录按钮一起锁死。用集合而非单值，是因为分别登录
  // 两个 agent 本就该允许并发（各自一个终端、一个后端 watch 线程）。
  const [loginPending, setLoginPending] = useState<Set<AgentId>>(new Set());
  // 哪些 agent 的等待是被用户主动取消的。login-done 只带 ok:bool，分不出「超时」与「取消」；
  // 按 agent 分开是因为两个 agent 本就可以并发登录。
  const cancelledRef = useRef<Set<AgentId>>(new Set());
  // login-done 只订阅一次（切 provider 时 unlisten/relisten 会漏事件），故当前选中项与文案走 ref。
  const providerRef = useRef(provider);
  providerRef.current = provider;
  const tRef = useRef(t);
  tRef.current = t;

  useEffect(() => {
    // 窗口已开时从另一张卡片再点「新建会话」：后端发 ns-prefill 更新表单（不重开窗口）。
    const un = listen<{ cwd?: string | null; provider?: string | null }>("ns-prefill", (e) => {
      if (e.payload.cwd != null) setCwd(normalizePath(e.payload.cwd));
      if (e.payload.provider != null) setProvider(e.payload.provider);
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
    // agent 名单由后端下发。拿到后再据此查 hooks 接线状态与登录态——前端不再自带一份 agent 列表。
    // 失败时保持 agents=null（未探测），UI 既不显示「未检测到已安装」也不禁用启动。
    listAgents()
      .then((list) => {
        setAgents(list);
        for (const { id } of list) {
          checkProviderHooks(id)
            .then((st) => setHooks((h) => ({ ...h, [id]: st })))
            .catch(() => {});
        }
        // 登录态：账号能解析出来就算已登录。取不到就保持 null（不提示），宁可不打扰也不误报未登录。
        getAccounts()
          .then((rows) => {
            const m: Record<string, boolean> = {};
            for (const { id } of list) m[id] = isLoggedIn(rows.find((r) => r.provider === id));
            setLoggedIn(m);
          })
          .catch(() => setLoggedIn(null));
      })
      .catch(() => {});
  }, []);

  // 登录在 detach 的外部终端里完成，拿不到退出码——后端轮询账号解析结果，完成/超时后发 login-done。
  useEffect(() => {
    const un = listen<LoginDone>("login-done", (e) => {
      const p = e.payload.provider;
      // 先无条件清掉**该 provider**的等待态：登录期间用户可能已切走，若按当前选中项过滤就再也
      // 清不掉了（等待态卡死，该 agent 再也点不动登录）。
      setLoginPending((s) => {
        if (!s.has(p)) return s;
        const n = new Set(s);
        n.delete(p);
        return n;
      });
      const cancelled = cancelledRef.current.delete(p); // 取一次即消费掉
      // 登录成功与否是该 provider 的客观事实，与当前选中谁无关。
      // 取消时后端也会再查一次账号——用户可能已经在终端里登完了，只是嫌等得慢。
      if (e.payload.ok) {
        setLoggedIn((m) => ({ ...(m ?? {}), [p]: true }));
      }
      // 但提示只对当前看着的那个 agent 显示，免得用户莫名看到别人的报错。
      if (p !== providerRef.current) return;
      const t = tRef.current.newSession;
      // 三种结局：成功（无提示）/ 被取消 / 超时。后端只带 ok:bool，取消与否由发起方记账。
      setError(e.payload.ok ? null : cancelled ? t.loginCancelled : t.loginTimeout);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

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

  /** 拉起交互式登录。成功 spawn 后不清等待态——等 login-done 事件（或 5 分钟超时 / 用户取消）才落回。 */
  async function doLogin() {
    const target = provider; // 锁定发起时的 provider：之后用户切走了，事件仍要能对上号
    if (loginPending.has(target)) return;
    setLoginPending((s) => new Set(s).add(target));
    cancelledRef.current.delete(target);
    setError(null);
    try {
      await loginAgent(target);
    } catch (e) {
      setError(String(e));
      setLoginPending((s) => {
        const n = new Set(s);
        n.delete(target);
        return n;
      });
    }
  }

  /**
   * 取消等待。终端可能已被关掉（手动关、崩溃、agent 自己退出），而后端只轮询账号文件，
   * 要 5 分钟才超时——这期间按钮一直不可点，用户既不能重来也不知道发生了什么。
   *
   * 不检测「终端还活着吗」：`wt.exe` 拉起窗口后自身立即退出，真正跑登录的是它的孙进程；
   * 而 `powershell -NoExit` 又会一直活着。靠监视进程只会在某些终端上失灵。
   *
   * 收尾由后端 emit `login-done`（它会再查一次账号，真登上了就报 ok:true），故不在此清等待态。
   */
  async function cancelLoginWait() {
    const target = provider;
    if (!loginPending.has(target)) return;
    // 后端的 login-done 只带 ok:bool，分不出「超时」与「被取消」。发起方自己记一笔。
    cancelledRef.current.add(target);
    setError(null);
    try {
      await cancelLogin(target);
    } catch {
      // 命令本身失败（不该发生）：至少别把按钮永久卡在等待态。
      cancelledRef.current.delete(target);
      setLoginPending((s) => {
        const n = new Set(s);
        n.delete(target);
        return n;
      });
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
                const { Icon } = agentAssets(p);
                return (
                  <button
                    key={p}
                    type="button"
                    data-testid={"ns-agent-" + p}
                    className={"ns-agent" + (provider === p ? " is-on" : "")}
                    onClick={() => setProvider(p)}
                  >
                    <Icon />
                    <span>{agentName(agents ?? [], p)}</span>
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
              {/* 等待中：这行承载「正在等」，按钮则变成「取消等待」。 */}
              <span>{loginPending.has(provider) ? t.newSession.loggingIn : t.newSession.notLoggedIn}</span>
              <button
                type="button"
                className="ns-repair"
                data-testid="ns-login"
                // 等待中不再是死按钮：终端可能已被关掉，而后端要 5 分钟才超时。点它即取消等待。
                onClick={loginPending.has(provider) ? cancelLoginWait : doLogin}
              >
                {loginPending.has(provider) ? t.newSession.cancelLogin : t.newSession.login}
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
