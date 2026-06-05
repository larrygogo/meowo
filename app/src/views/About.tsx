import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { emit } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useUpdate } from "../useUpdate";

const REPO = "github.com/larrygogo/cc-kanban";

export function About() {
  const [version, setVersion] = useState("");
  const [triggered, setTriggered] = useState(false);
  const [autostart, setAutostart] = useState(false);
  // 设置窗口只用于「显示状态」；检查不写托盘、安装委托主窗，避免双窗口竞态。
  const { status, version: newVersion, recheck } = useUpdate();

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => {});
    invoke<boolean>("get_autostart")
      .then(setAutostart)
      .catch(() => {});
  }, []);

  // 乐观切换：失败回滚。
  const toggleAutostart = () => {
    const next = !autostart;
    setAutostart(next);
    invoke("set_autostart", { enabled: next }).catch(() => setAutostart(!next));
  };

  const statusText: Record<typeof status, string> = {
    checking: "正在检查更新…",
    latest: "已是最新版本",
    available: triggered ? "已在主窗口开始更新…" : `发现新版本 v${newVersion}`,
    downloading: "更新中…",
    error: "检查更新失败，可重试",
  };

  // 有新版 → 交给主窗安装；其余 → 常驻「检查更新」可手动重试。
  const onAvailable = () => {
    setTriggered(true);
    emit("trigger-update").catch(() => {});
  };
  const btn =
    status === "available"
      ? { label: `更新到 v${newVersion}`, onClick: onAvailable, disabled: triggered }
      : { label: "检查更新", onClick: recheck, disabled: status === "checking" };

  return (
    <div className="about">
      <div className="about-title">cc-kanban</div>
      <div className="about-ver">{version ? `v${version}` : ""}</div>
      <p className="about-desc">常驻桌面贴纸，实时显示所有 Claude Code 会话的进度。</p>

      <label className="set-row">
        <span>开机自启</span>
        <input type="checkbox" checked={autostart} onChange={toggleAutostart} />
      </label>

      <button className="about-btn" disabled={btn.disabled} onClick={btn.onClick}>
        {btn.label}
      </button>
      <div className="about-status">{statusText[status]}</div>
      <div className="about-repo">{REPO}</div>
      <div className="about-license">MIT License · © 2026 larrygogo</div>
    </div>
  );
}
