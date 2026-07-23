/// 应用内确认对话框的请求端。实现是**原生小窗**(后端 confirm.rs 建 `confirm-<id>`
/// 无边框窗口,前端 ConfirmWindow 视图渲染)——应用样式与原生窗口能力(独立拖拽、
/// 可拖出主窗边界)兼得。系统 MessageBox 样式脱节已弃用;window.confirm 会被 Tauri
/// webview 吞掉恒 false,同样不可用(ManagedTerminal 接管流程的历史教训)。
/// 非 Tauri 环境(纯浏览器预览)invoke 抛错 → 按取消收场,绝不静默当同意。
import { invoke } from "@tauri-apps/api/core";

export function appConfirm(
  message: string,
  options: { title: string; danger?: boolean },
): Promise<boolean> {
  return invoke<boolean>("confirm_dialog", {
    title: options.title,
    message,
    danger: options.danger ?? false,
  }).catch(() => false);
}
