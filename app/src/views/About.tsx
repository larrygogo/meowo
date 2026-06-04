import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";

const REPO = "github.com/larrygogo/cc-kanban";

export function About() {
  const [version, setVersion] = useState("");
  const [status, setStatus] = useState("");

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => {});
  }, []);

  const checkUpdate = async () => {
    setStatus("检查中…");
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const up = await check();
      if (up) {
        setStatus(`发现新版本 v${up.version}，正在下载安装…`);
        await up.downloadAndInstall();
        const { relaunch } = await import("@tauri-apps/plugin-process");
        await relaunch();
      } else {
        setStatus("已是最新版本");
      }
    } catch (err) {
      console.error("[about] 检查更新失败：", err);
      setStatus("检查更新失败");
    }
  };

  return (
    <div className="about">
      <div className="about-title">cc-kanban</div>
      <div className="about-ver">{version ? `v${version}` : ""}</div>
      <p className="about-desc">常驻桌面贴纸，实时显示所有 Claude Code 会话的进度。</p>
      <div className="about-repo">{REPO}</div>
      <button className="about-btn" onClick={checkUpdate}>检查更新</button>
      <div className="about-status">{status}</div>
      <div className="about-license">MIT License · © 2026 larrygogo</div>
    </div>
  );
}
