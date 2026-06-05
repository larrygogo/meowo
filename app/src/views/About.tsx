import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { emit } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useUpdate } from "../useUpdate";

const REPO = "github.com/larrygogo/cc-kanban";

type Pane = "settings" | "about";

// app 标识：珊瑚渐变圆角方块 + 看板三列字形。
function AppMark() {
  return (
    <div className="pmark" aria-hidden>
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2" strokeLinecap="round">
        <line x1="7" y1="7" x2="7" y2="17" />
        <line x1="12" y1="7" x2="12" y2="14" />
        <line x1="17" y1="7" x2="17" y2="12" />
      </svg>
    </div>
  );
}

function Switch({ checked, onChange }: { checked: boolean; onChange: () => void }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      className={"pswitch" + (checked ? " on" : "")}
      onClick={onChange}
    >
      <span className="pswitch-knob" />
    </button>
  );
}

function SettingsPane() {
  const [autostart, setAutostart] = useState(false);

  useEffect(() => {
    invoke<boolean>("get_autostart").then(setAutostart).catch(() => {});
  }, []);

  const toggleAutostart = () => {
    const next = !autostart;
    setAutostart(next); // 乐观更新，失败回滚
    invoke("set_autostart", { enabled: next }).catch(() => setAutostart(!next));
  };

  return (
    <div className="ppane">
      <div className="pgroup-label">通用</div>
      <div className="pcard">
        <div className="pitem">
          <div className="pitem-text">
            <div className="pitem-label">开机自启</div>
            <div className="pitem-desc">登录系统后自动启动 cc-kanban</div>
          </div>
          <Switch checked={autostart} onChange={toggleAutostart} />
        </div>
      </div>
      <div className="phint">更多设置项陆续补充中…</div>
    </div>
  );
}

function AboutPane() {
  const [version, setVersion] = useState("");
  const [triggered, setTriggered] = useState(false);
  const { status, version: newVersion, recheck } = useUpdate();

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

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
    <div className="ppane pabout">
      <AppMark />
      <div className="pabout-name">cc-kanban</div>
      <div className="pabout-ver">{version ? `v${version}` : ""}</div>
      <p className="pabout-desc">常驻桌面贴纸，实时显示所有 Claude Code 会话的进度。</p>

      <button className="pbtn" disabled={btn.disabled} onClick={btn.onClick}>
        {btn.label}
      </button>
      <div className="pabout-status">{statusText[status]}</div>

      <div className="pabout-foot">
        <span className="pabout-repo">{REPO}</span>
        <span className="pabout-dot">·</span>
        <span>MIT License</span>
        <span className="pabout-dot">·</span>
        <span>© 2026 larrygogo</span>
      </div>
    </div>
  );
}

export function About() {
  const [pane, setPane] = useState<Pane>("settings");
  return (
    <div className="panel">
      <div className="panel-glow" />
      <nav className="panel-tabs">
        <button className={pane === "settings" ? "on" : ""} onClick={() => setPane("settings")}>
          设置
        </button>
        <button className={pane === "about" ? "on" : ""} onClick={() => setPane("about")}>
          关于
        </button>
      </nav>
      <div className="panel-body" key={pane}>
        {pane === "settings" ? <SettingsPane /> : <AboutPane />}
      </div>
    </div>
  );
}
