import { useCallback, useEffect, useRef, useState } from "react";
import type { DownloadEvent } from "@tauri-apps/plugin-updater";

type UpdateHandle = {
  version: string;
  body?: string | null;
  downloadAndInstall: (cb?: (e: DownloadEvent) => void) => Promise<void>;
};

export type UpdateStatus = "checking" | "latest" | "available" | "downloading" | "error";

// —— dev 预览钩子：无真实发布时预览「发现新版/更新说明/下载进度」形态 ——
// 任意窗口 devtools 执行 localStorage.setItem("cc-kanban-mock-update", "9.9.9") 后打开更新窗口；
// removeItem 后重新检查即恢复。生产构建 import.meta.env.DEV 恒 false，整块被摇树剔除。
function devMockUpdate(): UpdateHandle | null {
  if (!import.meta.env.DEV) return null;
  let v: string | null = null;
  try {
    v = localStorage.getItem("cc-kanban-mock-update");
  } catch {
    return null;
  }
  if (!v) return null;
  return {
    version: v,
    body: [
      "示例更新说明（dev mock，localStorage 移除 cc-kanban-mock-update 后恢复真实检查）：",
      "",
      "- 贴纸卡片新增右键菜单：星标 / 便签 / 重命名 / 归档 / 打开项目目录",
      "- 新增卡片菜单触发方式设置（右键菜单 / 卡片按钮二选一）",
      "- 重命名与便签行内编辑器视觉重做：✓/✕ 图标钮收进输入框",
      "- 版本更新独立成专门窗口，废除跨窗口 trigger-update 协议",
      "- 修复吸附缩略条圆角外的灰色尖角",
      "- 修复待交互/待审批徽标误转动：改为静态实心圆环",
      "- 修复恢复会话时 DB cwd 失真导致的 No conversation found",
      "- 这一段故意写得很长，用来验证更新说明滚动区在内容超高时的滚动与排版表现。",
    ].join("\n"),
    downloadAndInstall: async (cb) => {
      cb?.({ event: "Started", data: { contentLength: 100 } } as DownloadEvent);
      for (let i = 0; i < 20; i++) {
        await new Promise((r) => setTimeout(r, 120));
        cb?.({ event: "Progress", data: { chunkLength: 5 } } as DownloadEvent);
      }
      cb?.({ event: "Finished", data: {} } as unknown as DownloadEvent);
      await new Promise(() => {}); // 永不 resolve：绝不能在 dev 里走到真实 relaunch
    },
  };
}

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
  const handleRef = useRef<UpdateHandle | null>(null);

  // 返回本次检查的结果状态（调用方拿结果不能依赖异步 state）。
  const recheck = useCallback(async (): Promise<UpdateStatus> => {
    setStatus("checking");
    try {
      const up = devMockUpdate() ?? (await (await import("@tauri-apps/plugin-updater")).check());
      if (up) {
        handleRef.current = up as unknown as UpdateHandle;
        setVersion(up.version);
        setNotes(up.body?.trim() ? up.body : null);
        setStatus("available");
        return "available";
      }
      setStatus("latest");
      return "latest";
    } catch {
      setStatus("error");
      return "error";
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
    }
  }, []);

  return { status, version, notes, progress, apply, recheck };
}
