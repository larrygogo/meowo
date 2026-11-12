import { type ReactElement, useEffect, useLayoutEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { useShowWhenReady } from "../useShowWhenReady";
import { type AgentId, agentName, listSubdirectories, type DirListing } from "../api";
import { agentAssets, tintStyle } from "../providers";
import { normalizePath, unquotePath } from "../paths";
import { Dropdown } from "./menu";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { useT } from "../i18n";
import { formatBackendError } from "../i18n/errors";
import { useNewSessionForm } from "./useNewSessionForm";
import { useEscClose } from "../hooks/useEscClose";
import { remoteUi } from "../remoteMode";
import { WorktreeOption } from "./WorktreeOption";
import { Icon } from "../icons/Icon";
import { icFolder } from "../icons/glyphs";
import { busyProps } from "../busyButton";

function FolderIcon() {
  return (
    <Icon glyph={icFolder} size={14} />
  );
}

// 独立窗的初始高度，与 Rust 侧 open_new_session_window_impl 的 inner_size / min_inner_size 一致。
const BASE_H = 520;

/** 独立窗该有多高（CSS px）：表单放得下就是初始高度；放不下就加高到刚好放下，最多加到 cap（屏幕里还剩的地方）。
 *  chrome 是表单以外的固定部分（标题条 + 底部操作栏），need 是表单内容的自然高度。 */
export function fitWindowHeight(chrome: number, need: number, cap: number): number {
  return Math.round(Math.min(Math.max(chrome + need, BASE_H), Math.max(cap, BASE_H)));
}

/** 独立窗口页（label="new-session"）：新建一个全新会话。成功后自关；
 *  「正在启动」的反馈由看板的负 id 占位卡承担（pending PTY 合成，见 session_query）。 */

const qs = new URLSearchParams(window.location.search);
const initialCwd = normalizePath(qs.get("cwd") ?? "");
const initialProvider: AgentId | null = qs.get("provider");

export function NewSessionPanel({ onClose, prefill, onLaunched }: {
  onClose?: () => void;
  /** 远程页内打开时的预填(桌面走 URL query / ns-prefill 事件,远程两条都不通,改走 props)。 */
  prefill?: { cwd?: string | null; provider?: string | null };
  /** 启动成功回调(临时负 id,桌面恒 null)。远程 RemoteApp 借此选中新会话;桌面不传,
   *  reveal 开窗即导航,无需此通道。 */
  onLaunched?: (tempId: number | null) => void;
} = {}): ReactElement {
  const startCwd = prefill?.cwd != null ? normalizePath(prefill.cwd) : initialCwd;
  const startProvider = (prefill?.provider ?? initialProvider) as AgentId | null;
  const t = useT();
  // 窗口以 visible:false 创建（window.rs），首帧渲染后再显示，消除打开瞬间的白框闪烁。
  useShowWhenReady();
  const form = useNewSessionForm(startCwd, startProvider);
  const { cwd, setCwd, provider, setProvider, recent, hooks, busy, phase, repairing, error, setError, agents, avail, opts, setOpts,
    activeProfiles, relayPins, loginOperations, swappedFrom, launchOptions, canLaunch, repairHooks, doLogin, cancelLoginWait, warn, needLogin } = form;
  // 7T-5：面板打开时没有任何初始焦点——目录已经预填好了，回车却什么都不发生（键盘用户
  // 得先 Tab 若干次才摸到控件）。挂载即聚焦目录框；已有预填时连带全选，一次输入就能整段
  // 改写，回车直接启动（Enter 与按钮共用 canLaunch 守卫，见 launch）。
  const dirInputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    // 远程（手机）不抢焦点：聚焦输入框会立刻弹出软键盘，占掉半屏、还把面板顶上去，
    // 而手机上第一步多半是点「最近目录」而不是手打路径（复核指出）。
    if (remoteUi()) return;
    const el = dirInputRef.current;
    if (!el) return;
    el.focus();
    if (el.value) el.select();
  }, []);
  // 表单放不下时把窗口加高（只往下长，不重新居中）。窗口固定 460×520、不可拖大：勾上「在新 worktree 里开」
  // 多出一张约 140px 的卡片，Agent、权限、模型被挤到底栏下面（实测 Agent 钮顶 y=454，底栏从 452 起），
  // 没有任何提示说下面还有东西。不重新居中是因为那会让刚点的勾选框从指针下跑掉。
  // 每次渲染后量一遍：表单高度只随本组件的状态变（勾选、重名警告、接入项警告、报错），量一次很便宜。
  // 手机页内浮层（remoteUi）是整屏的，不归这里管。
  const bodyRef = useRef<HTMLDivElement>(null);
  const requestedH = useRef<number | null>(null);
  const resizing = useRef<Promise<void>>(Promise.resolve());
  useLayoutEffect(() => {
    if (remoteUi()) return;
    const body = bodyRef.current;
    const last = body?.lastElementChild as HTMLElement | null;
    // clientHeight 为 0 = 还没排版（或测试环境没有排版），这时量出来的都是 0，不能据此缩窗。
    if (!body || !last || body.clientHeight === 0) return;
    const need = last.offsetTop + last.offsetHeight + (parseFloat(getComputedStyle(body).paddingBottom) || 0);
    // 最多长到屏幕工作区的下沿（留 8px），再多就让表单自己滚动；量不到屏幕信息时不加高。
    const scr = window.screen as Screen & { availTop?: number };
    const room = (scr.availTop ?? 0) + scr.availHeight - window.screenY - (window.outerHeight - window.innerHeight) - 8;
    const desired = fitWindowHeight(window.innerHeight - body.clientHeight, need, Number.isFinite(room) ? room : BASE_H);
    if (Math.abs(desired - (requestedH.current ?? window.innerHeight)) <= 1) return;
    requestedH.current = desired;
    // Windows 上 resizable(false) 会把窗口 min/max 锁死成当前尺寸，setSize 被钳住，必须临时放开再锁回（同 Updater）。
    // 排队执行：连续两次调整（勾上又马上取消）不能交错。
    resizing.current = resizing.current.then(async () => {
      const w = getCurrentWindow();
      await w.setResizable(true);
      await w.setSize(new LogicalSize(window.innerWidth, desired));
      await w.setResizable(false);
    }).catch(() => { /* 非 Tauri 环境（测试/浏览器）或调整失败：维持原尺寸，表单照旧可滚动 */ });
  });

  // 窗口已开时从另一张卡片再点「新建会话」：后端发 ns-prefill 更新表单（不重开窗口）。
  useTauriEvent<{ cwd?: string | null; provider?: string | null }>("ns-prefill", (e) => {
    if (e.payload.cwd != null) setCwd(normalizePath(e.payload.cwd));
    if (e.payload.provider != null) setProvider(e.payload.provider);
  });


  function closeWin() {
    // 桌面是独立窗口,关窗即销毁;远程是页内浮层,由 onClose 回列表(无 onClose 才退回关窗)。
    if (onClose) {
      onClose();
      return;
    }
    getCurrentWindow().close();
  }
  // Esc 关窗（输入框内让位）：填错想放弃时不必去点右上角 ✕。
  useEscClose(closeWin);

  async function pickDir() {
    const picked = await open({ directory: true });
    if (typeof picked === "string") setCwd(normalizePath(picked));
  }

  // 远程端页内目录浏览:系统目录对话框在浏览器里弹不起来,改为逐级下钻的就地列表。
  const [browse, setBrowse] = useState<DirListing | null>(null);
  // 下钻失败(权限拒绝/断链网络盘)要说出来:静默吞掉的话,点了没反应像卡死(自审 L12)。
  const [browseError, setBrowseError] = useState<string | null>(null);
  function browseTo(path?: string) {
    listSubdirectories(path)
      .then((listing) => {
        setBrowse(listing);
        setBrowseError(null);
      })
      .catch((error) => setBrowseError(formatBackendError(error, t.locale)));
  }
  async function openRemoteBrowser() {
    // 起点取当前输入的目录;不存在/为空则回退磁盘列表,列不了就保持手输。
    try {
      setBrowse(await listSubdirectories(unquotePath(cwd) || undefined));
    } catch {
      try {
        setBrowse(await listSubdirectories());
      } catch (e) {
        // 两级都失败时 browse 仍为 null、browseError 的展示位(浏览面板内)根本不会渲染,
        // 必须借主错误行说出来——否则点了「浏览」毫无反应,与断链/卡死无从区分。
        setError(formatBackendError(e, t.locale));
      }
    }
  }

  async function launch() {
    const launched = await form.launch();
    if (!launched) return;
    // 「正在启动」的可见反馈由看板的占位卡承担（后端把 pending PTY 合成负 id 占位项
    // 合入看板查询，认领出真卡后对账撤下），面板这里启动成功直接自关即可。
    // 远程没有 reveal 这条导航：自关前把临时 id 交回 RemoteApp 选中新会话，
    // 否则用户被丢回「去侧栏选会话」空态。
    onLaunched?.(launched.tempId);
    closeWin();
  }

  // 输入框内容实时过滤最近项：空 / 已选中某项（完全匹配）时显示全部，输入片段时按 名+路径 过滤。
  // 比较前统一 normalize 斜杠方向，避免 C:/proj 与 C:\proj 因分隔符不同而无法高亮/匹配。
  const cwdNorm = normalizePath(cwd.trim());
  const q = cwdNorm.toLowerCase();
  const shownRecent =
    !q || recent.some((r) => r.toLowerCase() === q)
      ? recent
      : recent.filter((r) => r.toLowerCase().includes(q));

  return (
    <div className="ns-window">
      <div className="ns-titlebar" data-tauri-drag-region>
        <span className="ns-title">{t.newSession.title}</span>
        <button type="button" className="ns-close" aria-label={t.newSession.cancel} onClick={closeWin}>
          ×
        </button>
      </div>

      <div className="ns-body" ref={bodyRef}>
        <label className="ns-field">
          <span className="ns-label">{t.newSession.dir}</span>
          <div className="ns-picker">
            <div className="ns-dir-row">
              <input
                ref={dirInputRef}
                className="ns-input"
                data-testid="ns-dir"
                value={cwd}
                placeholder={t.newSession.dirPlaceholder}
                onChange={(e) => { setCwd(e.target.value); setError(null); }}
                // Enter 直接启动（launch 内部对空目录/busy 有守卫），与账号页 API Key 输入框同规。
                // IME 合成守卫：拼音按 Enter 提交候选时先触发 keydown，放行会用半截路径误启动。
                onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing) void launch(); }}
              />
              {/* 桌面走系统目录对话框(plugin-dialog);远程在浏览器里弹不起来,换页内浏览器。 */}
              {!remoteUi() ? (
                <button type="button" className="ns-browse" onClick={pickDir}>
                  {t.newSession.browse}
                </button>
              ) : (
                <button
                  type="button"
                  className="ns-browse"
                  onClick={() => (browse ? setBrowse(null) : void openRemoteBrowser())}
                >
                  {t.newSession.browse}
                </button>
              )}
            </div>
            {browse && (
              <div className="ns-dirbrowse" data-testid="ns-dirbrowse">
                <div className="ns-dirbrowse-head">
                  <button
                    type="button"
                    className="ns-browse"
                    disabled={browse.parent === null}
                    onClick={() => browseTo(browse.parent || undefined)}
                  >
                    {t.newSession.up}
                  </button>
                  {/* 只显当前目录名:整条路径在窄屏必被截断,反而遮住最有信息量的末段。 */}
                  <span className="ns-dirbrowse-path" data-tip={browse.path}>
                    {browse.path
                      ? (browse.path.split(/[\\/]/).filter(Boolean).pop() ?? browse.path)
                      : t.newSession.drives}
                  </span>
                  {browse.path && (
                    <button
                      type="button"
                      className="ns-browse ns-dirbrowse-pick"
                      onClick={() => {
                        setCwd(normalizePath(browse.path));
                        setBrowse(null);
                      }}
                    >
                      {t.newSession.pickHere}
                    </button>
                  )}
                </div>
                {browseError && <div className="ns-dirbrowse-empty ns-dirbrowse-err">{browseError}</div>}
                <div className="ns-dirbrowse-list">
                  {browse.dirs.map((d) => (
                    <button
                      key={d.path}
                      type="button"
                      className="ns-recent-item"
                      onClick={() => browseTo(d.path)}
                    >
                      <FolderIcon />
                      <span className="ns-recent-name">{d.name}</span>
                    </button>
                  ))}
                  {browse.dirs.length === 0 && (
                    <div className="ns-dirbrowse-empty">{t.newSession.noSubdirs}</div>
                  )}
                </div>
              </div>
            )}
            {/* 浏览器展开时顶掉「最近」列表:两块可滚列表叠放又挤又乱。 */}
            {!browse && recent.length > 0 && shownRecent.length > 0 && (              <div className="ns-recent-list">
                {shownRecent.map((r) => (
                  <button
                    key={r}
                    type="button"
                    className={
                      "ns-recent-item"
                      + (cwdNorm === r ? " is-on" : "")
                    }
                    data-tip={r}
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

        {/* 包 5：在新 worktree 里开（默认不勾）。手机上同样可用，目录由桌面端建。 */}
        <div className="ns-field">
          <WorktreeOption choice={form.worktree} disabled={busy} />
        </div>

        <div className="ns-field">
          <span className="ns-label">{t.newSession.agent}</span>
          {avail === null ? (
            // listAgents() 尚未 resolve：给「检测中」占位，而不是一块猜不出含义的空白。
            <div className="ns-agents" data-testid="ns-agents-detecting">
              {t.newSession.detectingAgents}
            </div>
          ) : avail.length === 0 ? (
            // 7T-8：此前是死胡同——一句「没检测到」，没有按钮也没有去处。安装入口在
            // 设置的「账号与用量」，直接把人送过去（引导里早就这么说了，这里漏了）。
            <div className="ns-warn" data-testid="ns-no-agents">
              <span>{t.newSession.noAgents}</span>
              {/* open_settings 是宿主执行类命令，远程桥对它返 null——按钮在手机上是死点击
                  （复核指出）。安装要在宿主跑安装脚本，手机做不了：说明原因，不写「去桌面端」（m10）。 */}
              {remoteUi() ? (
                <span className="ns-warn-remote">{t.newSession.installOnHost}</span>
              ) : (
                <button type="button" className="ns-btn" onClick={() => void invoke("open_settings").catch(() => {})}>
                  {t.newSession.goInstall}
                </button>
              )}
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
                    onClick={() => { setProvider(p); setError(null); }}
                  >
                    {/* currentColor 绘制的徽标（claude）要由容器补品牌色，只染图标不染文字。 */}
                    <span className="ns-agent-mark" style={tintStyle(p)}>
                      <Icon />
                    </span>
                    <span>{agentName(agents ?? [], p)}</span>
                  </button>
                );
              })}
            </div>
          )}
          {/* 7T-9：预填的 agent 没装、被悄悄换成别家时说一句。用户点开面板前就选好了
              要用谁，换掉不告诉他，等于让他用错工具跑一整轮。 */}
          {swappedFrom && avail && avail.length > 0 && (
            <div className="ns-hint" data-testid="ns-agent-swapped">
              {t.newSession.agentSwapped(agentName(agents ?? [], swappedFrom), agentName(agents ?? [], provider))}
            </div>
          )}
          {launchOptions.length > 0 && (
            <div className="ns-options" data-testid="ns-options">
              {/* 启动选项由插件声明（选择 → CLI flag），未声明的 agent 没有这块。
                  choice 文案：i18n 按 `<option>.<choice>` 取，缺省回退后端的产品词 label。
                  用自绘 Dropdown 而非原生 select：WebView2 的原生下拉跟随系统主题画白底，
                  无视页面 color-scheme（终端下拉当年正因此换掉，见 styles.css 的遗留注释）。 */}
              {launchOptions.map((option) => {
                // relay 启用时 model 档由中转固定（置灰 + 注明，理由见 relayPins）。
                const pinned = option.id === "model" ? relayPins[provider] : undefined;
                return (
                <div key={option.id} className="ns-option" data-testid={"ns-option-" + option.id}>
                  <span className="ns-option-label">{t.newSession.launchOption[option.id] ?? option.id}</span>
                  <Dropdown
                    align="left"
                    disabled={pinned !== undefined}
                    value={opts[option.id] ?? option.default}
                    options={option.choices.map((choice) => ({
                      value: choice.id,
                      label: t.newSession.launchChoice[`${option.id}.${choice.id}`] ?? choice.label,
                      // 高风险档（契约 risk=true）警示色 + 副标题，与普通档拉开视觉。
                      risky: choice.risk,
                      // 每档配一行后果说明；没配说明的高风险档退回通用风险提示。
                      sub: t.newSession.launchChoiceDesc[`${option.id}.${choice.id}`]
                        ?? (choice.risk ? t.newSession.riskyChoiceSub : undefined),
                    }))}
                    onChange={(v) => setOpts((m) => ({ ...m, [option.id]: v }))}
                  />
                  {pinned !== undefined && (
                    <span className="ns-option-note" data-testid="ns-option-pinned">{t.newSession.relayPinnedModel(pinned)}</span>
                  )}
                </div>
                );
              })}
            </div>
          )}
          {/* 修复接入项只是 Meowo 写自己的接入配置，2026-11-11 起进了远程白名单（remote.rs 模块头
              「例外」），手机上同样给按钮，不再写「请在桌面端处理」（UI 调研 m10）。警告文字必须留：
              整块藏掉的话，用户点「启动」会起一个 hook 没接、永远不显示状态的哑会话(审查 #10)。 */}
          {avail && avail.length > 0 && warn && (
            <div className="ns-warn" data-testid="ns-hooks-warn">
              <span>{hooks[provider] === "unknown" ? t.newSession.hooksUnknown : t.newSession.hooksMissing}</span>
              <button
                type="button"
                className="ns-repair"
                data-testid="ns-repair-hooks"
                {...busyProps(repairing, repairHooks)}
              >
                {repairing ? t.newSession.repairingHooks : t.newSession.repairHooks}
              </button>
            </div>
          )}
          {avail && avail.length > 0 && needLogin && (
            <div className="ns-warn" data-testid="ns-login-warn">
              {/* 等待中：这行承载「正在等」，按钮则变成「取消等待」。 */}
              <span>{loginOperations.isPending(provider) ? t.newSession.loggingIn : t.newSession.notLoggedIn}</span>
              {/* 登录要在宿主拉起终端、在电脑的浏览器里授权，手机做不了：说明原因（m10）。 */}
              {remoteUi() ? (
                <span className="ns-warn-remote">{t.newSession.loginOnHost}</span>
              ) : (
                <button
                  type="button"
                  className="ns-repair"
                  data-testid="ns-login"
                  // 等待中不再是死按钮：终端可能已被关掉，而后端要 5 分钟才超时。点它即取消等待。
                  onClick={loginOperations.isPending(provider) ? cancelLoginWait : doLogin}
                >
                  {loginOperations.isPending(provider) ? t.newSession.cancelLogin : t.newSession.login}
                </button>
              )}
            </div>
          )}
        </div>

        {/* N-2 的兄弟场景（独立新建窗）：分步状态与新建页同一套文案。 */}
        {busy && phase && (
          <div className="ns-hint" role="status" data-testid="ns-phase">
            {phase === "worktree" ? t.newSession.phaseWorktree : t.newSession.phaseStarting(agentName(agents ?? [], provider))}
          </div>
        )}
        {error && (
          <div className="ns-error" data-testid="ns-error" role="alert">
            {error}
          </div>
        )}
      </div>

      <div className="ns-actions">
        {/* 非默认账号活跃时明说：新会话将写进那个隔离账号。默认账号不显示——零感知底线。 */}
        {activeProfiles[provider] && (
          <span className="ns-profile-hint" data-testid="ns-active-profile">
            {t.newSession.activeProfile(activeProfiles[provider]!)}
          </span>
        )}
        <button type="button" className="ns-btn" onClick={closeWin}>
          {t.newSession.cancel}
        </button>
        <button
          type="button"
          className="ns-btn btn-primary"
          data-testid="ns-launch"
          // 启动中只标 aria-disabled（做空第一轮 #4）：canLaunch 含 !busy，直接 disabled 会在点下的瞬间把焦点踢到 body。
          disabled={!canLaunch && !busy}
          {...busyProps(busy, launch)}
        >
          {busy ? t.newSession.launching : t.newSession.launch}
        </button>
      </div>
    </div>
  );
}
