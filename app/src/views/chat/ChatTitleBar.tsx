/**
 * 对话窗标题栏（从 ChatWindow.tsx 拆出，行为不变）：外壳 + 右侧一排工具。
 * 左侧的标题菜单绑着十几个会话动作，留在 ChatWindow 拼好后以 `title` 插槽传进来。
 *
 * 右侧照 Kimi：[标记完成] [结束会话] [用编辑器打开 ▾] [分支 ↑领先 +增-删] [PR] [终端] [侧边面板]。
 * 「标记完成」「结束会话」是 Meowo 自己的收尾动作，放在 Kimi 那组前面。
 */
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { flushSync } from "react-dom";
import { getPullRequest, openLink, type GitDiffSummaryDto, type PullRequestDto } from "../../api";
import { useT } from "../../i18n";
import { remoteUi } from "../../remoteMode";
import { ArchiveIcon } from "../sticker/icons";
import { GitStatusChip, OpenerIcon, OpenInEditor, PullRequestChip, useFolderOpeners } from "./TitleBarTools";
import { planTitleBar, PROJECT_WANT_EM, sameOverflow, TITLE_WANT_EM, TitleOverflowContext, type TitleCompactKey, type TitleCompactSaving, type TitleOverflowItem, type TitleOverflowKey } from "./titleOverflow";

const NO_PLAN: { hidden: ReadonlySet<TitleOverflowKey>; compact: ReadonlySet<TitleCompactKey> } = { hidden: new Set(), compact: new Set() };

/** 各信息档对应栏上哪些元素（收起后绝对定位、不可见，宽度照样量得到，见 styles.css 的 data-compact 规则）。 */
const COMPACT_PARTS: readonly { step: TitleCompactKey; selector: string }[] = [
  { step: "prState", selector: ".tb-pr-state, .tb-git-pill.is-ahead, .tb-git-pill.is-behind" },
  { step: "branch", selector: ".tb-git-branch" },
];

/// 量标题栏、算哪些信息先收、哪些功能进 ⋯（规则见 titleOverflow.planTitleBar）。
/// 收起的东西都不卸载：功能项加 aria-hidden 退出排版流（.chat-bar-slot[aria-hidden]），
/// 信息档由栏上的 data-compact 让对应元素退出排版流，所以每样东西的自然宽度随时量得到：
/// 不用「先全显示量一遍、再收、再量」，没有来回抖。交给 planTitleBar 的宽度一律折回
/// 「一档都没收」时的自然值，所以收不收都算出同一组，结果收敛，setState 原样返回不重渲染。
/// 「结束会话」收成图标后内边距也换了，字样两侧的留白量不到，沿用上次带字时量到的差值。
/// 首帧在 layout effect 里同步量；之后栏宽、各项尺寸（语言、--cc-ui 缩放、PR/改动数到达）
/// 变化走 ResizeObserver，增删子项与改名走 MutationObserver，都用 flushSync 赶在绘制前收好，
/// 不让出界的一帧闪出来。jsdom 没有布局，宽度都是 0，按「全部放得下」处理。
function useTitleBarOverflow(ref: RefObject<HTMLElement | null>, remote: boolean): typeof NO_PLAN {
  const [plan, setPlan] = useState(NO_PLAN);
  useLayoutEffect(() => {
    const bar = ref.current;
    if (!bar) return;
    // 「结束会话」带字时：按钮宽 - 字样宽；只剩图标时的按钮宽。两种形态各只能量到一个，互相记着。
    const end = { extra: 0, icon: 0 };
    const compute = () => {
      const style = getComputedStyle(bar);
      const applied = new Set((bar.dataset.compact ?? "").split(" ").filter(Boolean) as TitleCompactKey[]);
      const available = bar.getBoundingClientRect().width - (parseFloat(style.paddingLeft) || 0) - (parseFloat(style.paddingRight) || 0);
      const fixed: number[] = [];
      const items: { key: TitleOverflowKey; width: number }[] = [];
      const savings: TitleCompactSaving[] = [];
      let titleMin = 0;
      let titleWant = 0;
      const widthOf = (el: Element) => el.getBoundingClientRect().width;
      const gapOf = (el: Element | null) => (el ? parseFloat(getComputedStyle(el).columnGap) || 0 : 0);
      for (const child of Array.from(bar.children) as HTMLElement[]) {
        const key = child.dataset.overflowKey as TitleOverflowKey | undefined;
        let width = widthOf(child);
        if (key) {
          for (const part of COMPACT_PARTS) {
            for (const el of Array.from(child.querySelectorAll(part.selector))) {
              const w = widthOf(el);
              if (!(w > 0)) continue; // display:none（手机上的分支名）：本来就不占位置
              const saving = w + gapOf(el.parentElement);
              savings.push({ step: part.step, owner: key, saving });
              if (applied.has(part.step)) width += saving;
            }
          }
          items.push({ key, width });
        } else if (child.classList.contains("chat-title-menu")) {
          titleMin = parseFloat(getComputedStyle(child).minWidth) || 0;
          const gap = gapOf(child);
          let count = 0;
          for (const part of Array.from(child.children) as HTMLElement[]) {
            const cls = part.classList;
            const em = parseFloat(getComputedStyle(part).fontSize) || 0;
            // scrollWidth 是整段文字的宽度，省略、收起都不影响；取整有 1px 误差，补上免得标题差半个像素出省略号。
            const w = cls.contains("chat-title") ? Math.min(part.scrollWidth + 1, TITLE_WANT_EM * em)
              : cls.contains("chat-title-project") ? Math.min(part.scrollWidth + 1, PROJECT_WANT_EM * em)
              : cls.contains("chat-title-sep") || cls.contains("chat-title-agent") || cls.contains("chat-title-btn") ? widthOf(part)
              : 0;
            if (!(w > 0)) continue;
            titleWant += w + (count++ > 0 ? gap : 0);
            if (cls.contains("chat-title-project") || cls.contains("chat-title-sep")) savings.push({ step: "project", owner: "title", saving: w + gap });
          }
        } else if (child.classList.contains("chat-end")) {
          const label = child.querySelector(".chat-end-label");
          const labelWidth = label ? widthOf(label) : 0;
          if (applied.has("endLabel")) { end.icon = width; width = labelWidth + end.extra; }
          else end.extra = width - labelWidth;
          // 还没见过图标形态时按按钮高度估（图标形态是正方形）；估偏了下一轮会用实测值再算一遍。
          const icon = end.icon || child.getBoundingClientRect().height;
          if (labelWidth > 0 && width > icon) savings.push({ step: "endLabel", owner: "fixed", saving: width - icon });
          // 排在 fixed 最前：planTitleBar 把「结束会话」省下的宽度记在第一项上。
          fixed.unshift(width);
        } else if (width > 0 && getComputedStyle(child).position !== "absolute") fixed.push(width);
      }
      const next = planTitleBar({ available, gap: parseFloat(style.columnGap) || 0, titleMin, titleWant, fixed, items, savings }, remote);
      return (prev: typeof NO_PLAN) => (sameOverflow(prev.hidden, next.hidden) && sameOverflow(prev.compact, next.compact) ? prev : next);
    };
    setPlan(compute());
    if (typeof ResizeObserver !== "function") return;
    const update = () => { const apply = compute(); flushSync(() => setPlan(apply)); };
    const resize = new ResizeObserver(update);
    // 标题、项目名自己也要看：没省略时改名会改它的宽度，而标题区（flex:1）的宽度不变。
    const observeAll = () => {
      resize.disconnect();
      resize.observe(bar);
      for (const child of Array.from(bar.children)) resize.observe(child);
      for (const el of Array.from(bar.querySelectorAll(".chat-title, .chat-title-project"))) resize.observe(el);
    };
    observeAll();
    const mutation = typeof MutationObserver === "function" ? new MutationObserver(() => { observeAll(); update(); }) : null;
    // subtree + characterData：标题省略着的时候改名，谁的盒子都不变，只有文字节点变了。
    mutation?.observe(bar, { childList: true, subtree: true, characterData: true });
    return () => { resize.disconnect(); mutation?.disconnect(); };
  }, [ref, remote]);
  return plan;
}

/** 可收起项的外壳：收起时留在 DOM 里照常排版量宽度，但退出排版流、不可见、不可聚焦
 *  （inert：React 18 不认 inert prop，直写属性，同 AppModal）。 */
function OverflowSlot({ k, hidden, children }: { k: TitleOverflowKey; hidden: ReadonlySet<TitleOverflowKey>; children: ReactNode }) {
  const off = hidden.has(k);
  return (
    <span className="chat-bar-slot" data-overflow-key={k} aria-hidden={off || undefined} {...(off ? { inert: "" } : {})}>
      {children}
    </span>
  );
}

const PR_ICON = (
  <Icon glyph={icPullRequest} size={14} />
);
const GIT_ICON = (
  <Icon glyph={icGitBranch} size={14} />
);
const terminalIcon = (size: IconSize) => <Icon glyph={icTerminalBox} size={size} />;
const infoIcon = (size: IconSize) => <Icon glyph={icList} size={size} />;
import { tipWithKeys } from "./WindowModals";
import { Icon, type IconSize } from "../../icons/Icon";
import { icGitBranch, icList, icPullRequest, icSidebarRight, icTerminalBox } from "../../icons/glyphs.chat";
import { icStopCircle } from "../../icons/glyphs";

/// 当前分支的 PR（对照 Kimi 标题栏）：换目录 / 换分支时查一次，之后 3 分钟一次——gh 要联网，
/// 不跟改动摘要的轮询走（后端另按目录缓存 60s）。手机不查：桌面专用命令，桥上不放行。
function usePullRequest(cwd: string | null, branch: string | null): PullRequestDto | null {
  const [pullRequest, setPullRequest] = useState<PullRequestDto | null>(null);
  useEffect(() => {
    setPullRequest(null);
    if (!cwd || !branch || remoteUi()) return;
    let alive = true;
    const fetchPr = () => void getPullRequest(cwd).then((pr) => { if (alive) setPullRequest(pr ?? null); }).catch(() => {});
    fetchPr();
    // 窗口隐藏时不查（规格：PR/CI 轮询隐藏时 0 次），首次查询照常。
    const timer = window.setInterval(() => { if (document.visibilityState !== "hidden") fetchPr(); }, 180_000);
    return () => { alive = false; window.clearInterval(timer); };
  }, [cwd, branch]);
  return pullRequest;
}

export function ChatTitleBar({
  leading, title, archive, end, cwd, prCwd, gitSummary, changesOpen, panelOpen, terminalOn,
  onToggleChanges, onTogglePanel, onToggleTerminal, onError, panelAgentBusy = false, panelWaiting = false, info = null,
  onPullRequest,
}: {
  /** 标题前的侧栏开关（macOS 侧栏收起时 / 手机紧凑形态才有）。 */
  leading: ReactNode;
  /** 标题菜单（或首读回来前的占位）。 */
  title: ReactNode;
  /** 「标记完成」直达钮；已完成 / 临时会话不给。 */
  archive: { busy: boolean; onClick: () => void } | null;
  /** 「结束会话」；不可结束时不给。 */
  end: { busy: boolean; onClick: () => void } | null;
  cwd: string | null;
  /** 查 PR 的目录：工作目录不是仓库时是第一个仓库（与标题栏分支同一个），见 ChatWindow。 */
  prCwd?: string | null;
  gitSummary: GitDiffSummaryDto | null;
  /** 停靠面板开着且在改动页签 / 开着（任意页签）。 */
  changesOpen: boolean;
  panelOpen: boolean;
  terminalOn: boolean;
  onToggleChanges: () => void;
  onTogglePanel: () => void;
  onToggleTerminal: () => void;
  onError: (message: string) => void;
  /** 面板收起着，但 agent 正在操作这个会话的面板浏览器：开关上亮一个点（同 Kimi Code）。 */
  panelAgentBusy?: boolean;
  /** 面板收起时，里面的网页在等用户答复权限请求。 */
  panelWaiting?: boolean;
  /** 会话信息栏开关（面板收起时才有，同 Kimi Code 的「显示 / 隐藏会话信息」）。 */
  info?: { open: boolean; onToggle: () => void } | null;
  /** 查到的 PR 报给对话窗：Ctrl+K 的「打开 PR」用同一份，不再多打一次 gh（UI 优化 #53）。 */
  onPullRequest?: (pr: PullRequestDto | null) => void;
}) {
  const t = useT();
  const pullRequest = usePullRequest(prCwd ?? cwd, gitSummary?.isRepo ? gitSummary.branch : null);
  useEffect(() => { onPullRequest?.(pullRequest); }, [pullRequest, onPullRequest]);
  const barRef = useRef<HTMLElement>(null);
  const { hidden, compact } = useTitleBarOverflow(barRef, remoteUi());
  const openers = useFolderOpeners(cwd ?? "", onError);
  const showOpen = !remoteUi() && !!cwd;
  const showGit = !!cwd && !!gitSummary?.isRepo;
  // 放不下的项按栏上从左到右的顺序进 ⋯ 菜单（与栏上读同一份 hidden，见 titleOverflow）。
  const overflow: TitleOverflowItem[] = [];
  if (showOpen && hidden.has("open")) {
    for (const option of openers.options) {
      overflow.push({ key: `open:${option.id}`, group: "open", label: t.chat.openWith(option.name), icon: <OpenerIcon opener={option} />, onClick: () => openers.pick(option.id) });
    }
  }
  if (showGit && hidden.has("git")) overflow.push({ key: "git", group: "git", label: t.chat.diff, icon: GIT_ICON, onClick: onToggleChanges, checked: changesOpen });
  if (pullRequest && hidden.has("pr")) {
    const url = pullRequest.url;
    overflow.push({ key: "pr", group: "pr", label: t.chat.prOpenTip(pullRequest.number, ""), icon: PR_ICON, onClick: () => void openLink(url).catch(() => {}) });
  }
  if (!remoteUi() && hidden.has("terminal")) overflow.push({ key: "terminal", group: "terminal", label: t.chat.terminal, icon: terminalIcon(14), onClick: onToggleTerminal, checked: terminalOn });
  if (info && hidden.has("info")) overflow.push({ key: "info", group: "info", label: info.open ? t.chat.sessionInfoHide : t.chat.sessionInfoShow, icon: infoIcon(14), onClick: info.onToggle });
  return (
    <TitleOverflowContext.Provider value={overflow}>
    <header className="chat-bar" data-tauri-drag-region ref={barRef} data-compact={compact.size ? [...compact].join(" ") : undefined}>
      {leading}
      {title}
      {/* 标题栏刻意不放运行状态徽标（实拍反馈嫌吵）：运行态已有多处冗余信号——
          窗口标题的 ▶ 标记、对话区底部的脉冲指示条、侧栏状态点。
          也不放任务进度入口（用户要求取消）：待办走底部停靠栏的「当前进度」，子任务在对话流里。 */}
      {/* 「标记完成」直达钮（对照 Kimi 标题栏的 markSessionDone）：做完一件事就勾掉，
          是对话页最高频的收尾动作，不该藏在标题菜单的第二层。「恢复进行中」仍在标题菜单里（低频）。 */}
      {archive && (
        <OverflowSlot k="archive" hidden={hidden}>
        <button
          type="button"
          className="chat-todo-btn chat-archive-btn"
          data-tip={t.sticker.archive}
          aria-label={t.sticker.archive}
          disabled={archive.busy}
          onClick={archive.onClick}
        >
          {/* 16px 与同排的文件/改动入口图标对齐（菜单里的同款是 13px）。 */}
          <ArchiveIcon archived={false} size={16} />
        </button>
        </OverflowSlot>
      )}
      {/* endable 口径:托管 PTY 或进程仍活着的孤儿会话(后端按 pid 杀树兜底),见 ChatHistoryDto。
          远程不再隐藏:stop_managed_terminal 已进 /rpc 白名单(remote.rs 头注释记录了放行理由),
          确认框在手机上走 window.confirm(transport 的 confirm_dialog 替身)。 */}
      {/* 标题显示不全时只留图标（data-compact 的 endLabel 档），名字由 aria-label 与提示给出。 */}
      {end && (
        <button
          type="button"
          className="chat-end"
          disabled={end.busy}
          onClick={end.onClick}
          aria-label={end.busy ? t.chat.terminalStopping : t.chat.endSession}
          data-tip={end.busy ? t.chat.terminalStopping : t.chat.endSession}
        >
          <Icon glyph={icStopCircle} size={16} className="chat-end-ico" />
          <span className="chat-end-label">{end.busy ? t.chat.terminalStopping : t.chat.endSession}</span>
        </button>
      )}
      {/* 用编辑器打开会话目录（宿主执行类，远程隐藏）。 */}
      {showOpen && cwd && <OverflowSlot k="open" hidden={hidden}><OpenInEditor cwd={cwd} onError={onError} openers={openers} /></OverflowSlot>}
      {/* 分支 + 改动：点开 / 收起改动页签（远程放行改动列表，remote.rs 模块头「例外」）。 */}
      {showGit && gitSummary && <OverflowSlot k="git" hidden={hidden}><GitStatusChip summary={gitSummary} active={changesOpen} onToggle={onToggleChanges} /></OverflowSlot>}
      {pullRequest && <OverflowSlot k="pr" hidden={hidden}><PullRequestChip pr={pullRequest} /></OverflowSlot>}
      {/* 终端：对话与终端两个视图之间切换（原「对话 | 终端」页签收成一颗开关，对照 Kimi）。
          远程没有终端视图（见 ChatWindow 的 setView 说明），不给死按钮。 */}
      {!remoteUi() && (
        <OverflowSlot k="terminal" hidden={hidden}>
        <button
          type="button"
          className={"chat-todo-btn chat-terminal-toggle" + (terminalOn ? " is-active" : "")}
          data-tip={tipWithKeys(t.chat.terminal, terminalOn ? "Ctrl+1" : "Ctrl+2")}
          aria-label={t.chat.terminal}
          aria-pressed={terminalOn}
          onClick={onToggleTerminal}
        >
          {terminalIcon(16)}
        </button>
        </OverflowSlot>
      )}
      {/* 侧边面板（文件 / 改动停靠面板）开合，回到上次看的页签；不是仓库就只有文件页。
          远程不放行文件页，面板只能经改动入口打开。 */}
      {info && (
        <OverflowSlot k="info" hidden={hidden}>
        <button
          type="button"
          className={"chat-todo-btn chat-info-toggle" + (info.open ? " is-active" : "")}
          data-tip={info.open ? t.chat.sessionInfoHide : t.chat.sessionInfoShow}
          aria-label={info.open ? t.chat.sessionInfoHide : t.chat.sessionInfoShow}
          aria-pressed={info.open}
          onClick={info.onToggle}
        >
          {infoIcon(16)}
        </button>
        </OverflowSlot>
      )}
      {!remoteUi() && cwd && (
        <button
          type="button"
          className={"chat-todo-btn chat-panel-toggle" + (panelOpen ? " is-active" : "")}
          data-tip={tipWithKeys(panelWaiting ? `${t.chat.browserPermissionWaiting} · ${t.chat.togglePanel}` : panelAgentBusy ? `${t.chat.browserAgentBusyTip} · ${t.chat.togglePanel}` : t.chat.togglePanel, "Ctrl+Shift+B")}
          aria-label={panelWaiting ? `${t.chat.browserPermissionWaiting} · ${t.chat.togglePanel}` : panelAgentBusy ? `${t.chat.browserAgentBusyTip} · ${t.chat.togglePanel}` : t.chat.togglePanel}
          aria-pressed={panelOpen}
          onClick={onTogglePanel}
        >
          <Icon glyph={icSidebarRight} size={16} />
          {panelWaiting
            ? <span className="chat-panel-waiting-dot" aria-hidden="true" />
            : panelAgentBusy && <span className="chat-panel-agent-dot" aria-hidden="true" />}
        </button>
      )}
    </header>
    </TitleOverflowContext.Provider>
  );
}
