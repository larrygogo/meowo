import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { emit } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useUpdate } from "../useUpdate";

const REPO = "github.com/larrygogo/cc-kanban";
const REPO_URL = "https://github.com/larrygogo/cc-kanban";
const openExt = (url: string) => invoke("open_url", { url }).catch(() => {});

type Section = "general" | "about";

function IconGear() {
  return (
    <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}
function IconInfo() {
  return (
    <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="9" />
      <line x1="12" y1="11" x2="12" y2="16" />
      <line x1="12" y1="8" x2="12" y2="8" />
    </svg>
  );
}

function Switch({ checked, onChange }: { checked: boolean; onChange: () => void }) {
  return (
    <button type="button" role="switch" aria-checked={checked} className={"pswitch" + (checked ? " on" : "")} onClick={onChange}>
      <span className="pswitch-knob" />
    </button>
  );
}

function GeneralSection() {
  const [autostart, setAutostart] = useState(false);
  useEffect(() => {
    invoke<boolean>("get_autostart").then(setAutostart).catch(() => {});
  }, []);
  const toggleAutostart = () => {
    const next = !autostart;
    setAutostart(next);
    invoke("set_autostart", { enabled: next }).catch(() => setAutostart(!next));
  };
  return (
    <>
      <div className="sec-title">通用</div>
      <div className="row-card">
        <div className="row">
          <div className="row-text">
            <div className="row-label">开机自启</div>
            <div className="row-desc">登录系统后自动启动 cc-kanban</div>
          </div>
          <Switch checked={autostart} onChange={toggleAutostart} />
        </div>
      </div>
      <div className="sec-hint">更多设置项陆续补充中…</div>
    </>
  );
}

function AboutSection() {
  const [version, setVersion] = useState("");
  const [triggered, setTriggered] = useState(false);
  const { status, version: newVersion, recheck } = useUpdate();

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  const onAvailable = () => {
    setTriggered(true);
    emit("trigger-update").catch(() => {});
  };
  const updateBtn =
    status === "available"
      ? { label: triggered ? "更新中…" : `更新到 v${newVersion}`, onClick: onAvailable, disabled: triggered, primary: true }
      : { label: status === "checking" ? "检查中…" : "检查更新", onClick: recheck, disabled: status === "checking", primary: false };

  const verSub =
    status === "available" ? `发现新版本 v${newVersion}` : status === "latest" ? "已是最新版本" : `v${version || "—"}`;

  return (
    <>
      <div className="sec-title">关于 cc-kanban</div>

      <div className="row-card">
        <div className="row">
          <div className="row-icon"><div className="pmark"><svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2" strokeLinecap="round"><line x1="7" y1="7" x2="7" y2="17" /><line x1="12" y1="7" x2="12" y2="14" /><line x1="17" y1="7" x2="17" y2="12" /></svg></div></div>
          <div className="row-text">
            <div className="row-label">版本信息</div>
            <div className="row-desc">{verSub}</div>
          </div>
          <button className={"sbtn" + (updateBtn.primary ? " primary" : "")} disabled={updateBtn.disabled} onClick={updateBtn.onClick}>
            {updateBtn.label}
          </button>
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">项目主页</div>
            <div className="row-desc">{REPO}</div>
          </div>
          <button className="sbtn" onClick={() => openExt(REPO_URL)}>
            打开
          </button>
        </div>
      </div>

      <p className="about-blurb">常驻桌面贴纸，实时显示所有 Claude Code 会话的进度。</p>

      <div className="about-foot">
        <a onClick={() => openExt(REPO_URL + "/issues")}>意见反馈</a>
        <span className="dot">·</span>
        <a onClick={() => openExt(REPO_URL + "/releases")}>更新日志</a>
        <div className="copy">MIT License · © 2026 larrygogo</div>
      </div>
    </>
  );
}

export function About() {
  const [sec, setSec] = useState<Section>("general");
  const close = () => getCurrentWindow().close().catch(() => {});

  return (
    <div className="settings">
      <aside className="side">
        <div className="side-top" data-tauri-drag-region />
        <nav className="side-nav">
          <button className={"nav-item" + (sec === "general" ? " on" : "")} onClick={() => setSec("general")}>
            <IconGear />
            <span>通用</span>
          </button>
          <button className={"nav-item" + (sec === "about" ? " on" : "")} onClick={() => setSec("about")}>
            <IconInfo />
            <span>关于</span>
          </button>
        </nav>
      </aside>

      <main className="main">
        <div className="main-bar" data-tauri-drag-region>
          <button className="winclose" title="关闭" onClick={close} aria-label="关闭">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
              <line x1="6" y1="6" x2="18" y2="18" />
              <line x1="18" y1="6" x2="6" y2="18" />
            </svg>
          </button>
        </div>
        <div className="main-body" key={sec}>
          {sec === "general" ? <GeneralSection /> : <AboutSection />}
        </div>
      </main>
    </div>
  );
}
