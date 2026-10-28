export async function copyPlainText(text: string): Promise<boolean> {
  if (!text) return false;
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      // 明文 HTTP 远程页或权限拒绝时继续走 DOM 复制回退。
    }
  }
  const stage = document.createElement("textarea");
  stage.value = text;
  stage.readOnly = true;
  stage.style.position = "fixed";
  stage.style.top = "0";
  stage.style.left = "0";
  stage.style.opacity = "0";
  document.body.appendChild(stage);
  stage.focus();
  stage.setSelectionRange(0, stage.value.length);
  let copied = false;
  try {
    copied = document.execCommand("copy");
  } catch {
    // 老 WebView 没有 execCommand 时保留原文供手动选择。
  }
  stage.remove();
  return copied;
}
