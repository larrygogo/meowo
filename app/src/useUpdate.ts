import { useCallback, useEffect, useRef, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import type { DownloadEvent } from "@tauri-apps/plugin-updater";

type UpdateHandle = {
  version: string;
  downloadAndInstall: (cb?: (e: DownloadEvent) => void) => Promise<void>;
};

export type UpdateStatus = "checking" | "latest" | "available" | "downloading" | "error";

/// 检查更新；对外暴露状态/版本/进度、手动重检 recheck() 与安装 apply()。
/// 非 Tauri 环境（测试/浏览器）或网络失败一律降级为 error，不抛错。
export function useUpdate() {
  const [status, setStatus] = useState<UpdateStatus>("checking");
  const [version, setVersion] = useState<string | null>(null);
  // null = 总大小未知（响应无 Content-Length），UI 显示不带百分比的「下载中…」。
  const [progress, setProgress] = useState<number | null>(0);
  const handleRef = useRef<UpdateHandle | null>(null);

  const recheck = useCallback(async () => {
    setStatus("checking");
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const up = await check();
      if (up) {
        handleRef.current = up as unknown as UpdateHandle;
        setVersion(up.version);
        setStatus("available");
      } else {
        setStatus("latest");
      }
    } catch {
      setStatus("error");
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
        if (e.event === "Started") {
          total = e.data.contentLength ?? 0;
          if (total === 0) setProgress(null);
        } else if (e.event === "Progress") {
          got += e.data.chunkLength;
          if (total > 0) setProgress(Math.min(100, Math.round((got / total) * 100)));
        } else if (e.event === "Finished") setProgress(100);
      });
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (err) {
      console.error("[update] 安装失败：", err);
      setStatus("error");
      // 广播给其它窗口（设置页据此复位「更新中…」按钮）。
      emit("update-failed").catch(() => {});
    }
  }, []);

  return { status, version, progress, apply, recheck };
}
