import { useCallback, useEffect, useRef, useState } from "react";
import type { DownloadEvent } from "@tauri-apps/plugin-updater";

type UpdateHandle = {
  version: string;
  downloadAndInstall: (cb?: (e: DownloadEvent) => void) => Promise<void>;
};

export type UpdateStatus = "checking" | "latest" | "available" | "downloading" | "error";

/// 检查更新并把结果回写托盘菜单；对外暴露状态/版本/进度、手动重检 recheck() 与安装 apply()。
/// 非 Tauri 环境（测试/浏览器）或网络失败一律降级为 error，不抛错。
export function useUpdate() {
  const [status, setStatus] = useState<UpdateStatus>("checking");
  const [version, setVersion] = useState<string | null>(null);
  const [progress, setProgress] = useState(0);
  const handleRef = useRef<UpdateHandle | null>(null);

  const recheck = useCallback(async () => {
    setStatus("checking");
    let found: string | null = null;
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const up = await check();
      if (up) {
        handleRef.current = up as unknown as UpdateHandle;
        setVersion(up.version);
        setStatus("available");
        found = up.version;
      } else {
        setStatus("latest");
      }
    } catch {
      setStatus("error");
    }
    // 无论有无更新/失败，都同步托盘文案（失败→保持「检查更新」可点）。
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("set_update_menu", { version: found });
    } catch {
      /* 忽略 */
    }
  }, []);

  useEffect(() => {
    recheck();
  }, [recheck]);

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

  return { status, version, progress, apply, recheck };
}
