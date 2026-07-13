import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { checkUpdate, downloadAndInstallUpdate } from "./api";

export type UpdateStatus = "checking" | "latest" | "available" | "downloading" | "error";

/// 检查更新；对外暴露状态/版本/更新说明/进度、手动重检 recheck() 与安装 apply()。
/// 非 Tauri 环境（测试/浏览器）或网络失败一律降级为 error，不抛错。
/// 下载/安装的唯一调用方是更新窗口（views/Updater）；主窗/设置窗只做只读检查（红点/角标），
/// 不再有跨窗口 trigger-update/update-failed 事件协议（旧协议曾致按钮死锁）。
export function useUpdate() {
  const [status, setStatus] = useState<UpdateStatus>("checking");
  const [version, setVersion] = useState<string | null>(null);
  // 新版本的更新说明（release notes，来自 updater manifest 的 notes 字段），无则 null。
  const [notes, setNotes] = useState<string | null>(null);
  // null = 总大小未知（响应无 Content-Length），UI 显示不带百分比的「下载中…」。
  const [progress, setProgress] = useState<number | null>(0);
  const checkedRef = useRef(false);

  // 返回本次检查的结果状态（调用方拿结果不能依赖异步 state）。
  const recheck = useCallback(async (): Promise<UpdateStatus> => {
    setStatus("checking");
    checkedRef.current = false;
    try {
      const up = await checkUpdate();
      if (up) {
        checkedRef.current = true;
        setVersion(up.version);
        setNotes(up.body?.trim() ? up.body : null);
        setStatus("available");
        return "available";
      }
      checkedRef.current = false;
      setStatus("latest");
      return "latest";
    } catch {
      checkedRef.current = false;
      setStatus("error");
      return "error";
    }
  }, []);

  useEffect(() => {
    recheck();
  }, [recheck]);

  const apply = useCallback(async () => {
    if (!checkedRef.current) return;
    setStatus("downloading");
    setProgress(0);
    try {
      let total = 0;
      let got = 0;
      const unlisten = await listen<{ chunkLength: number; contentLength: number | null }>(
        "update-download-progress",
        ({ payload }) => {
          total = payload.contentLength ?? 0;
          got += payload.chunkLength;
          if (total === 0) setProgress(null);
          else setProgress(Math.min(100, Math.round((got / total) * 100)));
        },
      );
      try {
        await downloadAndInstallUpdate();
      } finally {
        unlisten();
      }
      setProgress(100);
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (err) {
      console.error("[update] 安装失败：", err);
      setStatus("error");
    }
  }, []);

  return { status, version, notes, progress, apply, recheck };
}
