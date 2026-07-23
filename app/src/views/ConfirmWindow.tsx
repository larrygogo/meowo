/// 确认对话框小窗(label `confirm-<id>`,由后端 confirm.rs 创建)。无边框,整卡可拖拽
/// (原生窗口拖动,可拖出主窗边界);内容经命令取回而不是 URL 参数(任意语言文本免转义)。
/// Esc = 取消;默认焦点在取消上,Enter 顺手一按不该通过破坏性动作。
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useT } from "../i18n";
import { useShowWhenReady } from "../useShowWhenReady";

type Payload = { title: string; message: string; danger: boolean };

export function ConfirmWindow() {
  const t = useT();
  useShowWhenReady();
  const [payload, setPayload] = useState<Payload | null>(null);
  const id = Number(getCurrentWindow().label.slice("confirm-".length));
  const decide = (ok: boolean) => {
    // 结果送回请求方;窗口由后端在收到结果后关闭。失败也不留悬窗:自关兜底。
    void invoke("confirm_dialog_result", { id, ok }).catch(() => {
      void getCurrentWindow().close().catch(() => {});
    });
  };
  useEffect(() => {
    invoke<Payload>("confirm_dialog_payload", { id })
      .then(setPayload)
      // 取不到内容(请求已被并发取消)就直接按取消收场,不渲染空壳。
      .catch(() => decide(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") decide(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);
  if (!payload) return null;
  return (
    <div className="app-confirm is-window" data-tauri-drag-region>
      <strong data-tauri-drag-region>{payload.title}</strong>
      <p data-tauri-drag-region>{payload.message}</p>
      <div className="app-confirm-actions">
        <button type="button" autoFocus onClick={() => decide(false)}>{t.dialog.cancel}</button>
        <button type="button" className={payload.danger ? "is-danger" : "is-primary"} onClick={() => decide(true)}>
          {t.dialog.ok}
        </button>
      </div>
    </div>
  );
}
