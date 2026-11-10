import { useEffect, useRef, useState } from "react";
import {
  type AgentId,
  type AgentDescriptor,
  type HooksStatus,
  newSession,
  recentCwds,
  checkProviderHooks,
  repairProviderHooks,
  getSettings,
  listAgents,
  getAccounts,
  isLoggedIn,
} from "../api";
import { normalizePath, pathKey, unquotePath } from "../paths";
import { useAgentListRefresh } from "../useAgents";
import { useLoginOperations } from "../hooks/useLoginOperations";
import { useT, repairFailMessage } from "../i18n";
import { formatBackendError } from "../i18n/errors";
import { useWorktreeChoice } from "./useWorktreeChoice";

/// 新建会话的表单状态与动作（目录、agent、启动选项、hook / 登录检查、启动）。
/// 独立窗口面板（NewSessionPanel，手机页内浮层与关闭对话功能时用）和对话窗里的
/// 新建会话页（NewSessionPage，对照 Kimi：不弹窗，主区就地开）共用这一份，两边只管长相。


/** 启动选项记忆（provider → {option id → choice id}）。常用「跳过权限确认 / 指定模型」的
 *  用户每开一个会话都要重选两次下拉，且极易忘记而以默认档起会话——恢复路径早有持久化
 *  （会话存档回放），新建路径此前完全没有。localStorage 即可：这是 UI 预填偏好，
 *  未知 id 由后端声明表忽略/落默认，脏数据无害。 */
/// 设置页「会话」里的新建会话默认值读写的也是这一份（对照 Kimi 的「Agent 默认值」）：
/// 两边改的是同一件事，新建时改了会记住，设置里改了下次新建就预选它。
const LAUNCH_OPTS_KEY = "meowo-launch-selections";
export function loadStoredOpts(): Record<string, Record<string, string>> {
  try {
    const raw = JSON.parse(localStorage.getItem(LAUNCH_OPTS_KEY) ?? "{}");
    return raw && typeof raw === "object" && !Array.isArray(raw) ? raw : {};
  } catch {
    return {};
  }
}
export function saveStoredOpts(provider: string, opts: Record<string, string>) {
  try {
    localStorage.setItem(LAUNCH_OPTS_KEY, JSON.stringify({ ...loadStoredOpts(), [provider]: opts }));
  } catch {
    /* 隐私模式禁写：只失去记忆 */
  }
}

export function useNewSessionForm(startCwd: string, startProvider: AgentId | null) {
  const t = useT();
  const [cwd, setCwd] = useState(startCwd);
  // 首帧种子（settings.default_agent resolve 前）。真实默认值由后端给。
  const [provider, setProvider] = useState<AgentId>(startProvider ?? "claude");
  const [recent, setRecent] = useState<string[]>([]);
  const [hooks, setHooks] = useState<Record<string, HooksStatus>>({});
  const [busy, setBusy] = useState(false);
  // state 要到下一次 render 才更新；同一事件批次里的双击必须用 ref 同步挡住第二次 IPC。
  const launchPendingRef = useRef(false);
  const [repairing, setRepairing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // agents: 后端下发的名单（null = 尚未 resolve）。avail = 其中已安装的那些。
  const [agents, setAgents] = useState<AgentDescriptor[] | null>(null);
  const avail = agents === null ? null : agents.filter((a) => a.installed).map((a) => a.id);
  // 启动选项的选择（option id → choice id）。选项表由插件经 descriptor 声明；换 agent 清空
  // ——不同 agent 的选项 id 可能撞名（如都叫 approval）但语义不同，残留会串。
  const [opts, setOpts] = useState<Record<string, string>>({});
  // null = 尚未拿到账号（或取不到）→ 不显示未登录提示，避免误闪/误报。
  const [loggedIn, setLoggedIn] = useState<Record<string, boolean> | null>(null);
  // 各 provider 当前活跃账号的展示名（null = 默认账号）。非默认账号活跃时在启动按钮旁提示，
  // 防「切过一次账号就忘了，新会话全写进隔离账号」。
  const [activeProfiles, setActiveProfiles] = useState<Record<string, string | null>>({});
  // relay 启用的 provider → 中转固定的模型。中转的 --model 在 argv 里压轴（terminal.rs），
  // 面板选的模型静默不生效——model 档置灰并注明固定值，不留「选了却没生效」的坑。
  const [relayPins, setRelayPins] = useState<Record<string, string>>({});
  // 「在新 worktree 里开」（包 5），默认不勾。
  const worktree = useWorktreeChoice(cwd);
  const loginOperations = useLoginOperations((event) => {
    const p = event.provider;
    // 登录成功与否是该 provider 的客观事实，与当前选中谁无关。
    // 取消时后端也会再查一次账号——用户可能已经在终端里登完了，只是嫌等得慢。
    if (event.outcome === "success") {
      setLoggedIn((m) => ({ ...(m ?? {}), [p]: true }));
    }
    // 提示只对当前看着的那个 agent 显示，免得用户莫名看到别人的报错。
    if (p !== provider) return;
    setError(event.outcome === "success"
      ? null
      : event.outcome === "cancelled" ? t.newSession.loginCancelled : t.newSession.loginTimeout);
  });

  // agent 名单由后端下发。拿到后再据此查 hooks 接线状态与登录态——前端不再自带一份 agent 列表。
  // 失败时保持 agents=null（未探测），UI 既不显示「未检测到已安装」也不禁用启动。
  const reloadAgents = () => {
    listAgents()
      .then((list) => {
        setAgents(list);
        for (const { id } of list) {
          checkProviderHooks(id)
            .then((st) => setHooks((h) => ({ ...h, [id]: st })))
            .catch(() => {});
        }
        // 登录态：账号能解析出来就算已登录。取不到就保持 null（不提示），宁可不打扰也不误报未登录。
        //
        // 只给**有账号能力**的 agent 记登录态。`getAccounts()` 压根不会返回没声明该能力的 agent
        // （gemini / opencode），而「查不到行」≠「未登录」——它是「无账号概念，无从谈起」。
        // 曾经把两者混为一谈：查不到 → isLoggedIn(undefined) → false → 亮出登录入口 → 点下去，
        // 后端 `login_argv()` 却是 None，只能报「拉起登录失败」。留 undefined，needLogin 即为 false。
        Promise.all([getAccounts(), getSettings()])
          .then(([rows, settings]) => {
            const m: Record<string, boolean> = {};
            const ap: Record<string, string | null> = {};
            for (const { id } of list) {
              const row = rows.find((r) => r.provider === id);
              if (row) m[id] = isLoggedIn(row);
              ap[id] = row?.active_profile_name ?? null;
            }
            setLoggedIn(m);
            setActiveProfiles(ap);
            // relay_enabled 与后端 augment_argv 同一条判定（含密钥已存）；固定值取设置的
            // 中转模型。enabled 但取不到 model 时宁可不置灰（误锁比漏提示更糟）。
            const pins: Record<string, string> = {};
            for (const row of rows) {
              const rule = settings.relay?.per_agent?.[row.provider as AgentId];
              if (row.relay_enabled && rule?.model) pins[row.provider] = rule.model;
            }
            setRelayPins(pins);
          })
          .catch(() => setLoggedIn(null));
      })
      .catch(() => {});
  };

  useEffect(() => {
    // 若从会话卡片菜单带 provider 参数打开，保留该参数；否则回退到设置里的默认 agent。
    if (!startProvider) {
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
      .then((list) => {
        setRecent(list);
        // 无 prefill 时默认选中最近一个目录（保持可改）：从托盘/空态新建时表单原本是
        // 空的，必须先点一次最近项或浏览，而九成场景就是「上次那个目录」。
        // 函数式读当前值：只在用户还没填过时预填，绝不覆盖手动输入。
        if (list.length > 0) setCwd((current) => current || list[0]);
      })
      .catch(() => {});
    reloadAgents();
  }, []);
  // 装完一个 agent，这里的可选项就该多一个——不必关掉面板重开。
  useAgentListRefresh(reloadAgents);

  // default_agent 若未装，则退到首个已装 agent（avail 加载后校正）。
  // 7T-9：换过这一手此前完全静默——用户从卡片菜单带着 codex 的预填点进来，面板上
  // 选中的却是 claude，没有一个字解释。记下「本来要用谁」，在 Agent 区下说一句。
  const [swappedFrom, setSwappedFrom] = useState<string | null>(null);
  useEffect(() => {
    if (avail && avail.length > 0 && !avail.includes(provider)) {
      setSwappedFrom(provider);
      setProvider(avail[0]);
    }
  }, [avail, provider]);
  // 换 agent 重置选择（不同 agent 的选项 id 可能撞名但语义不同，残留会串），
  // 但优先回填本 provider 上次的选择——见 LAUNCH_OPTS_KEY。
  useEffect(() => setOpts(loadStoredOpts()[provider] ?? {}), [provider]);
  const launchOptions = agents?.find((a) => a.id === provider)?.launch_options ?? [];
  // 7T-6：Enter 与「启动」钮此前各判各的——按钮 disabled 含「一个 agent 都没装/还在
  // 检测」，Enter 路径只看目录和 busy，于是在没装 CLI 的机器上回车照样发 newSession，
  // 拿一条后端错误回来。守卫抽出来，两条路径共用。
  const canLaunch = Boolean(cwd.trim()) && !busy && (avail?.length ?? 0) > 0;

  /** 启动会话。成功返回 { tempId }（临时负 id；旧后端桌面回 null），失败把原因写进 error、返回 null。
   *  成功后保持 busy：调用方紧接着关面板 / 切去新会话，不给第二次点击留缝。 */
  async function launch(): Promise<{ tempId: number | null } | null> {
    if (!canLaunch || launchPendingRef.current) return null;
    launchPendingRef.current = true;
    setBusy(true);
    setError(null);
    try {
      // 输入框里躺的是用户原样粘的串:资源管理器「复制为路径」带引号,后端按字面
      // 找不到(7T-4)。只剥引号,不动斜杠方向——发给后端的 cwd 保持用户写法。
      let dir = unquotePath(cwd);
      // 勾了「在新 worktree 里开」：先建 worktree（同步，≤2s），会话照常开在新目录里；
      // setup 在后台跑，进度在会话页顶部看。不勾时这一步不走，与改前完全一样。
      if (worktree.enabled) dir = (await worktree.create(dir, provider)).path;
      const tempId = await newSession(dir, provider, opts);
      saveStoredOpts(provider, opts); // 启动成功才记：失败的组合不该成为下次的默认
      return { tempId };
    } catch (e) {
      launchPendingRef.current = false;
      setError(formatBackendError(e, t.locale));
      setBusy(false);
      return null;
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
      setError(formatBackendError(e, t.locale));
    } finally {
      setRepairing(false);
    }
  }

  /** 拉起交互式登录。成功 spawn 后不清等待态——等 login-done 事件（或 5 分钟超时 / 用户取消）才落回。 */
  async function doLogin() {
    const target = provider; // 锁定发起时的 provider：之后用户切走了，事件仍要能对上号
    if (loginOperations.isPending(target)) return;
    setError(null);
    try {
      await loginOperations.start(target);
    } catch (e) {
      setError(formatBackendError(e, t.locale));
    }
  }

  /**
   * 取消等待。终端可能已被关掉（手动关、崩溃、agent 自己退出），而后端只轮询账号文件，
   * 要 5 分钟才超时——这期间按钮一直不可点，用户既不能重来也不知道发生了什么。
   *
   * 不检测「终端还活着吗」：`wt.exe` 拉起窗口后自身立即退出，真正跑登录的是它的孙进程；
   * 而 `powershell -NoExit` 又会一直活着。靠监视进程只会在某些终端上失灵。
   *
   * 收尾由后端 emit 带 operationId 的 `login-done`，故不在此抢先清等待态。
   */
  async function cancelLoginWait() {
    const target = provider;
    if (!loginOperations.isPending(target)) return;
    setError(null);
    try {
      await loginOperations.cancel(target);
    } catch { /* hook 已解锁该 provider，允许用户重试 */ }
  }

  const warn = hooks[provider] === "missing" || hooks[provider] === "unknown";
  // 已装但未登录才提示（loggedIn 为 null = 拿不到账号，不打扰）。
  const needLogin = loggedIn?.[provider] === false;

  return {
    cwd, setCwd, provider, setProvider, recent, hooks, busy, repairing, error, setError,
    agents, avail, opts, setOpts, loggedIn, activeProfiles, relayPins, loginOperations,
    swappedFrom, launchOptions, canLaunch, launch, repairHooks, doLogin, cancelLoginWait, warn, needLogin,
    worktree,
  };
}
