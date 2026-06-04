import { useCallback, useEffect, useRef, useState } from "react";
import type { DownloadEvent } from "@tauri-apps/plugin-updater";

type UpdateHandle = {
  version: string;
  downloadAndInstall: (cb?: (e: DownloadEvent) => void) => Promise<void>;
};

export type UpdateStatus = "checking" | "latest" | "available" | "downloading" | "error";

/// 启动后台检查更新；把结果回写托盘菜单，并对外暴露状态/版本/下载进度与 apply()。
/// 非 Tauri 环境（测试/浏览器）或网络失败一律降级，不抛错。
export function useUpdate() {
  const [status, setStatus] = useState<UpdateStatus>("checking");
  const [version, setVersion] = useState<string | null>(null);
  const [progress, setProgress] = useState(0);
  const handleRef = useRef<UpdateHandle | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const { check } = await import("@tauri-apps/plugin-updater");
        const up = await check();
        if (cancelled) return;
        if (up) {
          handleRef.current = up as unknown as UpdateHandle;
          setVersion(up.version);
          setStatus("available");
        } else {
          setStatus("latest");
        }
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          await invoke("set_update_menu", { version: up ? up.version : null });
        } catch {
          /* 回写托盘失败无所谓 */
        }
      } catch {
        if (!cancelled) setStatus("error");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const apply = useCallback(async () => {
    const up = handleRef.current;
    if (!up) return;
    setStatus("downloading");
    setProgress(0);
    try {
      let total = 0;
      let got = 0;
      await up.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? 0;
        else if (e.event === "Progress") {
          got += e.data.chunkLength;
          if (total > 0) setProgress(Math.min(100, Math.round((got / total) * 100)));
        } else if (e.event === "Finished") setProgress(100);
      });
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (err) {
      console.error("[update] 安装失败：", err);
      setStatus("error");
    }
  }, []);

  return { status, version, progress, apply };
}
