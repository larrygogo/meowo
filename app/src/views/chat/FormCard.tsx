// 小表单卡（Meowo 的 MCP 工具 ask_form）：agent 开工前一次问清几件事，用户在卡上填好提交，卡片以用户身份
// 发一条消息回给 agent（formReply.ts）。单列、一题一块（对照 Claude Code 的 AskUserQuestion、Linear 的
// 新建表单）：标签在上、控件在下，手机上不用横向找；单选 / 多选 / 是否做成可点的选项块，不用小圆点。
//
// 后端回执（chat_forms.rs）：一句说明，最后一行 `[Form: {JSON}]` 是标题与字段。界面只认统一工具名，
// 不按 Provider 分支。发送状态与草稿记在组件之外：切走会话再切回来卡片会重新挂载，不能重发、不能丢草稿。
import { memo, useContext, useId, useMemo, useRef, useState, useSyncExternalStore, type KeyboardEvent } from "react";
import { useT } from "../../i18n";
import { busyProps } from "../../busyButton";
import { ChatActionsContext } from "./chatActions";
import { ImageFailure, imageErrorText, sentImageReceipt } from "./ChatImage";
import { formatFormAnswer, looksLikeCardReply, parseFormAnswer, type FormAnswer, type FormFieldType } from "./formReply";
import { useToolRunState } from "./toolState";
import type { ToolResultItem, ToolUseItem } from "./shared";

export type FormField = {
  label: string;
  type: FormFieldType;
  help: string | null;
  options: string[];
  placeholder: string | null;
  default: string | string[] | boolean | null;
  optional: boolean;
};
export type FormReceipt = { title: string; intro: string | null; fields: FormField[] };

const MARKER = "[Form: ";
const TYPES: readonly FormFieldType[] = ["text", "long_text", "choice", "multi_choice", "yes_no"];

const str = (value: unknown): string | null => (typeof value === "string" && value.trim() ? value.trim() : null);

export function parseFormReceipt(text: string): FormReceipt | null {
  const line = text.split("\n").map((entry) => entry.trim()).reverse().find((entry) => entry.startsWith(MARKER) && entry.endsWith("]"));
  if (!line) return null;
  let raw: Record<string, unknown>;
  try {
    raw = JSON.parse(line.slice(MARKER.length, -1)) as Record<string, unknown>;
  } catch {
    return null;
  }
  const title = str(raw.title);
  if (!title || !Array.isArray(raw.fields)) return null;
  const fields = raw.fields.flatMap((entry): FormField[] => {
    if (typeof entry !== "object" || entry === null) return [];
    const field = entry as Record<string, unknown>;
    const label = str(field.label);
    const type = TYPES.find((kind) => kind === field.type);
    if (!label || !type) return [];
    const options = Array.isArray(field.options) ? field.options.flatMap((o) => (str(o) ? [str(o)!] : [])) : [];
    if ((type === "choice" || type === "multi_choice") && options.length < 2) return [];
    const fallback = field.default;
    const value = typeof fallback === "boolean" || typeof fallback === "string" ? fallback
      : Array.isArray(fallback) ? fallback.filter((v): v is string => typeof v === "string") : null;
    return [{ label, type, help: str(field.help), options, placeholder: str(field.placeholder), default: value, optional: field.optional === true }];
  });
  return fields.length > 0 ? { title, intro: str(raw.intro), fields } : null;
}

export const FormMessage = memo(function FormMessage({ item, result, followUps, sessionId }: {
  item: ToolUseItem;
  result?: ToolResultItem;
  sessionId?: number;
  /** 这张卡之后的用户消息（Transcript.cardFollowUps）：认出当时填了什么。没传 = 拿不到后续。 */
  followUps?: readonly string[];
}) {
  const t = useT();
  const state = useToolRunState(result);
  const receipt = useMemo(() => (result && !result.is_error ? parseFormReceipt(result.text) : null), [result]);
  let content;
  if (state === "running" || state === "waiting") {
    content = (
      <div className="chat-sent-pending" role="status">
        <span className="chat-model-spinner" aria-hidden="true" />
        {t.chat.formPreparing}
      </div>
    );
  } else if (state === "interrupted") {
    content = <ImageFailure title={t.chat.formFailed} reason={t.chat.interrupted} name={null} />;
  } else if (!receipt) {
    const { error } = sentImageReceipt(result);
    const reason = error?.code === "not_found" && error.message ? error.message : error ? imageErrorText(t, error) : t.chat.imageErrors.unknown;
    content = <ImageFailure title={t.chat.formFailed} reason={reason} name={null} />;
  } else {
    content = <FormCard cardId={`${sessionId ?? ""}:${item.id}@${item.timestamp ?? ""}`} receipt={receipt} followUps={followUps ?? []} />;
  }
  return (
    <article className="chat-message chat-sent" data-enter-id={item.id}>
      {content}
    </article>
  );
});

type SendState = { status: "sending" | "sent"; answer: FormAnswer } | { status: "failed" };
const sendStates = new Map<string, SendState>();
const sendListeners = new Set<() => void>();
function setSendState(id: string, state: SendState) {
  sendStates.set(id, state);
  sendListeners.forEach((listener) => listener());
}
function subscribeSend(listener: () => void) {
  sendListeners.add(listener);
  return () => { sendListeners.delete(listener); };
}

/** 草稿：每项一个值。文本是字符串，单选是选项或 null，多选是数组，是否是布尔或 null。 */
type Value = string | string[] | boolean | null;
const drafts = new Map<string, Value[]>();

function initialValue(field: FormField): Value {
  const fallback = field.default;
  if (field.type === "multi_choice") return Array.isArray(fallback) ? fallback.filter((v) => field.options.includes(v)) : [];
  if (field.type === "yes_no") return typeof fallback === "boolean" ? fallback : null;
  if (field.type === "choice") return typeof fallback === "string" && field.options.includes(fallback) ? fallback : null;
  return typeof fallback === "string" ? fallback : "";
}

function isEmpty(value: Value): boolean {
  return value === null || value === "" || (typeof value === "string" && !value.trim()) || (Array.isArray(value) && value.length === 0);
}

function FormCard({ cardId, receipt, followUps }: { cardId: string; receipt: FormReceipt; followUps: readonly string[] }) {
  const t = useT();
  const actions = useContext(ChatActionsContext);
  const { title, intro, fields } = receipt;
  const [values, setValuesState] = useState<Value[]>(() => drafts.get(cardId) ?? fields.map(initialValue));
  const setValue = (index: number, value: Value) => {
    setValuesState((current) => {
      const next = current.map((v, i) => (i === index ? value : v));
      drafts.set(cardId, next);
      return next;
    });
    setMissing((current) => current.filter((i) => i !== index));
  };
  const [missing, setMissing] = useState<number[]>([]);
  const [declining, setDeclining] = useState(false);
  const [note, setNote] = useState("");
  const [expanded, setExpanded] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const sendState = useSyncExternalStore(subscribeSend, () => sendStates.get(cardId));
  const sending = sendState?.status === "sending";
  const sendFailed = sendState?.status === "failed";

  const card = useMemo(() => ({ title, fields: fields.map((field) => ({ label: field.label, type: field.type })) }), [title, fields]);
  let recorded: FormAnswer | null = null;
  let skipped = false;
  for (const text of followUps) {
    recorded = parseFormAnswer(text, card);
    if (recorded) break;
    if (!looksLikeCardReply(text)) {
      skipped = true;
      break;
    }
  }
  const answer = recorded ?? (sendState && sendState.status !== "failed" ? sendState.answer : null);
  const open = !answer && !skipped && !!actions;

  const display = (field: FormField, value: Value): string | null => {
    if (isEmpty(value)) return null;
    if (field.type === "yes_no") return value ? t.chat.formYes : t.chat.formNo;
    if (Array.isArray(value)) return field.options.filter((option) => value.includes(option)).join(t.chat.formListSep);
    return String(value).trim();
  };

  const send = async (next: FormAnswer) => {
    const current = sendStates.get(cardId);
    if (!actions || (current && current.status !== "failed")) return;
    setSendState(cardId, { status: "sending", answer: next });
    const ok = await actions.sendText(formatFormAnswer(next, card, t.locale)).catch(() => false);
    setSendState(cardId, ok ? { status: "sent", answer: next } : { status: "failed" });
    if (ok) drafts.delete(cardId);
  };
  const submit = () => {
    const absent = fields.flatMap((field, index) => (!field.optional && isEmpty(values[index]) ? [index] : []));
    setMissing(absent);
    if (absent.length > 0) {
      // 焦点落到第一项没填的：屏幕阅读器读到它的标签与「必填」，键盘用户直接接着填。
      rootRef.current?.querySelector<HTMLElement>(`[data-form-field="${absent[0]}"] :is(input, textarea, button)`)?.focus();
      return;
    }
    void send({ kind: "filled", values: fields.map((field, index) => display(field, values[index])) });
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!open || event.key !== "Enter" || !(event.ctrlKey || event.metaKey)) return;
    event.preventDefault();
    if (declining) void send({ kind: "declined", note: note.trim() || null });
    else submit();
  };

  if (!open && (answer || skipped)) {
    const filled = answer?.kind === "filled" ? answer : null;
    return (
      <div className="chat-form is-settled">
        <div className="chat-choices-result">
          <span className="chat-form-glyph" aria-hidden="true">{filled ? "✓" : "–"}</span>
          <div className="chat-choices-result-text">
            <span className="chat-choices-result-title">
              {filled ? t.chat.formFilled(title) : answer?.kind === "declined" ? t.chat.formDeclined(title) : t.chat.formSkipped(title)}
            </span>
            <span className="chat-choices-result-meta">
              {filled ? t.chat.formFilledMeta(filled.values.filter((v) => v !== null).length, fields.length)
                : answer?.kind === "declined" && answer.note ? t.chat.choicesNoteShown(answer.note) : t.chat.formFieldCount(fields.length)}
            </span>
          </div>
          {filled && (
            <button type="button" className="chat-choices-toggle" aria-expanded={expanded} onClick={() => setExpanded((v) => !v)}>
              {expanded ? t.chat.choicesHideAll : t.chat.formShowAnswers}
            </button>
          )}
        </div>
        {filled && expanded && (
          <dl className="chat-form-answers">
            {fields.map((field, index) => (
              <div key={index} className="chat-form-answer">
                <dt>{field.label}</dt>
                <dd className={filled.values[index] === null ? "is-blank" : undefined}>{filled.values[index] ?? t.chat.formBlank}</dd>
              </div>
            ))}
          </dl>
        )}
      </div>
    );
  }

  return (
    <div className="chat-form" ref={rootRef} onKeyDown={onKeyDown}>
      <div className="chat-choices-head">
        <span className="chat-choices-tag">{t.chat.formTag}</span>
        {open && <span className="chat-choices-hint">{t.chat.formHint}</span>}
      </div>
      <div className="chat-form-title">{title}</div>
      {intro && <div className="chat-form-intro">{intro}</div>}
      <div className="chat-form-fields">
        {fields.map((field, index) => (
          <FormFieldBlock key={index} index={index} field={field} value={values[index]}
            missing={missing.includes(index)} onChange={(value) => setValue(index, value)} />
        ))}
      </div>
      {missing.length > 0 && <div className="chat-form-missing" role="alert">{t.chat.formMissing(missing.length)}</div>}
      {open && (declining ? (
        <div className="chat-choices-foot">
          <input className="chat-choices-note" value={note} autoFocus placeholder={t.chat.formDeclinePlaceholder} aria-label={t.chat.formDeclinePlaceholder}
            onChange={(event) => setNote(event.target.value)} />
          <button type="button" className="chat-choices-none" onClick={() => setDeclining(false)}>{t.chat.formBack}</button>
          <button type="button" className="chat-choices-send" {...busyProps(sending, () => void send({ kind: "declined", note: note.trim() || null }))}>{t.chat.formDeclineSend}</button>
        </div>
      ) : (
        <div className="chat-choices-foot chat-form-foot">
          <button type="button" className="chat-choices-none" onClick={() => setDeclining(true)}>{t.chat.formDecline}</button>
          <span className="chat-form-foot-gap" />
          <button type="button" className="chat-choices-send" {...busyProps(sending, submit)}>{t.chat.formSubmit}</button>
        </div>
      ))}
      {sendFailed && <div className="chat-choices-error" role="alert">{t.chat.choicesSendFailed}</div>}
    </div>
  );
}

// 发送中、已发出时卡片换成收起的那一行（不渲染字段），所以字段没有「不可用」态。
function FormFieldBlock({ index, field, value, missing, onChange }: {
  index: number;
  field: FormField;
  value: Value;
  missing: boolean;
  onChange: (value: Value) => void;
}) {
  const t = useT();
  const id = useId();
  const labelId = `${id}-label`;
  const helpId = field.help ? `${id}-help` : undefined;
  const common = { "aria-labelledby": labelId, "aria-describedby": helpId, "aria-invalid": missing || undefined, "aria-required": !field.optional || undefined };
  let control;
  if (field.type === "text") {
    control = <input id={id} className="chat-form-input" value={typeof value === "string" ? value : ""} placeholder={field.placeholder ?? undefined} {...common} onChange={(event) => onChange(event.target.value)} />;
  } else if (field.type === "long_text") {
    control = <textarea id={id} className="chat-form-input is-long" rows={3} value={typeof value === "string" ? value : ""} placeholder={field.placeholder ?? undefined} {...common} onChange={(event) => onChange(event.target.value)} />;
  } else {
    const multi = field.type === "multi_choice";
    const options: { key: string; label: string; on: boolean; pick: () => void }[] = field.type === "yes_no"
      ? [true, false].map((yes) => ({ key: String(yes), label: yes ? t.chat.formYes : t.chat.formNo, on: value === yes, pick: () => onChange(value === yes && field.optional ? null : yes) }))
      : field.options.map((option) => {
        const on = multi ? Array.isArray(value) && value.includes(option) : value === option;
        const pick = () => {
          if (multi) {
            const list = Array.isArray(value) ? value : [];
            onChange(on ? list.filter((v) => v !== option) : [...list, option]);
          } else {
            onChange(on && field.optional ? null : option);
          }
        };
        return { key: option, label: option, on, pick };
      });
    control = (
      <div className="chat-form-options" role={multi ? "group" : "radiogroup"} {...common}>
        {options.map((option) => (
          <button key={option.key} type="button" className={"chat-form-option" + (option.on ? " is-on" : "")}
            role={multi ? "checkbox" : "radio"} aria-checked={option.on} onClick={option.pick}>
            {multi && <span className="chat-form-check" aria-hidden="true">{option.on ? "✓" : ""}</span>}
            <span>{option.label}</span>
          </button>
        ))}
      </div>
    );
  }
  return (
    <div className={"chat-form-field" + (missing ? " is-missing" : "")} data-form-field={index}>
      <label className="chat-form-label" id={labelId} htmlFor={field.type === "text" || field.type === "long_text" ? id : undefined}>
        {field.label}
        {field.optional ? <span className="chat-form-optional">{t.chat.formOptional}</span>
          : missing ? <span className="chat-form-required">{t.chat.formRequired}</span> : null}
      </label>
      {field.help && <div className="chat-form-help" id={helpId}>{field.help}</div>}
      {control}
    </div>
  );
}
