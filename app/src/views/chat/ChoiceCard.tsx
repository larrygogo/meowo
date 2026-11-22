// 「点一个」方案卡（Meowo 的 MCP 工具 offer_choices）：agent 把 2~4 个方案并排摆在正文里，用户点一个，
// 可补一句，卡片以用户身份发一条消息回给 agent（choiceReply.ts）。对照 v0 多版本、Midjourney 四宫格：
// 摆在面前一眼横比；缩略图看不清细节就点放大，进大图相册左右翻，大图里也能直接「选这个」。
// 三版对照与取舍见 .lead/20261122-rich-display/decisions.md。
//
// 后端回执（chat_choices.rs）：一句说明，每个带截图的方案一行图片引用，最后一行
// `[Choices: {JSON}]` 是题面与方案（image 是引用行的下标）。界面只认统一工具名，不按 Provider 分支。
import { memo, useContext, useMemo, useState, type KeyboardEvent } from "react";
import { isRemotePath, safeFileSrc } from "../../assetSrc";
import { useT } from "../../i18n";
import { busyProps } from "../../busyButton";
import { imeComposing } from "../../ime";
import { ChatActionsContext } from "./chatActions";
import { fileNameOf, ImageFailure, imageErrorText, sentImageReceipt, useImageLoad } from "./ChatImage";
import { formatChoiceAnswer, parseChoiceAnswer, type ChoiceAnswer } from "./choiceReply";
import { Lightbox, type GalleryItem } from "./Lightbox";
import { useToolRunState } from "./toolState";
import type { ToolResultItem, ToolUseItem } from "./shared";

export type ChoiceOption = { label: string; description: string | null; image: string | null };
export type ChoicesReceipt = { question: string; options: ChoiceOption[] };

const MARKER = "[Choices: ";
const IMAGE_REF = /\[Image(?: #\d+)?: source: ([^\]]+?)\]/g;

export function parseChoicesReceipt(text: string): ChoicesReceipt | null {
  const line = text.split("\n").map((entry) => entry.trim()).reverse().find((entry) => entry.startsWith(MARKER) && entry.endsWith("]"));
  if (!line) return null;
  let raw: Record<string, unknown>;
  try {
    raw = JSON.parse(line.slice(MARKER.length, -1)) as Record<string, unknown>;
  } catch {
    return null;
  }
  // 图片只认引用行（受后端可信目录检查）；摘要里只有下标。引用行被改写成普通文字时，下标落空，按没图显示。
  const images = [...text.matchAll(IMAGE_REF)].map((match) => match[1].trim());
  const question = typeof raw.question === "string" ? raw.question.trim() : "";
  const options = Array.isArray(raw.options)
    ? raw.options.flatMap((entry): ChoiceOption[] => {
      if (typeof entry !== "object" || entry === null) return [];
      const option = entry as Record<string, unknown>;
      if (typeof option.label !== "string" || !option.label.trim()) return [];
      const at = typeof option.image === "number" ? option.image : -1;
      return [{
        label: option.label.trim(),
        description: typeof option.description === "string" && option.description.trim() ? option.description.trim() : null,
        image: images[at] ?? null,
      }];
    })
    : [];
  return options.length >= 2 ? { question, options } : null;
}

export const ChoiceMessage = memo(function ChoiceMessage({ item, result, followUp }: {
  item: ToolUseItem;
  result?: ToolResultItem;
  /** 这张卡之后的第一条用户消息；还没有时 undefined。用来认出当时选了哪个（回放旧会话也一样）。 */
  followUp?: string;
}) {
  const t = useT();
  const state = useToolRunState(result);
  const receipt = useMemo(() => (result && !result.is_error ? parseChoicesReceipt(result.text) : null), [result]);
  let content;
  if (state === "running" || state === "waiting") {
    content = (
      <div className="chat-sent-pending" role="status">
        <span className="chat-model-spinner" aria-hidden="true" />
        {t.chat.choicesPreparing}
      </div>
    );
  } else if (state === "interrupted") {
    content = <ImageFailure title={t.chat.choicesFailed} reason={t.chat.interrupted} name={null} />;
  } else if (!receipt) {
    const { error } = sentImageReceipt(result);
    // 参数不对（缺题面、方案数不对）的回执是给 agent 看的一句话，原样显示比套一个笼统的原因有用。
    const reason = error?.code === "not_found" && error.message ? error.message : error ? imageErrorText(t, error) : t.chat.imageErrors.unknown;
    content = <ImageFailure title={t.chat.choicesFailed} reason={reason} name={null} />;
  } else {
    content = <ChoiceCard cardId={item.id} receipt={receipt} followUp={followUp} />;
  }
  return (
    <article className="chat-message chat-sent" data-enter-id={item.id}>
      {content}
    </article>
  );
});

/// 已经从卡上发出去的选择，按工具调用 id 记在组件之外（做空第一轮）：消息发出后、回到时间线之前
/// （agent 没按说明结束这一轮，消息在排队），切走会话再切回来，卡片会重新挂载；只记在 state 里
/// 它又变回可点，会发第二次。
const sentChoices = new Map<string, ChoiceAnswer>();

function ChoiceCard({ cardId, receipt, followUp }: { cardId: string; receipt: ChoicesReceipt; followUp?: string }) {
  const t = useT();
  const actions = useContext(ChatActionsContext);
  const { question, options } = receipt;
  const [selected, setSelected] = useState<number | null>(null);
  const [note, setNote] = useState("");
  const [sending, setSending] = useState(false);
  const [sendFailed, setSendFailed] = useState(false);
  // 发出去之后先按发出的结果显示（先给结果）；时间线里的那条用户消息到了，两者一致。
  const [sent, setSent] = useState<ChoiceAnswer | null>(() => sentChoices.get(cardId) ?? null);
  const [expanded, setExpanded] = useState(false);
  const [gallery, setGallery] = useState<number | null>(null);

  const card = useMemo(() => ({ question, labels: options.map((option) => option.label) }), [question, options]);
  const recorded = followUp !== undefined ? parseChoiceAnswer(followUp, card) : null;
  const answer = sent ?? recorded;
  // 之后已经有用户消息、却不是在卡上选的：用户直接打字回复了，这张卡不再接受点选。
  const skipped = !answer && followUp !== undefined;
  const open = !answer && !skipped && !!actions;

  const gallerySources = useMemo(() => options.flatMap((option, index) => {
    if (!option.image || isRemotePath(option.image)) return [];
    const src = safeFileSrc(option.image);
    return src ? [{ index, item: { src, name: `${index + 1} · ${option.label}`, caption: option.description, path: option.image } as GalleryItem }] : [];
  }), [options]);

  const send = async (next: ChoiceAnswer) => {
    if (!actions || sending || sentChoices.has(cardId)) return;
    setSending(true);
    setSendFailed(false);
    sentChoices.set(cardId, next);
    const ok = await actions.sendText(formatChoiceAnswer(next, card, t.locale)).catch(() => false);
    setSending(false);
    if (ok) {
      setSent(next);
    } else {
      sentChoices.delete(cardId);
      setSendFailed(true);
    }
  };
  const submit = () => {
    if (selected !== null) void send({ kind: "picked", index: selected, note: note.trim() || null });
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!open || event.target instanceof HTMLInputElement || event.ctrlKey || event.metaKey || event.altKey) return;
    const digit = Number(event.key);
    if (Number.isInteger(digit) && digit >= 1 && digit <= options.length) {
      event.preventDefault();
      setSelected(digit - 1);
    }
  };
  const zoom = (index: number) => {
    const at = gallerySources.findIndex((entry) => entry.index === index);
    if (at >= 0) setGallery(at);
  };

  const grid = (interactive: boolean) => (
    <div className="chat-choices-grid" role={interactive ? "radiogroup" : "list"} aria-label={question || t.chat.choicesTag}
      data-count={options.length} data-images={gallerySources.length > 0 || undefined}>
      {options.map((option, index) => (
        <ChoiceTile key={index} index={index} option={option}
          interactive={interactive}
          selected={interactive ? selected === index : answer?.kind === "picked" && answer.index === index}
          onSelect={() => setSelected(index)}
          onZoom={gallerySources.some((entry) => entry.index === index) ? () => zoom(index) : null} />
      ))}
    </div>
  );

  const lightbox = gallery !== null && gallerySources.length > 0 && (
    <Lightbox items={gallerySources.map((entry) => entry.item)} index={Math.min(gallery, gallerySources.length - 1)} onClose={() => setGallery(null)}
      renderActions={open ? (item) => {
        const index = gallerySources.find((entry) => entry.item === item)?.index;
        if (index === undefined) return null;
        return (
          <button type="button" className="chat-lightbox-action is-pick" aria-pressed={selected === index}
            onClick={() => { setSelected(index); setGallery(null); }}>
            <span>{t.chat.choicesPickThis(index + 1)}</span>
          </button>
        );
      } : undefined} />
  );

  if (!open && (answer || skipped)) {
    const picked = answer?.kind === "picked" ? options[answer.index] : null;
    return (
      <div className="chat-choices is-settled">
        <div className="chat-choices-result">
          {picked?.image && <SettledThumb path={picked.image} />}
          <div className="chat-choices-result-text">
            <span className="chat-choices-result-title">
              {picked && answer?.kind === "picked" ? t.chat.choicesPicked(answer.index + 1, picked.label)
                : answer?.kind === "none" ? t.chat.choicesRejected : t.chat.choicesSkipped}
            </span>
            <span className="chat-choices-result-meta">
              {question}
              {answer?.note ? ` · ${t.chat.choicesNoteShown(answer.note)}` : ""}
            </span>
          </div>
          <button type="button" className="chat-choices-toggle" aria-expanded={expanded} onClick={() => setExpanded((v) => !v)}>
            {expanded ? t.chat.choicesHideAll : t.chat.choicesShowAll}
          </button>
        </div>
        {expanded && grid(false)}
        {lightbox}
      </div>
    );
  }

  return (
    <div className="chat-choices" onKeyDown={onKeyDown}>
      <div className="chat-choices-head">
        <span className="chat-choices-tag">{t.chat.choicesTag}</span>
        {open && <span className="chat-choices-hint">{t.chat.choicesHint(options.length, gallerySources.length > 0)}</span>}
      </div>
      {question && <div className="chat-choices-question">{question}</div>}
      {grid(open)}
      {open && (
        <div className="chat-choices-foot">
          <input className="chat-choices-note" value={note} placeholder={t.chat.choicesNotePlaceholder}
            onChange={(event) => setNote(event.target.value)}
            onKeyDown={(event) => {
              if (event.key !== "Enter" || imeComposing(event)) return;
              event.preventDefault();
              submit();
            }} />
          <button type="button" className="chat-choices-none"
            {...busyProps(sending, () => void send({ kind: "none", note: note.trim() || null }))}>{t.chat.choicesNone}</button>
          {/* 没选方案是表单校验型的不可用（可以 disabled）；发送中走 busyProps，焦点不丢。 */}
          <button type="button" className="chat-choices-send" disabled={selected === null} {...busyProps(sending, submit)}>
            {selected === null ? t.chat.choicesSendIdle : t.chat.choicesSend(selected + 1)}
          </button>
        </div>
      )}
      {sendFailed && <div className="chat-choices-error" role="alert">{t.chat.choicesSendFailed}</div>}
      {lightbox}
    </div>
  );
}

function ChoiceTile({ index, option, interactive, selected, onSelect, onZoom }: {
  index: number;
  option: ChoiceOption;
  interactive: boolean;
  selected: boolean;
  onSelect: () => void;
  onZoom: (() => void) | null;
}) {
  const t = useT();
  const load = useImageLoad(option.image);
  const src = option.image && !isRemotePath(option.image) ? safeFileSrc(option.image) : null;
  const body = (
    <>
      {option.image && (
        <span className="chat-choice-shot">
          {src && !load.failed
            ? <img src={src} alt={option.label} loading="lazy" draggable={false} onLoad={load.onLoad} onError={load.onError} />
            : <span className="chat-choice-shot-missing">{t.chat.imageErrors.unreadable} · {fileNameOf(option.image)}</span>}
        </span>
      )}
      <span className="chat-choice-num" aria-hidden="true">{index + 1}</span>
      <span className="chat-choice-text">
        <b>{option.label}</b>
        {option.description && <small>{option.description}</small>}
      </span>
    </>
  );
  return (
    <div className={"chat-choice" + (selected ? " is-selected" : "") + (option.image ? " has-shot" : "")} role={interactive ? undefined : "listitem"}>
      {interactive
        ? <button type="button" className="chat-choice-pick" role="radio" aria-checked={selected} onClick={onSelect}>{body}</button>
        : <div className="chat-choice-pick">{body}</div>}
      {onZoom && src && !load.failed && (
        <button type="button" className="chat-choice-zoom" aria-label={t.chat.choicesZoom(index + 1)} data-tip={t.chat.choicesZoom(index + 1)} onClick={onZoom}>⤢</button>
      )}
    </div>
  );
}

function SettledThumb({ path }: { path: string }) {
  const load = useImageLoad(path);
  const src = !isRemotePath(path) ? safeFileSrc(path) : null;
  if (!src || load.failed) return null;
  return <img className="chat-choices-result-thumb" src={src} alt="" loading="lazy" draggable={false} onLoad={load.onLoad} onError={load.onError} />;
}
