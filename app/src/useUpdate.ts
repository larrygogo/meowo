import { useCallback, useEffect, useRef, useState } from "react";

type UpdateHandle = { version: string; downloadAndInstall: () => Promise<void> };

/// 启动后台检查更新；有新版则暴露版本号与 apply()（下载安装并重启）。
/// 非 Tauri 环境（测试/浏览器）或检查失败一律静默。
export function useUpdate() {
  const [version, setVersion] = useState<string | null>(null);
  const [updating, setUpdating] = useState(false);
  const handleRef = useRef<UpdateHandle | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const { check } = await import("@tauri-apps/plugin-updater");
        const up = await check();
        if (up && !cancelled) {
          handleRef.current = up as unknown as UpdateHandle;
          setVersion(up.version);
        }
      } catch {
        /* 静默 */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const apply = useCallback(async () => {
    const up = handleRef.current;
    if (!up) return;
    setUpdating(true);
    try {
      await up.downloadAndInstall();
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (err) {
      console.error("[update] 安装失败：", err);
      setUpdating(false);
    }
  }, []);

  return { version, updating, apply };
}
