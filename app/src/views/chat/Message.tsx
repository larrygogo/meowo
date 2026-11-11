import { memo, useContext, useEffect, useRef, useState, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { pushEscLayer } from "../../escLayers";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useT } from "../../i18n";
import { attachmentInstructionEn, attachmentInstructionZh } from "../../i18n/attachmentInstruction";
import { type ChatItem } from "../../api";
import { ChatMarkdown, PlainMarkdown } from "../ChatMarkdown";
import { ErrorBoundary } from "../../ErrorBoundary";
import { parseUserText, type CrossMessage } from "./localCommand";
import { copyPlainText } from "./clipboard";
import { fmtSpan, stripAnsi } from "./shared";
import { ChatActionsContext } from "./chatActions";
import { EchoActionsContext, EchoPhaseContext, outgoingIdOf, type EchoActions } from "./outgoingPrompts";
import { useRememberedDetails, useRememberedOpen } from "./foldMemory";
import { FILE_TOKEN_REFRESHED_EVENT, IMAGE_LOAD_FAILED_EVENT, remoteUi } from "../../remoteMode";
import { openSettingsAt } from "../../settingsNav";

export function MessageCopyButton({ text, className = "chat-message-copy" }: { text: string; className?: string }) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  const copy = () => {
    void copyPlainText(text).then((ok) => {
      if (!ok) return;
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    });
  };
  const label = copied ? t.chat.messageCopied : t.chat.copyMessage;
  return (
    <button type="button" className={className} aria-label={label} data-tip={label} onClick={copy}>
      {copied ? (
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m5 12 4 4L19 6" /></svg>
      ) : (
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><rect x="8" y="8" width="11" height="12" rx="2" /><path d="M15 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h2" /></svg>
      )}
    </button>
  );
}

// Claude Code 把粘贴/引用的图片在 transcript 里记成一行「[Image: source: <本地路径>]」。
// 原样渲染就是在气泡里摊一整行 C:\Users\... 路径——对话里最脏的元素。渲染成缩略图
// （asset 协议已启用，scope 限定在各账号的 image-cache 与 meowo-paste 图片目录，见
// tauri.conf.json）；scope 外/文件已删时 onError 回退成文件名徽章。
// 路径**任何形式都不上屏**——包括 title：hover 提示与读屏都会念出它，而本地路径常含
// 用户名等身份信息。要看是哪张图，缩略图本身与文件名就够了。
const IMAGE_REF = /\[Image(?: #\d+)?: source: ([^\]]+?)\]/g;

// kimi 文本回退时不吃指令里的路径行,附件行「- <图片绝对路径>」原文落进 transcript
// (claude 会转成上面的 [Image: source: …] 引用行)。有指令头时把这类行也抽成 chip。
// 只认 Windows 绝对路径(盘符 + 反斜杠,与实测形态一致);路径可含空格,故不按空白切、
// 整行捕获到扩展名收尾。扩展名口径与 ChatWindow 的 IMAGE_EXTENSIONS 保持一致。
const IMAGE_PATH_LINE = /^- ([A-Za-z]:\\.+\.(?:png|jpe?g|gif|webp|bmp|svg))\s*$/i;

/** 附件指令头（composer 注入给 CLI 的那句，见 i18n 的 attachmentInstruction）。
 *  图片已渲染成缩略图时，这句对人是纯噪音——按下方规则剥掉。直接从两份字典共用的那一处真源
 *  派生（attachmentInstruction("") 去掉附件列表后 trim），不手抄原句：文案改动自动跟上，
 *  历史消息无论当时用哪种语言写下都认（评审：手抄字面量会在改文案时静默失效）。
 *  不引整份 zh/en 字典：那样会把按需加载的另一种语言又静态拖回来（i18n/index.tsx）。 */
const ATTACHMENT_INSTRUCTION_HEADERS = new Set(
  [attachmentInstructionZh, attachmentInstructionEn].map((make) => make("").trim()),
);

/** 把用户文本拆成「正文（去掉图片引用行）+ 图片路径列表」。图片不混排在文字里：
 *  气泡是「说的话」，图片是附件，各归各的（正文里的 [Image #N] 指代照旧保留）。
 *  图片来源有两种形态：claude 式 [Image: source: …] 引用行，以及 kimi 文本回退时
 *  的「- <图片绝对路径>」附件行（仅在有附件指令头时抽取，见下方 IMAGE_PATH_LINE）。
 *  导出给 ChatWindow 的 ↑ 召回复用：召回只回填正文，口径必须与气泡渲染一致。 */
export function splitUserText(text: string): { body: string; images: { path: string; key: string }[] } {
  const matches = [...text.matchAll(IMAGE_REF)];
  const images = matches.map((match, index) => ({ path: match[1].trim(), key: `${match.index}:${index}` }));
  // 无图片引用也不能早退：CLI 会把附件行「- <路径>」里的图片路径吃掉、转成独立
  // image 块，文本只剩指令头 + 光杆「-」——这形状同样要剥（实拍回归）。
  // 逐行抽引用，只删「原行确实含图片引用、抽走后只剩空白或空弹头」的行——
  // 曾按全文过滤 trim()==="-" 的行，把用户自己写的独立「-」行也误删了（评审确认）。
  const kept: string[] = [];
  for (const line of text.split("\n")) {
    const stripped = line.replace(IMAGE_REF, "");
    const leftover = stripped.trim();
    if (stripped !== line && (leftover === "" || leftover === "-")) continue;
    kept.push(stripped);
  }
  // 指令头连同其后紧跟的光杆「-」行（附件残行）一起处理：残行总是剥；仍有实质
  // 附件行（非图片路径「- xxx」）时保留头，否则头也剥。用户自己写的孤立「-」
  // 不紧跟在指令头后，不受影响。
  // 消息是否经附件机制发出(有指令头):独行光杆只在此前提下、且仅剥「文本最顶端那段
  // 机器前缀」——CC 把光杆「[Image #N]」挪到提交文本最前,始终排在用户正文之前。一旦
  // 遇到第一行真实内容就停止,用户正文里手打的孤立「[Image #N]」照旧保留(此前全消息
  // 范围剥,把用户写的指代也吃了——desktop 回归)。
  const hasInstructionHeader = kept.some((line) =>
    ATTACHMENT_INSTRUCTION_HEADERS.has(line.replace(/\[Image #\d+\]\s*/g, "").trim()),
  );
  // kimi 式「指令头 + 图片路径行」:文本回退时 kimi 不吃路径行,「- C:\…\image.png」
  // 原样落进文本。有指令头时把这些行抽成 chip 并剥掉——剥在判头保留之前,这样
  // 「头 + 纯图片路径行」会走「无实质附件行 → 头也剥」的既有分支;混排的非图片
  // 「- xxx」附件行留下,头按既有语义保留。无指令头时不动:用户手打的「- 路径」
  // 列表是正文内容。
  if (hasInstructionHeader) {
    const withoutImagePaths: string[] = [];
    for (let i = 0; i < kept.length; i++) {
      const match = IMAGE_PATH_LINE.exec(kept[i].trim());
      if (match) {
        images.push({ path: match[1], key: `path:${i}` });
        continue;
      }
      withoutImagePaths.push(kept[i]);
    }
    kept.length = 0;
    kept.push(...withoutImagePaths);
  }
  let inLeadingRefs = hasInstructionHeader;
  const body: string[] = [];
  for (let i = 0; i < kept.length; i++) {
    const line = kept[i];
    // CC 把粘贴图片的光杆指代「[Image #N]」挪到提交文本最前:可能与指令头粘成一行
    // (「[Image #1]请读取并结合…」),也可能独占一行——判头前先剥掉它,否则精确
    // 比对失配,整段样板漏进气泡(远程发图实拍)。带 source 的完整引用行已在上一轮抽走。
    const bareStripped = line.replace(/\[Image #\d+\]\s*/g, "");
    if (!ATTACHMENT_INSTRUCTION_HEADERS.has(bareStripped.trim())) {
      // 顶端机器前缀里的独行光杆指代(图片已由 image 块渲染),剥。
      if (inLeadingRefs && bareStripped.trim() === "" && line.trim() !== "") continue;
      // 第一行真实内容(空行不计):机器前缀结束,此后不再剥独行指代。
      if (line.trim() !== "") inLeadingRefs = false;
      body.push(line);
      continue;
    }
    let j = i + 1;
    while (j < kept.length && kept[j].trim() === "-") j++;
    // 头要保留(还有非图片附件行)时也用剥过指代的版本:图片已是缩略图,光杆指代无信息。
    if (kept[j]?.trim().startsWith("- ")) body.push(bareStripped);
    i = j - 1;
  }
  return { body: body.join("\n").trim(), images };
}

/** 大图查看层：滚轮缩放、拖拽平移、双击复位；工具栏有缩放按钮与明显的关闭键。
 *  点击空白处或 Esc 也能关（拖拽松手不算点击）。 */
function Lightbox({ src, name, onClose }: { src: string; name: string; onClose: () => void }) {
  const t = useT();
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const stageRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ startX: number; startY: number; baseX: number; baseY: number; moved: boolean } | null>(null);
  const justDraggedRef = useRef(false);
  const clampScale = (value: number) => Math.min(8, Math.max(0.2, value));
  const reset = () => { setScale(1); setOffset({ x: 0, y: 0 }); };

  // Esc 关灯箱。挂捕获段并拦住传播：ChatWindow 有一个全局 Esc=拒绝审批的监听，
  // 用户按 Esc 只想关图，不能顺手把审批拒了。同时注册 Esc 层（escLayers.ts）——
  // 层栈是全局监听的统一让位判据，捕获段截停是本层自己的双保险。
  useEffect(() => {
    const popLayer = pushEscLayer();
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      popLayer();
      window.removeEventListener("keydown", onKey, true);
    };
  }, [onClose]);

  // 滚轮缩放。React 的 onWheel 在根容器上是 passive 监听，preventDefault 无效
  // （页面会跟着滚）——自己挂非 passive 的原生监听。
  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      setScale((current) => clampScale(current * (event.deltaY < 0 ? 1.15 : 1 / 1.15)));
    };
    stage.addEventListener("wheel", onWheel, { passive: false });
    return () => stage.removeEventListener("wheel", onWheel);
  }, []);

  const onMouseDown = (event: ReactMouseEvent) => {
    event.preventDefault();
    dragRef.current = { startX: event.clientX, startY: event.clientY, baseX: offset.x, baseY: offset.y, moved: false };
  };
  const onMouseMove = (event: ReactMouseEvent) => {
    const drag = dragRef.current;
    if (!drag) return;
    const dx = event.clientX - drag.startX;
    const dy = event.clientY - drag.startY;
    if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
    setOffset({ x: drag.baseX + dx, y: drag.baseY + dy });
  };
  const onMouseUp = () => {
    if (!dragRef.current) return;
    justDraggedRef.current = dragRef.current.moved;
    dragRef.current = null;
  };
  const onStageClick = (event: ReactMouseEvent) => {
    // 拖拽平移松手不是「点空白关闭」；只有真正点在空白处才关。
    if (justDraggedRef.current) { justDraggedRef.current = false; return; }
    if (event.target === event.currentTarget) onClose();
  };

  return createPortal(
    <div className="chat-lightbox" role="dialog" aria-label={name}>
      <div
        ref={stageRef}
        className="chat-lightbox-stage"
        onClick={onStageClick}
        onMouseDown={onMouseDown}
        onMouseMove={onMouseMove}
        onMouseUp={onMouseUp}
        onMouseLeave={onMouseUp}
        onDoubleClick={reset}
      >
        {/* no-referrer:远程模式下 src 是带 token 的 /file URL,不许经 Referer 外泄。 */}
        <img
          src={src}
          alt={name}
          draggable={false}
          referrerPolicy="no-referrer"
          style={{ transform: `translate(${offset.x}px, ${offset.y}px) scale(${scale})` }}
        />
      </div>
      <div className="chat-lightbox-bar">
        <button type="button" aria-label={t.chat.zoomOut} data-tip={t.chat.zoomOut} onClick={() => setScale((s) => clampScale(s / 1.25))}>−</button>
        <button type="button" className="chat-lightbox-zoom" aria-label={t.chat.zoomReset} data-tip={t.chat.zoomReset} onClick={reset}>{Math.round(scale * 100)}%</button>
        <button type="button" aria-label={t.chat.zoomIn} data-tip={t.chat.zoomIn} onClick={() => setScale((s) => clampScale(s * 1.25))}>+</button>
        <button type="button" aria-label={t.chat.lightboxClose} data-tip={t.chat.lightboxClose} onClick={onClose}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 6l12 12M18 6L6 18" /></svg>
        </button>
      </div>
    </div>,
    document.body,
  );
}

/** 跨会话消息块：另一个 Claude 会话经 SendMessage 发来的一条消息。
 *  它是真的对话内容（不是 <task-notification> 那种机器记录），所以正文直接可读；
 *  但复核回执这类消息动辄几十行，收起态限高 220px，超出才给「展开全部」。
 *  超没超按实测判（scrollHeight vs clientHeight），不按字数猜——同样行数的中英文、
 *  带不带长 URL，渲染出来的高度差很多。 */
function CrossMessageBlock({ message }: { message: CrossMessage }) {
  const t = useT();
  const [expanded, setExpanded] = useState(false);
  const bodyRef = useRef<HTMLDivElement>(null);
  const [overflowing, setOverflowing] = useState(false);
  useEffect(() => {
    const el = bodyRef.current;
    if (!el) return;
    // +4 容差：亚像素行高会让恰好齐平的块虚报溢出，白挂一个点了没变化的按钮。
    const measure = () => setOverflowing(el.scrollHeight > el.clientHeight + 4);
    measure();
    // 只在文本变化时量一次是不够的（复核指出）：窗口变窄会让原本齐平的消息超高、
    // 变宽则相反，不复测就出现「短消息挂着展开钮」或「该有钮的没有」。
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [message.text]);
  return (
    <div className="chat-crossmsg">
      <div className="chat-crossmsg-head">
        <span className="chat-crossmsg-icon" aria-hidden="true">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"><path d="M7 8h10l-3-3M17 16H7l3 3" /></svg>
        </span>
        <span className="chat-crossmsg-from">{message.fromName || t.chat.crossMessageUnknown}</span>
        {/* 权限模式只在**非默认**时显示：default 是常态，写出来只是噪声。 */}
        {message.mode && message.mode !== "default" && (
          <span className="chat-crossmsg-mode">{message.mode}</span>
        )}
        <span className="chat-crossmsg-label">{t.chat.crossMessage}</span>
      </div>
      <div ref={bodyRef} className={"chat-crossmsg-body" + (expanded ? " is-expanded" : "")}>
        <div className="chat-text">{message.text}</div>
      </div>
      {/* 收起态只在真的超高时给按钮；展开后保留「收起」，长消息才收得回去。 */}
      {(overflowing || expanded) && (
        <button type="button" className="chat-crossmsg-more" onClick={() => setExpanded((v) => !v)}>
          {expanded ? t.chat.approvalCollapse : t.chat.approvalExpand}
        </button>
      )}
    </div>
  );
}

/** 图片缩略图 + 点开灯箱。导出给 composer 的附件条复用：粘贴的图片与 transcript
 *  里的图片走同一套预览（按路径走 convertFileSrc，asset 读不到时回退文件名 chip）。 */
export function ImageRef({ path }: { path: string }) {
  const [failed, setFailed] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const name = path.split(/[\\/]/).pop() || path;
  // 远程端失败时可能只是 /file 凭据过期（桌面重启换代），挂着等 transport 重领后重试。
  // 桌面端没人派发该事件，失败即终态，行为不变。
  useEffect(() => {
    if (!failed) return;
    const retry = () => setFailed(false);
    window.addEventListener(FILE_TOKEN_REFRESHED_EVENT, retry);
    return () => window.removeEventListener(FILE_TOKEN_REFRESHED_EVENT, retry);
  }, [failed]);
  if (failed) {
    return (
      <span className="chat-image-chip" data-tip={name}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><rect x="3" y="5" width="18" height="14" rx="2" /><circle cx="9" cy="10" r="1.6" /><path d="m5 17 4.5-4.5L13 16l3-3 3 4" /></svg>
        <span>{name}</span>
      </span>
    );
  }
  const src = convertFileSrc(path);
  return (
    <>
      <button type="button" className="chat-image-thumb-btn" data-tip={name} onClick={() => setExpanded(true)}>
        <img className="chat-image-thumb" src={src} alt={name} loading="lazy" referrerPolicy="no-referrer" onError={() => {
          setFailed(true);
          window.dispatchEvent(new CustomEvent(IMAGE_LOAD_FAILED_EVENT));
        }} />
      </button>
      {/* 灯箱走 portal（在 Lightbox 内）：消息块开着 content-visibility（paint 包含），
          fixed 覆盖层留在消息里会被裁剪在消息框内。 */}
      {expanded && <Lightbox src={src} name={name} onClose={() => setExpanded(false)} />}
    </>
  );
}

function ImageRow({ images }: { images: { path: string; key: string }[] }) {
  return (
    <div className="chat-image-row">
      {images.map((image) => <ImageRef path={image.path} key={image.key} />)}
    </div>
  );
}

/** 该条用户消息是否**只有**图片（无正文）。是则返回图片路径，供 Transcript 把连续的
 *  纯图片消息合并成一行——Claude Code 把一次多图粘贴记成连续多条独立消息，逐条渲染
 *  就是竖着摞一列，而用户视角那是「一次发的几张图」。 */
export function imageOnlyPaths(text: string): string[] | null {
  const { body, images } = splitUserText(text);
  return !body && images.length > 0 ? images.map((image) => image.path) : null;
}

/** 连续纯图片消息合并后的展示块（Transcript 专用）。 */
export function UserImageGroup({ paths, anchorId }: { paths: string[]; anchorId?: string }) {
  return (
    <article className="chat-message is-user has-images" data-msg-id={anchorId}>
      <ImageRow images={paths.map((path, index) => ({ path, key: `${index}:${path}` }))} />
    </article>
  );
}

/** 思考过程块：恒为收起的一行灰字，点开看全文。2026-10 用户对照 Kimi 客户端实拍
 *  嫌对话区拥挤，定为「思考过程收成一行」——此前短思考摊开、长思考露 6 行预览，
 *  每轮都多出一段灰字。开合只随用户点击变（按会话记住），流式更新不会把它弹开。 */
function ReasoningBlock({ item, nextStamp = null }: { item: Extract<ChatItem, { type: "reasoning" | "reasoning_delta" }>; nextStamp?: string | null }) {
  const t = useT();
  // reasoning_delta 是还在流的那条，reasoning 是收尾后的定稿——用它区分「在跑 / 跑完」。
  const live = item.type === "reasoning_delta";
  const reasoningDetails = useRememberedDetails(`reasoning:${item.id}`);
  const startedAt = Date.parse(item.timestamp ?? "");
  const [elapsed, setElapsed] = useState(() => (live && Number.isFinite(startedAt) ? Date.now() - startedAt : null));

  useEffect(() => {
    if (!live || !Number.isFinite(startedAt)) return;
    setElapsed(Date.now() - startedAt);
    const timer = setInterval(() => setElapsed(Date.now() - startedAt), 1000);
    return () => clearInterval(timer);
  }, [live, startedAt]);

  // 跑完的：用下一条非 meta 条目的时间戳收尾（思考自己没有结束时间）。
  // 还在跑的：每秒 tick，转定稿后数字停在最后一次 tick 上不再动。
  let span: string | null = null;
  if (elapsed !== null && elapsed >= 1000) {
    span = fmtSpan(elapsed);
  } else if (!live) {
    const endedAt = Date.parse(nextStamp ?? "");
    if (Number.isFinite(startedAt) && Number.isFinite(endedAt) && endedAt - startedAt >= 1000) {
      span = fmtSpan(endedAt - startedAt);
    }
  }

  return (
    <details className="chat-reasoning" {...reasoningDetails}>
      <summary>
        <span className="chat-reasoning-icon" aria-hidden="true">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><path d="M9 18h6M10 22h4M12 2a7 7 0 0 0-4 12.7V17h8v-2.3A7 7 0 0 0 12 2z" /></svg>
        </span>
        {live ? t.chat.reasoningLive : t.chat.reasoning}
        {span && <span className="chat-reasoning-time">{span}</span>}
        <span className="chat-tool-chevron">›</span>
      </summary>
      <div className="chat-md"><ChatMarkdown text={item.text} /></div>
    </details>
  );
}

/// memo：流式期间 items 引用每轮都变（Transcript 整棵重建），但未变化的条目经
/// reducer 的写时复制保持同一引用——memo 让重渲染只落在真正变化的那条上，
/// parseUserText / split 这类逐条解析不再全量重跑。
/// 回合出错卡。最新的那张带「继续」：失败只给一个最短的恢复动作（对照 Kimi 的
/// turnFailed 卡）——发一句「继续」让 Agent 接着做；旧的出错卡后面已有新内容，不出按钮。
///
/// 下一步（对照 Kimi 错误文案的「原因 + 下一步」）：登录失效时光点「继续」只会再错一次，
/// 要先说清该去登录；连不上 API 时提示查网络和代理。判据是后端写的中文标签（各 agent
/// 插件共用，见 claude classify_error / codex / opencode），其余标签照旧只给「继续」。
const LOGIN_ERROR_LABELS = new Set(["需要重新登录", "认证失败"]);
const NETWORK_ERROR_LABELS = new Set(["无法连接 API"]);

function TurnErrorCard({ item, latest }: { item: Extract<ChatItem, { type: "turn_error" }>; latest: boolean }) {
  const t = useT();
  const actions = useContext(ChatActionsContext);
  const [busy, setBusy] = useState(false);
  const login = LOGIN_ERROR_LABELS.has(item.label);
  const hint = login ? t.chat.turnErrorLoginHint : NETWORK_ERROR_LABELS.has(item.label) ? t.chat.turnErrorNetworkHint : null;
  return (
    <article className="chat-message is-turn-error" role="alert">
      <div className="chat-turn-error-label">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M12 8v5M12 16.5v.5" /></svg>
        {/* 标签是后端写的中文 sentinel，英文界面要经 errorLabels 翻译（看板卡片同一张表）。 */}
        {t.errorLabels[item.label] ?? item.label}
      </div>
      <div className="chat-text">{item.text}</div>
      {latest && hint && <div className="chat-turn-error-hint">{hint}</div>}
      {latest && actions && (
        <div className="chat-turn-error-actions">
          {/* 登录入口在设置的「账号与用量」。open_settings 是宿主命令，手机上点了没反应，不给。 */}
          {login && !remoteUi() && (
            <button type="button" className="chat-turn-error-continue" onClick={() => openSettingsAt("account")}>
              {t.chat.turnErrorLoginGo}
            </button>
          )}
          <button
            type="button"
            className="chat-turn-error-continue"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              void actions.sendText(t.chat.turnErrorContinueText).finally(() => setBusy(false));
            }}
          >
            {t.chat.turnErrorContinue}
          </button>
        </div>
      )}
    </article>
  );
}

/// 用户消息页脚的「撤回」（对照 Kimi 的 undoTooltip「撤回消息」）：对话窗声明这条此刻可撤
///（当前段、非插话、会话空闲、CLI 支持）才出现，其余时候整颗不渲染，不留灰按钮。
function UndoButton({ id, text }: { id: string; text: string }) {
  const t = useT();
  const actions = useContext(ChatActionsContext);
  if (!actions?.undoFrom || !actions.undoableIds?.has(id)) return null;
  const undo = actions.undoFrom;
  return (
    <button type="button" className="chat-turn-copy" aria-label={t.chat.undoTip} data-tip={t.chat.undoTip} onClick={() => undo(id, text)}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M9 14 4 9l5-5" /><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11" /></svg>
    </button>
  );
}

/// 用户气泡正文：超过 10 行收起并渐隐，底部给「展开全部」（对照 Kimi 的 u-text-wrap.is-clamped）。
/// 贴进来的一大段日志、报错原样摊开能占好几屏，回答被挤到看不见的地方；用户自己写的
/// 话通常一眼就认得，看开头足够。超没超按实测高度判（同 CrossMessageBlock），展开态按会话记住。
function UserBubbleText({ id, body }: { id: string; body: string }) {
  const t = useT();
  const [expanded, setExpanded] = useRememberedOpen(`user:${id}`, false);
  const textRef = useRef<HTMLDivElement>(null);
  const [overflowing, setOverflowing] = useState(false);
  useEffect(() => {
    const el = textRef.current;
    if (!el) return;
    // +4 容差：亚像素行高会让恰好 10 行的消息虚报溢出。
    const measure = () => setOverflowing(el.scrollHeight > el.clientHeight + 4);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [body]);
  const clamped = !expanded;
  return (
    <div className={"chat-user-text" + (clamped && overflowing ? " is-clamped" : "")}>
      {/* 限高与渐隐落在内层：气泡底色在 .chat-text 上，深色主题是半透明的 rgba，
          「渐变到气泡色」的遮罩盖不住字（UI 调研 chat #13 实拍无渐隐），只能对文字层 mask。 */}
      <div className="chat-text"><div ref={textRef} className={"chat-user-clip" + (clamped ? " is-clampable" : "")}>{body}</div></div>
      {(overflowing || expanded) && (
        <button type="button" className="chat-user-toggle" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>
          {expanded ? t.chat.approvalCollapse : t.chat.approvalExpand}
        </button>
      )}
    </div>
  );
}

/// 压缩分隔条（对照 Kimi 的「已自动压缩上下文（tokens）· 查看摘要」）：压缩后用户最想知道
/// 它还记得什么，摘要就是答案。摘要默认收起、限高滚动（动辄上万字），开合按会话记住。
/// provider 没给细节时退回一句「上下文已压缩」。
function CompactDivider({ item, summary }: { item: Extract<ChatItem, { type: "meta" }>; summary: string | null }) {
  const t = useT();
  const [open, setOpen] = useRememberedOpen(`compact:${item.id}`, false);
  const auto = item.compaction?.trigger === "auto";
  const pre = item.compaction?.pre_tokens;
  return (
    <>
      <div className="chat-meta chat-compact">
        <span />
        {auto ? t.chat.compactAuto : t.chat.compact}
        {pre ? <small className="chat-meta-sub">{t.chat.compactPreTokens(Math.round(pre / 1000))}</small> : null}
        {summary && (
          <button type="button" className="chat-meta-link" aria-expanded={open} onClick={() => setOpen(!open)}>
            {open ? t.chat.compactHideSummary : t.chat.compactViewSummary}
          </button>
        )}
        <span />
      </div>
      {summary && open && <div className="chat-compact-summary chat-md"><ChatMarkdown text={summary} /></div>}
    </>
  );
}

/// 发送失败的回显页脚：标注 + 「重发」「放回输入框」。
function EchoFailedActions({ itemId, actions }: { itemId: string; actions: EchoActions | null }) {
  const t = useT();
  const id = outgoingIdOf(itemId);
  return (
    <>
      <span className="chat-user-note is-warn" role="alert">{t.chat.echoFailed}</span>
      {actions && id != null && <>
        <button type="button" className="chat-user-action" onClick={() => actions.retry(id)}>{t.chat.echoRetry}</button>
        <button type="button" className="chat-user-action" onClick={() => actions.restore(id)}>{t.chat.echoRestore}</button>
      </>}
    </>
  );
}

/// 未确认送达的出口（手机上也不能只剩「去终端页看看」）。对话窗读过终端画面：这段话还停在
/// CLI 输入框里，就给「提交」替你按一次回车；输入框里没有或认不出，给「重发 / 放回输入框」。
function EchoUncertainActions({ itemId, actions }: { itemId: string; actions: EchoActions | null }) {
  const t = useT();
  const id = outgoingIdOf(itemId);
  if (actions && id != null && actions.stuck.has(id)) {
    return (
      <>
        <span className="chat-user-note is-warn" data-tip={t.chat.echoStuckTip}>{t.chat.echoStuck}</span>
        <button type="button" className="chat-user-action" onClick={() => actions.submit(id)}>{t.chat.echoSubmit}</button>
      </>
    );
  }
  return (
    <>
      <span className="chat-user-note is-warn" data-tip={t.chat.echoUncertainTip}>{t.chat.echoUncertain}</span>
      {actions && id != null && <>
        <button type="button" className="chat-user-action" onClick={() => actions.retry(id)}>{t.chat.echoRetry}</button>
        <button type="button" className="chat-user-action" onClick={() => actions.restore(id)}>{t.chat.echoRestore}</button>
      </>}
    </>
  );
}

/// 默认收起、开合按会话记住的 <details>（键见 foldMemory）：Message 里的分支不能直接调 hook。
function RememberedDetails({ memoryKey, className, children }: { memoryKey: string; className: string; children: ReactNode }) {
  return <details className={className} {...useRememberedDetails(memoryKey)}>{children}</details>;
}

type MessageProps = { item: ChatItem; nextStamp?: string | null; latest?: boolean; compactSummary?: string | null };

/** 消息级兜底：这一条渲染抛错时只把它换成原文，别的消息照常（ErrorBoundary.tsx）。
 *  正文变了（流式追加）就重试一次；按内容而不是对象比，轮询每次换新对象时不必反复重试、反复报错。 */
export const Message = memo(function Message(props: MessageProps) {
  const { item } = props;
  const resetKey = `${item.id}\u0000${"text" in item && typeof item.text === "string" ? item.text : ""}`;
  return (
    <ErrorBoundary scope="message" resetKey={resetKey} fallback={() => <MessageFallback item={props.item} />}>
      <MessageBody {...props} />
    </ErrorBoundary>
  );
});

/** 原文：有正文的取正文，工具调用之类没有正文的把记录本身摊开（排障时也看得到是哪条）。 */
function MessageFallback({ item }: { item: ChatItem }) {
  const raw = "text" in item && typeof item.text === "string" ? item.text : JSON.stringify(item, null, 2);
  return (
    <article className="chat-message" data-msg-id={item.id}>
      <div className="chat-text chat-md"><PlainMarkdown text={raw} /></div>
    </article>
  );
}

function MessageBody({ item, nextStamp = null, latest = false, compactSummary = null }: MessageProps) {
  const t = useT();
  const echoPhase = useContext(EchoPhaseContext).get(item.id);
  const echoActions = useContext(EchoActionsContext);
  // 不再挂悬停的完整时间浮层：时间已在回合页脚常显，整段正文一悬停就弹日期是噪音（用户要求）。
  const stampDate = item.timestamp ? new Date(item.timestamp) : null;
  const timeText = stampDate && !Number.isNaN(stampDate.getTime())
    ? stampDate.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false })
    : null;
  if (item.type === "user_text") {
    // 斜杠命令在 transcript 里是一条 XML 包裹的用户消息。渲染成命令徽章 + 可展开输出，
    // 而不是把 <command-name> 这类标签摊在对话里让人自己读。
    const parts = parseUserText(item.text);
    const { body, images } = splitUserText(parts.text);
    if (parts.local) {
      // 只剩免责声明（写给模型的 caveat）的那条：对人零信息量，整条不渲染。
      if (!parts.commands.length && !parts.blocks.length && !parts.crossMessages.length && !parts.text) return null;
      return (
        <article className="chat-message is-user is-command" data-msg-id={item.id}>
          {parts.commands.map((command, index) => (
            <span className="chat-command" key={`${command.name}-${index}`}>
              <span className="chat-command-icon" aria-hidden="true">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"><path d="M4 17l6-6-6-6M12 19h8" /></svg>
              </span>
              <span className="chat-command-name">{command.name || t.chat.localCommand}</span>
              {command.args && <span className="chat-command-args">{command.args}</span>}
            </span>
          ))}
          {/* 跨会话消息：另一个 Claude 会话发来的话。它是**真的对话内容**，不是机器记录，
              所以默认展开、正文按原样排（对方写的是自然语言，不是 markdown）；只把
              来源标出来，好和用户自己说的话区分开。 */}
          {parts.crossMessages.map((message, index) => (
            <CrossMessageBlock message={message} key={`cross-${index}`} />
          ))}
          {images.length > 0 && <ImageRow images={images} />}
          {body && <div className="chat-text">{body}</div>}
          {/* 机器记录一律默认收起、按原文顺序排（命令输出 /cost、/status 能刷一屏；通知是整段
              XML+JSON；兜底的系统记录、hook 反馈同理），一行摘要足够定位，想看细节再展开。
              记忆键按类别各自计数，沿用旧前缀，用户此前展开过的不因改版全部收回。 */}
          {(() => {
            const seen: Record<string, number> = {};
            return parts.blocks.map((block) => {
              const n = (seen[block.kind] = (seen[block.kind] ?? -1) + 1);
              const prefix = { stdout: "cmd", notification: "notify", record: "record", hook: "hook" }[block.kind];
              const label = { stdout: t.chat.commandOutput, notification: t.chat.taskNotification, record: t.chat.systemRecord, hook: t.chat.hookFeedback }[block.kind];
              return (
                <RememberedDetails className="chat-tool chat-command-out" memoryKey={`${prefix}:${item.id}:${n}`} key={`${prefix}-${n}`}>
                  <summary>
                    <span className="chat-tool-name">{label}</span>
                    <span className="chat-tool-summary">{stripAnsi(block.summary)}</span>
                    <span className="chat-tool-chevron">›</span>
                  </summary>
                  <pre>{stripAnsi(block.body)}</pre>
                </RememberedDetails>
              );
            });
          })()}
        </article>
      );
    }
    // 用户消息保持原文（用户不是在写 markdown，行首 # 变大标题只会失真）。
    // 气泡只包正文；时间与复制在气泡外右下方（Kimi 的用户回合页脚）。
    return (
      // data-msg-id：右侧用户消息导航（UserMessageNav）按它定位、跳转。
      <article className={"chat-message is-user" + (images.length ? " has-images" : "")} data-msg-id={item.id}>
        {images.length > 0 && <ImageRow images={images} />}
        {body && <UserBubbleText id={item.id} body={body} />}
        {(body || timeText || echoPhase || item.interjected) && <div className="chat-user-foot">
          {/* 来源与送达是这条消息的事实，标在页脚：中途插入解释它为何出现在回复中间；
              未确认是回显超时仍不见落盘，照实说，不悄悄撤掉。 */}
          {item.interjected && <span className="chat-user-note" data-tip={t.chat.interjectedBadgeTip}>{t.chat.interjectedBadge}</span>}
          {echoPhase === "uncertain" && <EchoUncertainActions itemId={item.id} actions={echoActions} />}
          {/* 发出时以为空闲、其实 CLI 还在忙：它把这条收进了队列，照实说「已排队」。 */}
          {echoPhase === "accepted" && <span className="chat-user-note" data-tip={t.chat.echoQueuedTip}>{t.chat.echoQueued}</span>}
          {/* 先上屏再发送：写终端期间转一个小圈；没写进去就照实标失败，给重发与放回输入框。 */}
          {echoPhase === "sending" && <span className="chat-user-note is-sending" role="status"><span className="chat-model-spinner" aria-hidden="true" />{t.chat.echoSending}</span>}
          {echoPhase === "failed" && <EchoFailedActions itemId={item.id} actions={echoActions} />}
          {body && <MessageCopyButton text={body} className="chat-turn-copy" />}
          {body && <UndoButton id={item.id} text={body} />}
          {timeText && <time className="chat-turn-time">{timeText}</time>}
        </div>}
      </article>
    );
  }
  if (item.type === "turn_error") {
    // 回合错误（API Error 等，CC 以 model=<synthetic> 写盘的系统插入文案）：这不是模型
    // 说的话，套错误皮而不是正文皮——红标签一眼可辨故障，原文保留供排查。原文是固定
    // 短句不是 markdown，直接平铺。
    return <TurnErrorCard item={item} latest={latest} />;
  }
  if (item.type === "assistant_text" || item.type === "assistant_delta") {
    return (
      <article className="chat-message is-assistant">
        <div className="chat-text chat-md"><ChatMarkdown text={item.text} /></div>
      </article>
    );
  }
  if (item.type === "reasoning" || item.type === "reasoning_delta") {
    // 长推理默认收成**预览**（显示开头几行并渐隐），而不是整段藏起来——既能一眼看到
    // agent 在想什么，又不会让上百行把结论和后续对话挤出屏幕。短的直接摊开，
    // 没必要为几行内容加一次点击。开合定夺在 ReasoningBlock(首挂载一次,流式不翻转)。
    return <ReasoningBlock item={item} nextStamp={nextStamp} />;
  }
  if (item.type === "tool_use") {
    return (
      <RememberedDetails className="chat-tool" memoryKey={`tool:${item.id}`}>
        <summary>
          <span className="chat-tool-icon"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><path d="M4 17l6-6-6-6M12 19h8" /></svg></span>
          <span className="chat-tool-name">{item.name}</span><span className="chat-tool-summary">{item.summary}</span><span className="chat-tool-chevron">›</span>
        </summary>
        <pre>{item.summary}</pre>
      </RememberedDetails>
    );
  }
  if (item.type === "tool_result") {
    return (
      <RememberedDetails className={"chat-tool chat-result" + (item.is_error ? " is-error" : "")} memoryKey={`result:${item.id}`}>
        <summary>
          <span className="chat-tool-icon is-file"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><path d="M6 3h8l4 4v14H6zM14 3v5h5" /></svg></span>
          <span className="chat-tool-name">{t.chat.toolResult}</span><span className="chat-tool-summary">{stripAnsi(item.text)}</span><span className="chat-tool-chevron">›</span>
        </summary>
        <pre>{stripAnsi(item.text)}</pre>
      </RememberedDetails>
    );
  }
  if (item.type === "meta" && item.kind.startsWith("handoff:")) {
    // 跨 provider 接续的段间分隔条（kind = "handoff:<目标引擎展示名>"，见 ChatWindow 的
    // prefixItems 组装）：前序段历史内联在上方，分隔条标明从哪一段起换了引擎。
    return <div className="chat-meta"><span />{t.chat.handoffDivider(item.kind.slice("handoff:".length))}<span /></div>;
  }
  // 复制到其他引擎的新会话：开头那条「请读交接文件」换成一句来历说明，路径原文不上屏。
  if (item.type === "meta" && item.kind === "handoff-copied") {
    return <div className="chat-meta"><span />{t.chat.handoffCopied}<span /></div>;
  }
  // 中断分隔条：CLI 记下的「用户按了 Esc」，标明上方那轮回复是被打断的、不是自然结束。
  if (item.type === "meta" && item.kind === "interrupted") {
    return <div className="chat-meta"><span />{t.chat.turnInterrupted}<span /></div>;
  }
  // 送达回执在 reduceChatEvents 就滤掉了，走到这里说明有绕过它的来源，不画。
  if (item.type === "prompt_queued") return null;
  return <CompactDivider item={item} summary={compactSummary} />;
}
