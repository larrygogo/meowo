// 小表单卡（Meowo 的 MCP 工具 ask_form）：agent 开工前一次问清几件事，用户在卡上填好提交，卡片以用户身份
// 发一条消息回给 agent（formReply.ts）。单列、一题一块（对照 Claude Code 的 AskUserQuestion、Linear 的
// 新建表单）：标签在上、控件在下，手机上不用横向找；单选 / 多选 / 是否做成可点的选项块，不用小圆点。
// 单选 / 多选可带「其他」，点了出一行输入框，填选项以外的答案（AskUserQuestion 同款）。
//
// 后端回执（chat_forms.rs）：一句说明，最后一行 `[Form: {JSON}]` 是标题与字段。界面只认统一工具名，
// 不按 Provider 分支。发送状态与草稿记在组件之外：切走会话再切回来卡片会重新挂载，不能重发、不能丢草稿。
import { memo, useContext, useId, useMemo, useRef, useState, useSyncExternalStore, type KeyboardEvent } from "react";
import { useT } from "../../i18n";
import { busyProps } from "../../busyButton";
import { ChatActionsContext } from "./chatActions";
import { ImageFailure } from "./ChatImage";
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
  /** 单选 / 多选多一个「其他」，用户自己填。 */
  allowOther: boolean;
};
export type FormReceipt = { title: string; intro: string | null; fields: FormField[] };

const MARKER = "[Form: ";
const TYPES: readonly FormFieldType[] = ["text", "long_text", "choice", "multi_choice", "yes_no"];
/** 旧版本借过图片工具的错误前缀：`[Image error: not_found] 原因`。显示时去掉前缀只留原因。 */
const LEGACY_ERROR = /^\[Image error: [^\]]*\]\s*/;

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
    const choice = type === "choice" || type === "multi_choice";
    const options = Array.isArray(field.options) ? field.options.flatMap((o) => (str(o) ? [str(o)!] : [])) : [];
    if (choice && options.length < 2) return [];
    const fallback = field.default;
    const value = typeof fallback === "boolean" || typeof fallback === "string" ? fallback
      : Array.isArray(fallback) ? fallback.filter((v): v is string => typeof v === "string") : null;
    return [{
      label, type, help: str(field.help), options, placeholder: str(field.placeholder), default: value,
      optional: field.optional === true, allowOther: choice && field.allow_other === true,
    }];
  });
  return fields.length > 0 ? { title, intro: str(raw.intro), fields } : null;
}

export const FormMessage = memo(function FormMessage({ item, result, followUps, sessionId }: {
  item: ToolUseItem;
  result?: ToolResultItem;
  sessionId?: number;
  /** 这张卡之后的用户消息（Transcript.formFollowUps）：认出当时填了什么。没传 = 拿不到后续。 */
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
    // 参数不对的回执是给 agent 看的一句话（「表单没发出去：…」），原样显示比套一个笼统的原因有用。
    const reason = result?.text.trim().replace(LEGACY_ERROR, "") || t.chat.imageErrors.unknown;
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

/** 每项一个值：文本是字符串，单选是选项或 null，多选是数组，是否是布尔或 null。 */
type Value = string | string[] | boolean | null;
/** 「其他」：勾没勾、填了什么。 */
type Other = { on: boolean; text: string };
/** 草稿整份记在组件外（审查 2026-11-23）：切会话回来，填到一半的字段、「不填了」写的那句都还在。 */
type Draft = { values: Value[]; others: Other[]; declining: boolean; note: string };
const drafts = new Map<string, Draft>();

function initialValue(field: FormField): Value {
  const fallback = field.default;
  if (field.type === "multi_choice") return Array.isArray(fallback) ? fallback.filter((v) => field.options.includes(v)) : [];
  if (field.type === "yes_no") return typeof fallback === "boolean" ? fallback : null;
  if (field.type === "choice") return typeof fallback === "string" && field.options.includes(fallback) ? fallback : null;
  return typeof fallback === "string" ? fallback : "";
}

function isBlank(value: Value): boolean {
  return value === null || (typeof value === "string" && !value.trim()) || (Array.isArray(value) && value.length === 0);
}

function FormCard({ cardId, receipt, followUps }: { cardId: string; receipt: FormReceipt; followUps: readonly string[] }) {
  const t = useT();
  const actions = useContext(ChatActionsContext);
  const { title, intro, fields } = receipt;
  const [draft, setDraftState] = useState<Draft>(() => drafts.get(cardId)
    ?? { values: fields.map(initialValue), others: fields.map(() => ({ on: false, text: "" })), declining: false, note: "" });
  const setDraft = (update: (current: Draft) => Draft) => setDraftState((current) => {
    const next = update(current);
    drafts.set(cardId, next);
    return next;
  });
  const { values, others, declining, note } = draft;
  const [missing, setMissing] = useState<number[]>([]);
  const clearMissing = (index: number) => setMissing((current) => current.filter((i) => i !== index));
  const setValue = (index: number, value: Value) => {
    setDraft((current) => ({ ...current, values: current.values.map((v, i) => (i === index ? value : v)) }));
    clearMissing(index);
  };
  const setOther = (index: number, other: Other) => {
    setDraft((current) => ({ ...current, others: current.others.map((o, i) => (i === index ? other : o)) }));
    clearMissing(index);
  };
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

  /** 这一项要发出去的文字；null 表示没填。「其他」勾上但没写字也算没填。 */
  const display = (field: FormField, index: number): string | null => {
    const value = values[index];
    const other = others[index]?.on ? others[index].text.trim() : "";
    if (field.type === "yes_no") return value === null ? null : value ? t.chat.formYes : t.chat.formNo;
    // 单选开着「其他」就只认填的字（没写算没填）；关掉「其他」，原来选的那项还在。
    if (field.type === "choice") return others[index]?.on ? other || null : typeof value === "string" ? value : null;
    if (field.type === "multi_choice") {
      const picked = Array.isArray(value) ? field.options.filter((option) => value.includes(option)) : [];
      // 「其他」填的字可能自带「、」：标上「其他：」，AI 分得清哪段是自己填的（复验 2026-11-23）。
      const all = other ? [...picked, t.chat.formOtherPrefix(other)] : picked;
      return all.length > 0 ? all.join(t.chat.formListSep) : null;
    }
    return isBlank(value) ? null : String(value).trim();
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
    const answers = fields.map((field, index) => display(field, index));
    const absent = fields.flatMap((field, index) => (!field.optional && answers[index] === null ? [index] : []));
    setMissing(absent);
    if (absent.length > 0) {
      // 焦点落到第一项没填的：屏幕阅读器读到它的标签与「必填」，键盘用户直接接着填。
      // 「其他」开着却没写字时，焦点给它的输入框（复验 2026-11-23）。
      const block = rootRef.current?.querySelector<HTMLElement>(`[data-form-field="${absent[0]}"]`);
      (block?.querySelector<HTMLElement>("[data-form-other]") ?? block?.querySelector<HTMLElement>("input, textarea, button"))?.focus();
      return;
    }
    void send({ kind: "filled", values: answers });
  };
  const decline = () => void send({ kind: "declined", note: note.trim() || null });
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!open || event.key !== "Enter" || !(event.ctrlKey || event.metaKey)) return;
    event.preventDefault();
    if (declining) decline();
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
          <FormFieldBlock key={index} index={index} field={field} value={values[index]} other={others[index]}
            missing={missing.includes(index)} onChange={(value) => setValue(index, value)} onOther={(other) => setOther(index, other)} />
        ))}
      </div>
      {missing.length > 0 && <div className="chat-form-missing" role="alert">{t.chat.formMissing(missing.length)}</div>}
      {open && (declining ? (
        <div className="chat-choices-foot">
          <input className="chat-choices-note" value={note} autoFocus placeholder={t.chat.formDeclinePlaceholder} aria-label={t.chat.formDeclinePlaceholder}
            onChange={(event) => { const next = event.target.value; setDraft((current) => ({ ...current, note: next })); }} />
          <button type="button" className="chat-choices-none" onClick={() => setDraft((current) => ({ ...current, declining: false }))}>{t.chat.formBack}</button>
          <button type="button" className="chat-choices-send" {...busyProps(sending, decline)}>{t.chat.formDeclineSend}</button>
        </div>
      ) : (
        <div className="chat-choices-foot chat-form-foot">
          <button type="button" className="chat-choices-none" onClick={() => setDraft((current) => ({ ...current, declining: true }))}>{t.chat.formDecline}</button>
          <span className="chat-form-foot-gap" />
          <button type="button" className="chat-choices-send" {...busyProps(sending, submit)}>{t.chat.formSubmit}</button>
        </div>
      ))}
      {sendFailed && <div className="chat-choices-error" role="alert">{t.chat.choicesSendFailed}</div>}
    </div>
  );
}

// 发送中、已发出时卡片换成收起的那一行（不渲染字段），所以字段没有「不可用」态。
function FormFieldBlock({ index, field, value, other, missing, onChange, onOther }: {
  index: number;
  field: FormField;
  value: Value;
  other: Other;
  missing: boolean;
  onChange: (value: Value) => void;
  onOther: (other: Other) => void;
}) {
  const t = useT();
  const id = useId();
  const labelId = `${id}-label`;
  const helpId = field.help ? `${id}-help` : undefined;
  const common = { "aria-labelledby": labelId, "aria-describedby": helpId, "aria-invalid": missing || undefined, "aria-required": !field.optional || undefined };
  let control;
  if (field.type === "text") {
    control = <input id={id} className="chat-form-input" value={typeof value === "string" ? value : ""} placeholder={field.placeholder ?? undefined}
      {...common} onChange={(event) => onChange(event.target.value)} />;
  } else if (field.type === "long_text") {
    control = <textarea id={id} className="chat-form-input is-long" rows={3} value={typeof value === "string" ? value : ""} placeholder={field.placeholder ?? undefined}
      {...common} onChange={(event) => onChange(event.target.value)} />;
  } else {
    const multi = field.type === "multi_choice";
    const options: { key: string; label: string; on: boolean; pick: () => void }[] = field.type === "yes_no"
      ? [true, false].map((yes) => ({ key: String(yes), label: yes ? t.chat.formYes : t.chat.formNo, on: value === yes, pick: () => onChange(value === yes && field.optional ? null : yes) }))
      : field.options.map((option) => {
        const on = multi ? Array.isArray(value) && value.includes(option) : value === option && !other.on;
        const pick = () => {
          if (multi) {
            const list = Array.isArray(value) ? value : [];
            onChange(on ? list.filter((v) => v !== option) : [...list, option]);
            return;
          }
          // 单选：点一个选项就退出「其他」；再点已选中的（可选题）是清空。
          if (other.on) onOther({ ...other, on: false });
          onChange(on && field.optional ? null : option);
        };
        return { key: option, label: option, on, pick };
      });
    if (field.allowOther) {
      options.push({
        key: "\u0000other", label: t.chat.formOther, on: other.on,
        // 不清掉单选原来的值：关掉「其他」时它回来（复验 2026-11-23：曾把默认值弄丢）。
        pick: () => onOther({ ...other, on: !other.on }),
      });
    }
    control = (
      <>
        <div className="chat-form-options" role={multi ? "group" : "radiogroup"} {...common}>
          {options.map((option) => (
            <button key={option.key} type="button" className={"chat-form-option" + (option.on ? " is-on" : "")}
              role={multi ? "checkbox" : "radio"} aria-checked={option.on} onClick={option.pick}>
              {multi && <span className="chat-form-check" aria-hidden="true">{option.on ? "✓" : ""}</span>}
              <span>{option.label}</span>
            </button>
          ))}
        </div>
        {other.on && (
          <input className="chat-form-input" data-form-other="" value={other.text} autoFocus placeholder={t.chat.formOtherPlaceholder} aria-label={`${field.label} · ${t.chat.formOther}`}
            aria-invalid={missing || undefined} onChange={(event) => onOther({ on: true, text: event.target.value })} />
        )}
      </>
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
