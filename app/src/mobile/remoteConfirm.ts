// 手机远程页的确认框：应用内模态，替掉 window.confirm。
//
// 为什么：桌面的 appConfirm 走后端开的主题小窗（ConfirmWindow），手机上没有那扇窗，原先转成
// 浏览器原生 confirm——样式脱节，传进来的 danger 与主按钮文案（「重启这些会话」「结束会话」）
// 全丢，按钮只剩「确定」，而这几处恰恰是会杀进程的操作（独立审查 2026-11-15，手机切换账号
// 「要不要重启 n 个会话」；standards：不用系统原生控件）。
//
// 用纯 DOM 而不是 React：transport 的 IPC 替身在任何组件之外被调用，要一个不依赖渲染树、
// 随叫随有的层。外观复用桌面确认卡（.app-confirm）与模态底（.chat-modal-overlay），不另起样式。
import { pushEscLayer } from "../escLayers";
import { loadedDict } from "../i18n";
import { zh } from "../i18n/zh";

export type RemoteConfirmArgs = {
  title?: string;
  message?: string;
  danger?: boolean;
  confirmLabel?: string | null;
};

function dict() {
  return document.documentElement.lang === "en" ? loadedDict("en") ?? zh : zh;
}

export function remoteConfirm(args: RemoteConfirmArgs): Promise<boolean> {
  const t = dict();
  return new Promise((resolve) => {
    const previousFocus = document.activeElement as HTMLElement | null;
    const overlay = document.createElement("div");
    overlay.className = "chat-modal-overlay remote-confirm-overlay";

    const card = document.createElement("div");
    card.className = "app-confirm remote-confirm";
    card.setAttribute("role", "alertdialog");
    card.setAttribute("aria-modal", "true");

    const title = document.createElement("strong");
    title.id = `remote-confirm-${Date.now().toString(36)}`;
    title.textContent = args.title ?? "";
    card.setAttribute("aria-labelledby", title.id);

    const body = document.createElement("p");
    const span = document.createElement("span");
    span.textContent = args.message ?? "";
    body.append(span);

    const actions = document.createElement("div");
    actions.className = "app-confirm-actions";
    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.textContent = t.dialog.cancel;
    const ok = document.createElement("button");
    ok.type = "button";
    ok.className = args.danger ? "is-danger" : "btn-primary";
    // 主按钮说清后果，未指定才回退通用「确定」（同 ConfirmWindow）。
    ok.textContent = args.confirmLabel || t.dialog.ok;
    actions.append(cancel, ok);

    card.append(title, body, actions);
    overlay.append(card);

    // 对话窗有窗口级「Esc = 拒绝审批」：入栈让它让位（AGENTS：自绘弹层必须接入 escLayers）。
    const popEsc = pushEscLayer();
    let settled = false;
    const finish = (value: boolean) => {
      if (settled) return;
      settled = true;
      popEsc();
      document.removeEventListener("keydown", onKey, true);
      overlay.remove();
      previousFocus?.focus?.();
      resolve(value);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      finish(false);
    };
    cancel.addEventListener("click", () => finish(false));
    ok.addEventListener("click", () => finish(true));
    // 点卡外空白 = 取消（不是同意）：触屏上误触背景最常见，宁可多问一次。
    overlay.addEventListener("click", (event) => {
      if (event.target === overlay) finish(false);
    });
    document.addEventListener("keydown", onKey, true);
    document.body.append(overlay);
    // 默认焦点在取消上：Enter 顺手一按不该通过破坏性动作（同 ConfirmWindow）。
    cancel.focus();
  });
}
