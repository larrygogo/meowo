/// broker 审批的「回应」侧：结算锁、按请求绑定的失败原因、数字键映射、持久放行的按钮文案。
/// 卡片本体在 ApprovalRequestCard（broker 与屏幕两条通道共用的那一张）。
///
/// 为什么单独成模块（对照 Kimi 的 kimi.approvals 仓库）：接收侧早已抽到 useApprovalChannel，
/// 回应侧却散在 ChatWindow 里——审批与结构化提问靠约定共用一个 resolvingApproval，「说明并
/// 拒绝」的表单状态靠 effect 在换请求时手动清零。这里把结算收成一个 hook，卡片按 requestId
/// 挂载（key），卡内状态随请求自然重置，不再需要「记得清」。

import { useEffect, useState } from "react";
import { respondRuntimeRequest, resolvePendingApproval } from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { runtimeRequestCommandUnavailable } from "./runtimeRequest";

type Dict = ReturnType<typeof useT>;

export type ResolveResult = { ok: true } | { ok: false; message: string };

/// broker 挂起请求的结算：普通审批与提问都走统一 respond，仅旧后端命令缺席时回退。
/// 一个会话同一时刻只结算一件：连点、键盘与按钮同时触发都只发一次。
/// 失败原因按 requestId 记：轮询换上新请求后，旧请求的错误不污染新卡。
export function useBrokerResolver(sessionId: number) {
  const t = useT();
  const [resolving, setResolving] = useState(false);
  const [error, setError] = useState<{ requestId: string; message: string } | null>(null);
  const settle = async (
    requestId: string,
    choice: string,
    command: (sessionId: number, requestId: string, choice: string) => Promise<void>,
  ): Promise<ResolveResult | null> => {
    if (resolving) return null;
    setResolving(true);
    try {
      await command(sessionId, requestId, choice);
      setError((current) => (current?.requestId === requestId ? null : current));
      return { ok: true };
    } catch (raw) {
      const message = formatBackendError(raw, t.locale);
      setError({ requestId, message });
      return { ok: false, message };
    } finally {
      setResolving(false);
    }
  };
  const respond = (requestId: string, choice: string) =>
    settle(requestId, choice, async (session, id, answer) => {
      try {
        await respondRuntimeRequest(session, id, answer);
      } catch (raw) {
        // 旧后端只能显示旧题面、没有新命令；只有命令缺席才回退，业务错误绝不二次作答。
        if (!runtimeRequestCommandUnavailable(raw)) throw raw;
        await resolvePendingApproval(session, id, answer);
      }
    });
  const errorFor = (requestId: string) => (error?.requestId === requestId ? error.message : null);
  return { resolving, respond, errorFor };
}

/// 数字键 → 审批选择（对照 Kimi 审批卡的 1/2/3）：1 = 允许一次，2.. = 依次的持久放行。
/// 超出持久放行数量的数字不对应任何按钮，返回 null（不吞键）。Enter 刻意不映射——
/// 输入框外随手一个回车就放行一条命令，换不来那点快捷。
export function approvalChoiceForKey(key: string, suggestionCount: number): string | null {
  if (!/^[1-9]$/.test(key)) return null;
  const digit = Number(key);
  if (digit === 1) return "allow_once";
  return digit - 2 < suggestionCount ? `suggestion:${digit - 2}` : null;
}

/// 焦点所在处另有数字键的主人：输入框 / 文本框（数字是要打的字）、按钮与链接（键归它自己）。
/// 窗口级的数字键监听据此让开；卡上的「1」「2」徽标也按同一判据显隐——走查 C-5：默认焦点就在
/// 输入框，徽标却一直亮着，按下去只把数字打进输入框（p2-digit-in-ta），徽标说了假话。
/// 数字键刻意不像 Esc 那样在空输入框里放行：Esc 拒绝有两段式确认，放行一条命令没有，空框里
/// 随手敲个「1」开头的消息就替用户放行了（ChatWindow.session.test「焦点在输入框：数字键属于输入」）。
export function digitKeysOwnedElsewhere(active: Element | null): boolean {
  if (!(active instanceof HTMLElement)) return false;
  if (active.tagName === "TEXTAREA" || active.tagName === "INPUT" || active.isContentEditable) return true;
  return !!active.closest("button, a, [role='button'], summary");
}

/// 数字键此刻是否归审批卡（徽标显隐用）：跟着焦点走，焦点进出输入框、按钮时重算。
export function useDigitKeysLive(enabled: boolean): boolean {
  const [live, setLive] = useState(() => enabled && !digitKeysOwnedElsewhere(document.activeElement));
  useEffect(() => {
    if (!enabled) return;
    // focusout 时 activeElement 还是旧元素，等这一轮焦点转移落定再读。
    const update = () => window.setTimeout(() => setLive(!digitKeysOwnedElsewhere(document.activeElement)), 0);
    update();
    document.addEventListener("focusin", update);
    document.addEventListener("focusout", update);
    return () => {
      document.removeEventListener("focusin", update);
      document.removeEventListener("focusout", update);
    };
  }, [enabled]);
  return enabled && live;
}

function suggestionParts(suggestion: unknown, index: number, t: Dict): { base: string; detail: string } {
  if (!suggestion || typeof suggestion !== "object" || Array.isArray(suggestion)) {
    return { base: t.chat.allowSuggested(index + 1), detail: "" };
  }
  const entry = suggestion as Record<string, unknown>;
  const base = (() => { switch (entry.destination) {
    case "session": return t.chat.allowSession;
    case "localSettings": return t.chat.allowLocalProject;
    case "projectSettings": return t.chat.allowProject;
    case "userSettings": return t.chat.allowUser;
    default: return t.chat.allowSuggested(index + 1);
  } })();
  const firstRule = Array.isArray(entry.rules) ? entry.rules[0] : null;
  if (!firstRule || typeof firstRule !== "object" || Array.isArray(firstRule)) {
    // 放行的不是一条命令规则，而是一个目录（claude 2.1.325 真机抓到的建议：
    // `{type:"addDirectories", destination:"session", directories:[…]}`，终端里对应
    // `Yes, and always allow access to <目录> from this project`）。不写出目录，按钮就只剩
    // 「本会话内允许」，看不出放行的范围比这条命令大。
    const directory = Array.isArray(entry.directories) && typeof entry.directories[0] === "string" ? entry.directories[0] : "";
    return { base, detail: directory };
  }
  const rule = firstRule as Record<string, unknown>;
  const tool = typeof rule.toolName === "string" ? rule.toolName : "";
  const content = typeof rule.ruleContent === "string" ? rule.ruleContent : "";
  return { base, detail: content || tool };
}

export function approvalSuggestionLabel(suggestion: unknown, index: number, t: Dict): string {
  const { base, detail } = suggestionParts(suggestion, index, t);
  if (!detail) return base;
  const short = detail.length > 42 ? detail.slice(0, 41) + "…" : detail;
  return `${base} · ${short}`;
}

/// 悬浮提示与按钮文案同源，但不截断：按钮上 42 字截断的长命令在这里能看全。
/// 此前直接 JSON.stringify 整个建议——把协议内部结构甩给用户，谁也读不动。
export function approvalSuggestionTip(suggestion: unknown, index: number, t: Dict): string {
  const { base, detail } = suggestionParts(suggestion, index, t);
  return detail ? `${base} · ${detail}` : base;
}
