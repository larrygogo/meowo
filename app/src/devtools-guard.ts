// 正式构建下封死 WebView 的「调试入口」：屏蔽右键菜单 + DevTools 快捷键。
// dev 构建（bun run dev）原样放行，方便开发期调试。
export function lockdownInProduction() {
  if (!import.meta.env.PROD) return;

  // 屏蔽 WebView 默认右键菜单（重新加载/另存为/检查等）。
  window.addEventListener("contextmenu", (e) => e.preventDefault(), { capture: true });

  // 封死 DevTools 快捷键：F12 与 Ctrl+Shift+I/J/C。
  // 用 e.code 而非 e.key，避免 Shift 改变字母大小写带来的判定遗漏。
  window.addEventListener(
    "keydown",
    (e) => {
      const isDevtools =
        e.code === "F12" ||
        (e.ctrlKey &&
          e.shiftKey &&
          (e.code === "KeyI" || e.code === "KeyJ" || e.code === "KeyC"));
      if (isDevtools) e.preventDefault();
    },
    { capture: true }
  );
}
