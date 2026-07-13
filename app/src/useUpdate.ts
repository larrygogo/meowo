import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { checkUpdate, downloadUpdate, getSettings, installDownloadedUpdate, type Settings } from "./api";

export type UpdateStatus = "checking" | "latest" | "available" | "downloading" | "ready" | "error";

/// 检查更新；对外暴露状态/版本/更新说明/进度，以及下载、安装和手动重检操作。
/// 非 Tauri 环境（测试/浏览器）或网络失败一律降级为 error，不抛错。
/// automatic=true 时服从设置里的自动更新开关，延迟检查并后台下载；安装始终由更新窗口确认。
export function useUpdate(options: { automatic?: boolean; delayMs?: number } = {}) {
  const { automatic = false, delayMs = 10_000 } = options;
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
        setStatus(up.downloadState);
        return up.downloadState;
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

  const download = useCallback(async () => {
    if (!checkedRef.current) return;
    setStatus("downloading");
    setProgress(0);
    try {
      setStatus(await downloadUpdate());
    } catch (err) {
      console.error("[update] 下载失败：", err);
      setStatus("error");
    }
  }, []);

  const install = useCallback(async () => {
    try {
      await installDownloadedUpdate();
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (err) {
      console.error("[update] 安装失败：", err);
      setStatus("error");
    }
  }, []);

  // 更新下载是后端进程级共享任务；每个窗口都订阅同一组事件，晚打开的更新窗口也能接续状态。
  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    void listen<{ downloaded: number; contentLength: number | null }>(
      "update-download-progress",
      ({ payload }) => {
        setStatus("downloading");
        const total = payload.contentLength ?? 0;
        setProgress(total === 0 ? null : Math.min(100, Math.round((payload.downloaded / total) * 100)));
      },
    ).then((fn) => unlisteners.push(fn)).catch(() => {});
    void listen("update-download-finished", () => {
      setProgress(100);
      setStatus("ready");
    }).then((fn) => unlisteners.push(fn)).catch(() => {});
    void listen("update-download-failed", () => {
      setStatus("error");
    }).then((fn) => unlisteners.push(fn)).catch(() => {});
    return () => unlisteners.forEach((fn) => fn());
  }, []);

  // 更新窗口/设置页沿用即时手动检查；主窗则服从“自动更新”设置并延迟启动网络请求。
  useEffect(() => {
    if (automatic) return;
    void recheck();
  }, [automatic, recheck]);

  useEffect(() => {
    if (!automatic) return;
    let disposed = false;
    let timer: number | undefined;
    let interval: number | undefined;
    let enabled: boolean | undefined;
    let unlisten: (() => void) | undefined;
    const run = () => {
      void recheck().then((next) => {
        if (!disposed && next === "available") void download();
      });
    };
    const applySettings = (settings: Settings) => {
      if (enabled === settings.auto_update_enabled) return;
      enabled = settings.auto_update_enabled;
      if (timer != null) window.clearTimeout(timer);
      if (interval != null) window.clearInterval(interval);
      if (!enabled) {
        setStatus("latest");
        return;
      }
      timer = window.setTimeout(() => {
        run();
        interval = window.setInterval(run, 12 * 60 * 60 * 1000);
      }, delayMs);
    };
    void getSettings().then((settings) => {
      if (!disposed) applySettings(settings);
    }).catch(() => setStatus("error"));
    void listen<Settings>("settings-changed", ({ payload }) => applySettings(payload))
      .then((fn) => { unlisten = fn; })
      .catch(() => {});
    return () => {
      disposed = true;
      if (timer != null) window.clearTimeout(timer);
      if (interval != null) window.clearInterval(interval);
      unlisten?.();
    };
  }, [automatic, delayMs, download, recheck]);

  return { status, version, notes, progress, download, install, recheck };
}
