import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { useUpdate } from "../useUpdate";

const REPO = "github.com/larrygogo/cc-kanban";

export function About() {
  const [version, setVersion] = useState("");
  const { status, version: newVersion, progress, apply } = useUpdate();

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => {});
  }, []);

  const statusText: Record<typeof status, string> = {
    checking: "正在检查更新…",
    latest: "已是最新版本",
    available: `发现新版本 v${newVersion}`,
    downloading: `下载安装中 ${progress}%`,
    error: "检查更新失败",
  };

  const showUpdateBtn = status === "available" || status === "downloading";

  return (
    <div className="about">
      <div className="about-title">cc-kanban</div>
      <div className="about-ver">{version ? `v${version}` : ""}</div>
      <p className="about-desc">常驻桌面贴纸，实时显示所有 Claude Code 会话的进度。</p>
      <div className="about-repo">{REPO}</div>
      {showUpdateBtn && (
        <button
          className="about-btn"
          disabled={status === "downloading"}
          onClick={status === "downloading" ? undefined : apply}
        >
          {status === "downloading" ? `下载中 ${progress}%` : `更新到 v${newVersion}`}
        </button>
      )}
      <div className="about-status">{statusText[status]}</div>
      <div className="about-license">MIT License · © 2026 larrygogo</div>
    </div>
  );
}
