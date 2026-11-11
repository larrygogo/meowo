import { useEffect, useLayoutEffect, useRef, useState, type FocusEvent, type ReactNode, type RefObject } from "react";
import { remoteUi } from "../../remoteMode";
import { announceCard } from "./cardAnnouncer";
import { Icon } from "../../icons/Icon";
import { icCopy } from "../../icons/glyphs.chat";
import { icCheck } from "../../icons/glyphs";

/// 卡片正文里「用户做决定必须看的那一块」：命令原文、题干与选项、关卡选项。正文被限高时
/// 首屏要先露出它（见 useKeyContentInView），其余行（请求工具、标签、摘要）滚上去看。
const KEY_CONTENT = ".chat-approval-detail, .chat-question-panel, .chat-approval-options, .lead-gate-options";

/// 渐隐带的高度，与 styles.css 里 .chat-approval-copy[data-clip] 的遮罩同值。
const CLIP_FADE = 16;

/// 限高的卡片正文：把关键内容滚进首屏，并标出哪一侧还有内容被裁（data-clip，CSS 画渐隐）。
///
/// 起因（做空审查 2026-11-11）：620×480 下审批卡正文只剩 140px，命令排在最下面，首屏只露 7px；
/// 问答卡只剩半个选项，关卡卡一个选项都看不到——用户看得到「允许」却看不到要允许什么，
/// 也没有任何迹象表明下面还有内容。
/// 不改布局、只动 scrollTop 与属性，所以不会与卡片限高（--chat-overlay-room）来回自激。
function useKeyContentInView(copyRef: RefObject<HTMLDivElement | null>) {
  useEffect(() => {
    const copy = copyRef.current;
    if (!copy) return;
    // 用户自己滚过、或焦点进了正文（理由框、其他回答、附加意见）之后不再代滚：
    // 人在读上面的说明时内容高度一变就被拽回命令处，比看不到命令更糟。
    let hands = false;
    const takeOver = () => { hands = true; };
    const hints = () => {
      const top = copy.scrollTop > 1;
      const bottom = copy.scrollTop + copy.clientHeight < copy.scrollHeight - 1;
      const clip = top && bottom ? "both" : top ? "top" : bottom ? "bottom" : "";
      if (clip) copy.setAttribute("data-clip", clip); else copy.removeAttribute("data-clip");
      // 被裁的正文要能用键盘滚到：滚动区本身进 Tab 序（方向键/PageDown 滚动），没被裁时不占一站。
      if (clip) copy.tabIndex = 0; else copy.removeAttribute("tabindex");
    };
    const settle = () => {
      const key = copy.querySelector<HTMLElement>(KEY_CONTENT);
      if (key && !hands && !copy.contains(document.activeElement) && copy.scrollHeight > copy.clientHeight + 1) {
        const box = copy.getBoundingClientRect();
        const rect = key.getBoundingClientRect();
        const keyTop = rect.top - box.top + copy.scrollTop;
        // 关键块放得下就让它的下沿贴住可视区下沿（上方的说明能多露就多露）；放不下就从它的第一行起。
        // 关键块下面还有内容时底边会渐隐（CLIP_FADE），有余量就再多滚这一段，别让最后一个选项落在渐隐里。
        const slack = copy.clientHeight - rect.height;
        const below = copy.scrollHeight - (keyTop + rect.height) > 1;
        const target = slack >= 0 ? Math.max(0, keyTop + rect.height - copy.clientHeight + (below ? Math.min(CLIP_FADE, slack) : 0)) : keyTop;
        if (Math.abs(copy.scrollTop - target) > 1) copy.scrollTop = Math.min(target, keyTop);
      }
      hints();
    };
    settle();
    copy.addEventListener("scroll", hints, { passive: true });
    copy.addEventListener("wheel", takeOver, { passive: true });
    copy.addEventListener("touchmove", takeOver, { passive: true });
    copy.addEventListener("keydown", takeOver);
    copy.addEventListener("focusin", takeOver);
    const off = () => {
      copy.removeEventListener("scroll", hints);
      copy.removeEventListener("wheel", takeOver);
      copy.removeEventListener("touchmove", takeOver);
      copy.removeEventListener("keydown", takeOver);
      copy.removeEventListener("focusin", takeOver);
    };
    if (typeof ResizeObserver === "undefined") return off;
    // 可视高度随窗口/键盘变，内容高度随展开、换题、倒计时折行变：正文自身与每个子块都观察。
    const sizes = new ResizeObserver(settle);
    const rewatch = () => {
      sizes.disconnect();
      sizes.observe(copy);
      for (const child of Array.from(copy.children)) sizes.observe(child);
      settle();
    };
    const children = new MutationObserver(rewatch);
    children.observe(copy, { childList: true });
    rewatch();
    return () => { sizes.disconnect(); children.disconnect(); off(); };
  }, [copyRef]);
}

/// 审批/交互卡的统一外壳。对话页上四类卡——broker 审批(claude hook 劫走的请求)、
/// 终端命令审批(claude/kimi 屏幕识别)、交互选择器(question/plan)、其他屏幕提示
/// (信任页/启动步骤/菜单)——此前各写一段 section、三套视觉(白卡/琥珀内联/琥珀浮层),
/// 现在全部收敛到 .chat-approval 一族:标题圆点 + 待授权徽章、正文、左右两个动作组。
///
/// 只统一长相,不统一决定回路:broker 卡走 resolvePendingApproval RPC,屏幕卡把按键
/// 写进 PTY——那些差异留在调用方的按钮 onClick 里。动作排布的约定也在这里定死:
/// 左组(sideActions)放「不作本次决定」的动作(零副作用的仅收起、范围更大的持久放行),
/// 右组(actions)只放本次的决定(拒绝/允许/确认),与 Esc=拒绝的落点对得上。
export function ApprovalCard({ title, badge, className, children, sideActions, actions, returnFocusTo }: {
  title: string;
  badge?: string;
  className?: string;
  /// 卡片正文(工具名/描述/命令原文/选项列表…),渲染进 .chat-approval-copy。
  children?: ReactNode;
  sideActions?: ReactNode;
  actions?: ReactNode;
  /// 收卡后的焦点归还目标(composer 输入框)。C-9 overlay 后卡片不再是输入框的
  /// 文档流邻居,浏览器不会自己找回去——拒/答/收起导致卸载时显式归还。
  returnFocusTo?: RefObject<HTMLElement | null>;
}) {
  const ref = useRef<HTMLElement>(null);
  const copyRef = useRef<HTMLDivElement>(null);
  useKeyContentInView(copyRef);
  // 首焦点规则（UI 优化 c3/m11，取代 G-16 的「出现即把焦点交给第一个动作按钮」）：
  // 卡片挂载不再抢焦点。窗口刚打开时焦点在 body，旧做法把它落到 DOM 序第一个动作钮——实际是
  // 「说明并拒绝」，用户开窗就打字，空格点开理由框、后面的字进了理由框、回车即误拒（chat s13
  // 实测）。现在：焦点无处落脚（body）时交给 composer（returnFocusTo），打字永远进输入框；
  // 焦点已在别处一律不动。读屏/键盘用户用 Tab 进卡，进卡首个落点是「拒绝」（见 onFocus）。
  // 手机远程不做挂载移焦：WebKit 把程序化 focus 画成焦点环，看着像推荐选项；聚焦输入框还会
  // 弹键盘（mobile m11 实拍）。
  useEffect(() => {
    if (remoteUi()) return;
    const active = document.activeElement;
    if (active && active !== document.body) return;
    const composer = returnFocusTo?.current;
    if (composer?.isConnected) composer.focus({ preventScroll: true });
  // eslint-disable-next-line react-hooks/exhaustive-deps -- 只在挂载时判一次；returnFocusTo 是 ref 容器
  }, []);
  // 键盘 Tab 从卡外进来时，首个落点改道到「拒绝」（.is-deny）：DOM 序里排在它前面的是
  // 「展开全部/复制命令」、持久放行和「说明并拒绝」——落在「说明并拒绝」上随手一个空格就是
  // 打开理由框。只认键盘模态（input-modality 的 data-im=kbd，Tab 按下即挂上）：鼠标点卡里
  // 任何按钮都照常落在被点的那颗；进卡之后卡内移动（relatedTarget 在卡内）也不改道，
  // Shift+Tab 回「说明并拒绝」照常可达。没有「拒绝」的卡（提问、Lead 关卡）保持 DOM 序。
  const onFocus = (event: FocusEvent<HTMLElement>) => {
    const card = ref.current;
    if (!card) return;
    const from = event.relatedTarget;
    if (from instanceof Node && card.contains(from)) return;
    if (document.documentElement.getAttribute("data-im") !== "kbd") return;
    const entry = card.querySelector<HTMLButtonElement>(".chat-approval-actions > button.is-deny");
    if (!entry || entry.disabled || entry === event.target) return;
    entry.focus();
  };
  // 出现即播报标题（polite，不动焦点）：卡片不获焦时 alertdialog 读屏不会主动念，见 cardAnnouncer。
  // eslint-disable-next-line react-hooks/exhaustive-deps -- 只在出现那一刻播一次，标题后续变化（倒计时等）不重播
  useEffect(() => { announceCard(title); }, []);
  // 焦点归还用 layout cleanup:被动 effect 的 cleanup 在 DOM 移除后才跑,那时
  // contains(activeElement) 已失效(焦点先掉回 body);layout cleanup 在移除前执行,
  // 还能认出「焦点在卡内」。归还本身推迟到宏任务:同一帧挂载的新卡会先接管焦点
  // (那时 activeElement 已不是 body,不抢),且 compose 的 inert 解锁 effect
  // (屏幕识别卡在场时锁定 composer)也已跑完。
  useLayoutEffect(() => {
    const el = ref.current;
    return () => {
      if (!el || !el.contains(document.activeElement)) return;
      // eslint-disable-next-line react-hooks/exhaustive-deps -- returnFocusTo 是父组件给的 ref 容器，卸载时要读它最新指向的元素
      const target = returnFocusTo?.current;
      window.setTimeout(() => {
        // 切到终端视图时 composer 同帧卸载(detached),focus 无意义也不生效——跳过。
        if ((document.activeElement === document.body || document.activeElement == null) && target?.isConnected) target.focus();
      }, 0);
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- returnFocusTo 是父组件的 ref 容器，卸载时读它最新指向的元素；只在挂载卸载时跑，父组件重渲染不该重挂
  }, []);
  return (
    <section ref={ref} className={"chat-approval" + (className ? ` ${className}` : "")} role="alertdialog" aria-modal="false" aria-label={title} onFocus={onFocus}>
      <div ref={copyRef} className="chat-approval-copy">
        <div className="chat-approval-head">
          <strong>{title}</strong>
          {badge && <span className="chat-approval-badge">{badge}</span>}
        </div>
        {children}
      </div>
      {/* 左组为空时由 CSS 的 :empty 收掉,右组按钮天然靠右。 */}
      {(sideActions || actions) && <div className="chat-approval-actions">
        <div className="chat-approval-actions-side">{sideActions}</div>
        {actions}
      </div>}
    </section>
  );
}

/// 破坏性命令的浅启发式:只用来给「允许一次」换危险色(rm -rf 与 ls 不该同一个
/// 视觉权重),不改变任何行为。宁漏勿冤——漏判只是回到默认主按钮,误判会把一条
/// 安全命令染红。同时覆盖 bash 与 Windows cmd 的常见破坏形态。
const RISKY_COMMAND = /(?:\brm\s+-[a-z]*[rf][a-z]*\b|\bsudo\b|\bmkfs(?:\.[a-z0-9]+)?\b|\bdd\b[^|&;]*\bof=|\bshutdown\b|\breboot\b|\bgit\s+reset\s+--hard\b|\bgit\s+clean\s+-[a-z]*[fd]|\bgit\s+push\b[^|&;]*--force|\bchmod\s+-R\b|\bchown\s+-R\b|>\s*\/dev\/sd|\bformat\s+[a-z]:|\bdel\s+\/[fsq]\b|\brd\s+\/s\b|\brmdir\s+\/s\b)/i;

export function isRiskyCommand(text: string): boolean {
  return RISKY_COMMAND.test(text);
}

/// 长命令详情:pre 限高 160px、外层 copy 又限高滚动,两层嵌套滚动里长命令读不全
/// 也拿不走。给两个出口:「展开全部」摘掉 pre 的限高(回到外层单层滚动)、
/// 「复制命令」整段进剪贴板。label/copy 文案由调用方给(i18n 在 ChatWindow 侧)。
export function ApprovalCommandDetail({ label, command, expandLabel, collapseLabel, copyLabel, copiedLabel }: {
  label: string;
  command: string;
  expandLabel: string;
  collapseLabel: string;
  copyLabel: string;
  copiedLabel: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);
  // 只有命令真的超出 pre 的限高才给「展开全部」：一行的 git push 也挂着它、点了毫无变化，
  // 还白占约 45px，而矮窗口里审批卡最缺的就是高度（UI 调研 chat #9）。判法照搬
  // CrossMessageBlock：实测 scrollHeight，+4 容差防亚像素虚报，窗口宽度变化时复测。
  const preRef = useRef<HTMLPreElement>(null);
  const [overflowing, setOverflowing] = useState(false);
  useEffect(() => {
    const el = preRef.current;
    if (!el) return;
    const measure = () => setOverflowing(el.scrollHeight > el.clientHeight + 4);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [command]);
  const copy = () => {
    if (!navigator.clipboard?.writeText) return;
    void navigator.clipboard.writeText(command).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    }).catch(() => {});
  };
  return (
    <div className={"chat-approval-detail" + (expanded ? " is-expanded" : "")}>
      <span>{label}</span>
      <pre ref={preRef}>{command}</pre>
      <div className="chat-approval-detail-tools">
        {/* 展开后 pre 不再限高、量不出溢出，仍要留着「收起」。 */}
        {(overflowing || expanded) && (
          <button type="button" onClick={() => setExpanded((value) => !value)}>{expanded ? collapseLabel : expandLabel}</button>
        )}
        {/* 图标钮：文字版独占一行（约 30px），矮窗里正是它把命令挤到折叠线下；名字留给读屏与悬停提示。 */}
        <button type="button" className="chat-approval-copy-btn" aria-label={copied ? copiedLabel : copyLabel} data-tip={copied ? copiedLabel : copyLabel} onClick={copy}>
          <Icon glyph={copied ? icCheck : icCopy} size={14} />
        </button>
      </div>
    </div>
  );
}
