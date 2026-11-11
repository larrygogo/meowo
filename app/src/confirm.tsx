/// 应用内确认对话框的请求端。实现是**原生小窗**(后端 confirm.rs 建 `confirm-<id>`
/// 无边框窗口,前端 ConfirmWindow 视图渲染)——应用样式与原生窗口能力(独立拖拽、
/// 可拖出主窗边界)兼得。系统 MessageBox 样式脱节已弃用;window.confirm 会被 Tauri
/// webview 吞掉恒 false,同样不可用(ManagedTerminal 接管流程的历史教训)。
/// 非 Tauri 环境(纯浏览器预览)invoke 抛错 → 按取消收场,绝不静默当同意。
import { invoke } from "@tauri-apps/api/core";

/** 单按钮提示（只有「知道了」），给失败提示用：借确认框会出现「取消 / 确定」两个按钮、
 *  点哪个都一样（UI 调研 shell #13）。建窗失败同 appConfirm 走系统错误框兜底。 */
export function appAlert(message: string, options: { title: string }): Promise<void> {
  return invoke<boolean>("confirm_dialog", {
    title: options.title,
    message,
    danger: false,
    confirmLabel: null,
    alert: true,
  }).then(() => undefined, async (error: unknown) => {
    console.error("[confirm] 提示框打开失败：", error);
    try {
      const { message: showMessage } = await import("@tauri-apps/plugin-dialog");
      await showMessage(message, { title: options.title, kind: "error" });
    } catch {
      /* 非 Tauri 环境：至少有 console 线索 */
    }
  });
}

export function appConfirm(
  message: string,
  options: { title: string; danger?: boolean; confirmLabel?: string },
): Promise<boolean> {
  return invoke<boolean>("confirm_dialog", {
    title: options.title,
    message,
    danger: options.danger ?? false,
    confirmLabel: options.confirmLabel ?? null,
  }).catch(async (error: unknown) => {
    // 确认框没建起来（后端建窗失败等）绝不能静默当取消——用户点了按钮却什么都没发生，
    // 也无从知晓。留 console 线索，并用系统原生错误框兜底可见（样式脱节可接受：
    // 这是罕见的失败路径）；操作本身仍按取消收场，绝不静默当同意。
    console.error("[confirm] 确认框打开失败：", error);
    try {
      const { message: showMessage } = await import("@tauri-apps/plugin-dialog");
      await showMessage(String(error), { title: options.title, kind: "error" });
    } catch {
      /* 非 Tauri 环境（测试/浏览器预览）或插件不可用：至少有 console 线索 */
    }
    return false;
  });
}
