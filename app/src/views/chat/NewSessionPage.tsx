import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { type AgentId, agentName, listSubdirectories } from "../../api";
import { agentAssets, tintStyle } from "../../providers";
import { normalizePath, pathKey } from "../../paths";
import { Dropdown } from "../menu";
import { useT } from "../../i18n";
import { useNewSessionForm } from "../useNewSessionForm";
import { WorkspacePicker } from "./WorkspacePicker";

/// 对话窗里的新建会话页（对照 Kimi Code：新建会话不弹窗，主区就地换成一个输入框，
/// 上方是工作区选择，下面一排 Agent / 启动选项的小下拉，写下第一句话发出去就开聊）。
/// 表单逻辑与独立窗口面板共用 useNewSessionForm，这里只管长相与「第一句话」。

export function NewSessionPage({ prefill, droppedPath, onLaunched }: {
  prefill: { cwd?: string | null; provider?: string | null };
  /** 拖进窗口的路径（nonce 区分同一路径的两次拖入）。是文件夹就设为工作区。 */
  droppedPath?: { path: string; nonce: number } | null;
  /** 启动成功：临时负 id、用的 agent、第一句话（可为空）。 */
  onLaunched: (tempId: number, provider: AgentId, message: string) => void;
}) {
  const t = useT();
  const form = useNewSessionForm(normalizePath(prefill.cwd ?? ""), (prefill.provider ?? null) as AgentId | null);
  const { cwd, setCwd, provider, setProvider, recent, hooks, busy, repairing, error, setError, agents, avail, opts, setOpts,
    activeProfiles, relayPins, loginOperations, swappedFrom, launchOptions, warn, needLogin } = form;
  const [message, setMessage] = useState("");
  const textRef = useRef<HTMLTextAreaElement>(null);
  // 打开即可打字：九成场景目录已按最近一次预填好，第一步就是写第一句话。
  useEffect(() => textRef.current?.focus(), []);

  useEffect(() => {
    if (!droppedPath) return;
    let cancelled = false;
    // 拖拽只给路径不说类型：能列子目录的才是文件夹（拖进来的文件会被它拒掉）。
    listSubdirectories(droppedPath.path)
      .then(() => { if (!cancelled) { setCwd(normalizePath(droppedPath.path)); setError(null); } })
      .catch(() => { if (!cancelled) setError(t.newSession.dropNotFolder); });
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 只由一次新的拖入驱动
  }, [droppedPath?.nonce]);

  async function pickFolder() {
    const picked = await open({ directory: true });
    if (typeof picked === "string") { setCwd(normalizePath(picked)); setError(null); }
  }

  async function submit() {
    if (!cwd.trim()) { setError(t.newSession.workspaceRequired); return; }
    const launched = await form.launch();
    // 旧后端桌面命令不回临时 id（null）：会话照样起来、reveal 照样切过去，只是第一句话
    // 没法记到它身上——放回输入框不现实（页面随即关掉），宁可不清空让用户看见。
    if (launched?.tempId != null) onLaunched(launched.tempId, provider, message.trim());
  }

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    // IME 合成中的回车是选候选词，不是发送。
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      void submit();
    }
  };

  // 预填的目录（卡片菜单「在此工作区新建」）可能不在最近列表里，补在最前面。
  const dirs = cwd && !recent.some((dir) => pathKey(dir) === pathKey(cwd)) ? [cwd, ...recent] : recent;

  return (
    <div className="nsp" data-testid="new-session-page">
      <div className="nsp-drag" data-tauri-drag-region />
      <div className="nsp-center">
        <h1 className="nsp-title">{t.newSession.title}</h1>
        {/* 工作区在输入框上方单列（对照 Kimi）：它决定会话在哪跑，分量比底部那排选项重。 */}
        <WorkspacePicker
          cwd={cwd}
          dirs={dirs}
          onPick={(dir) => { setCwd(dir); setError(null); }}
          onPickFolder={() => void pickFolder()}
        />
        <div className={"nsp-composer" + (busy ? " is-busy" : "")}>
          <textarea
            ref={textRef}
            className="nsp-input"
            data-testid="nsp-input"
            rows={3}
            value={message}
            placeholder={t.newSession.pagePlaceholder}
            disabled={busy}
            onChange={(event) => setMessage(event.target.value)}
            onKeyDown={onKeyDown}
          />
          <div className="nsp-bar">
            <div className="nsp-chips">
              {avail && avail.length > 0 && (
                <span className="nsp-chip" data-testid="nsp-agent">
                  <Dropdown
                    align="left"
                    value={provider}
                    options={avail.map((id) => {
                      const { Icon } = agentAssets(id);
                      return { value: id, label: agentName(agents ?? [], id), icon: <span className="nsp-agent-mark" style={tintStyle(id)}><Icon /></span> };
                    })}
                    onChange={(value) => { setProvider(value); setError(null); }}
                  />
                </span>
              )}
              {launchOptions.map((option) => {
                // relay 启用时 model 档由中转固定：置灰，悬停说明固定成了什么。
                const pinned = option.id === "model" ? relayPins[provider] : undefined;
                const optionLabel = t.newSession.launchOption[option.id] ?? option.id;
                return (
                  <span key={option.id} className="nsp-chip" data-tip={pinned !== undefined ? t.newSession.relayPinnedModel(pinned) : optionLabel} data-testid={"nsp-option-" + option.id}>
                    <Dropdown
                      align="left"
                      disabled={pinned !== undefined}
                      value={opts[option.id] ?? option.default}
                      options={option.choices.map((choice) => ({
                        value: choice.id,
                        label: choice.id === "default"
                          ? t.newSession.defaultOf(optionLabel)
                          : t.newSession.launchChoice[`${option.id}.${choice.id}`] ?? choice.label,
                        risky: choice.risk,
                        sub: t.newSession.launchChoiceDesc[`${option.id}.${choice.id}`]
                          ?? (choice.risk ? t.newSession.riskyChoiceSub : undefined),
                      }))}
                      onChange={(value) => setOpts((current) => ({ ...current, [option.id]: value }))}
                    />
                  </span>
                );
              })}
            </div>
            <button
              type="button"
              className="nsp-send"
              data-testid="nsp-send"
              aria-label={busy ? t.newSession.launching : t.newSession.start}
              data-tip={busy ? t.newSession.launching : t.newSession.start}
              disabled={busy || (avail?.length ?? 0) === 0}
              onClick={() => void submit()}
            >
              {busy ? <span className="nsp-spin" aria-hidden="true" /> : (
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <path d="M12 19V5M5 12l7-7 7 7" />
                </svg>
              )}
            </button>
          </div>
        </div>

        <div className="nsp-notes">
          {avail === null && <div className="nsp-note">{t.newSession.detectingAgents}</div>}
          {avail?.length === 0 && (
            <div className="nsp-note is-warn" data-testid="nsp-no-agents">
              <span>{t.newSession.noAgents}</span>
              <button type="button" className="nsp-note-btn" onClick={() => void invoke("open_settings").catch(() => {})}>{t.newSession.goInstall}</button>
            </div>
          )}
          {swappedFrom && avail && avail.length > 0 && (
            <div className="nsp-note">{t.newSession.agentSwapped(agentName(agents ?? [], swappedFrom), agentName(agents ?? [], provider))}</div>
          )}
          {avail && avail.length > 0 && warn && (
            <div className="nsp-note is-warn">
              <span>{hooks[provider] === "unknown" ? t.newSession.hooksUnknown : t.newSession.hooksMissing}</span>
              <button type="button" className="nsp-note-btn" onClick={() => void form.repairHooks()} disabled={repairing}>
                {repairing ? t.newSession.repairingHooks : t.newSession.repairHooks}
              </button>
            </div>
          )}
          {avail && avail.length > 0 && needLogin && (
            <div className="nsp-note is-warn">
              <span>{loginOperations.isPending(provider) ? t.newSession.loggingIn : t.newSession.notLoggedIn}</span>
              <button type="button" className="nsp-note-btn" onClick={() => void (loginOperations.isPending(provider) ? form.cancelLoginWait() : form.doLogin())}>
                {loginOperations.isPending(provider) ? t.newSession.cancelLogin : t.newSession.login}
              </button>
            </div>
          )}
          {activeProfiles[provider] && <div className="nsp-note">{t.newSession.activeProfile(activeProfiles[provider]!)}</div>}
          {error && <div className="nsp-note is-error" role="alert" data-testid="nsp-error">{error}</div>}
        </div>
      </div>
    </div>
  );
}
