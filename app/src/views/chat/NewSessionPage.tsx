import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { type AgentId, type LiveSession, agentName, getSessionAdminPage, listSubdirectories, savePastedAttachment } from "../../api";
import { formatBackendError } from "../../i18n/errors";
import { type Attachment, attachmentOf, base64OfBuffer } from "./drafts";
import { ImageRef } from "./Message";
import { fmtShortAgo } from "../sticker/helpers";
import { agentAssets, tintStyle } from "../../providers";
import { normalizePath, pathKey } from "../../paths";
import { PillSelect } from "./PillSelect";
import { AgentAppIcon } from "./AgentAppIcon";

function ShieldIcon() {
  return <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" /></svg>;
}
import { useT } from "../../i18n";
import { useNewSessionForm } from "../useNewSessionForm";
import { WorkspacePicker } from "./WorkspacePicker";

/// 对话窗里的新建会话页（对照 Kimi Code：新建会话不弹窗，主区就地换成一个输入框，
/// 上方是工作区选择，下面一排 Agent / 启动选项的小下拉，写下第一句话发出去就开聊）。
/// 表单逻辑与独立窗口面板共用 useNewSessionForm，这里只管长相与「第一句话」。

/** 「最近会话」列几条（对照 Kimi 新建页输入框下的最近会话，更多去会话管理页）。 */
const RECENT_LIMIT = 5;

export function NewSessionPage({ prefill, droppedPath, onLaunched, onOpenSession, onMoreSessions }: {
  prefill: { cwd?: string | null; provider?: string | null };
  /** 点「最近会话」里的一条：直接回到那条会话接着聊。 */
  onOpenSession?: (sessionId: number) => void;
  /** 「查看更多」：去会话管理页，预选当前工作区。 */
  onMoreSessions?: (cwd: string) => void;
  /** 拖进窗口的路径（nonce 区分同一路径的两次拖入）。是文件夹就设为工作区。 */
  droppedPath?: { path: string; nonce: number } | null;
  /** 启动成功：临时负 id、用的 agent、第一句话（可为空）及粘进来的附件。 */
  onLaunched: (tempId: number, provider: AgentId, message: string, attachments: Attachment[]) => void;
}) {
  const t = useT();
  const form = useNewSessionForm(normalizePath(prefill.cwd ?? ""), (prefill.provider ?? null) as AgentId | null);
  const { cwd, setCwd, provider, setProvider, recent, hooks, busy, repairing, error, setError, agents, avail, opts, setOpts,
    activeProfiles, relayPins, loginOperations, swappedFrom, launchOptions, warn, needLogin } = form;
  const [message, setMessage] = useState("");
  // 粘进来的截图/文件（用户反馈：新建页粘不了图，只能等会话起来再贴一次）。和对话页同一套
  // 落盘与附件形状，发出去后交给对话页的发送通道，原生贴图 / 附路径降级都在那边。
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  // 还在落盘的粘贴数：大截图写盘要一会儿，「Ctrl+V 紧接回车」会在图回来之前就把会话发出去，
  // 图静默丢掉（做空复现）。落盘期间先不让发。
  const [pasting, setPasting] = useState(0);
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

  // 这个工作区最近的几条会话（对照 Kimi）：想做的事往往接着某条旧会话做更合适，
  // 新建前顺手给出来，免得同一件事开出好几条会话。换工作区跟着换。
  const [recentSessions, setRecentSessions] = useState<{ cwd: string; items: LiveSession[]; total: number } | null>(null);
  useEffect(() => {
    const dir = cwd.trim();
    if (!dir || !onOpenSession) { setRecentSessions(null); return; }
    let alive = true;
    // 输入时每个字都会改 cwd：稍等一下再查，选目录 / 点最近项是一次到位的。
    const timer = window.setTimeout(() => {
      getSessionAdminPage({ workspaces: [dir], status: "all", updatedBeforeDays: null, page: 0, pageSize: RECENT_LIMIT })
        .then((page) => { if (alive) setRecentSessions({ cwd: dir, items: page.items, total: page.total }); })
        .catch(() => { if (alive) setRecentSessions(null); });
    }, 200);
    return () => { alive = false; window.clearTimeout(timer); };
  }, [cwd, onOpenSession]);

  async function pickFolder() {
    const picked = await open({ directory: true });
    if (typeof picked === "string") { setCwd(normalizePath(picked)); setError(null); }
  }

  async function submit() {
    if (pasting > 0) return;
    if (!cwd.trim()) { setError(t.newSession.workspaceRequired); return; }
    const launched = await form.launch();
    // 旧后端桌面命令不回临时 id（null）：会话照样起来、reveal 照样切过去，只是第一句话
    // 没法记到它身上——放回输入框不现实（页面随即关掉），宁可不清空让用户看见。
    if (launched?.tempId != null) onLaunched(launched.tempId, provider, message.trim(), attachments);
  }

  const pasteFiles = async (files: File[]) => {
    setPasting((count) => count + 1);
    const results = await Promise.allSettled(files.map(async (file) => {
      const path = await savePastedAttachment(file.name || "pasted.png", base64OfBuffer(await file.arrayBuffer()));
      return { ...attachmentOf(path), size: file.size };
    }));
    setPasting((count) => count - 1);
    const fresh = results.flatMap((entry) => (entry.status === "fulfilled" ? [entry.value] : []));
    const failed = results.find((entry): entry is PromiseRejectedResult => entry.status === "rejected");
    if (failed) setError(formatBackendError(failed.reason, t.locale));
    // 上限与对话页一致（12）：多出来的在那边也发不出去。
    setAttachments((current) => [...current, ...fresh].slice(0, 12));
  };

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
        {/* 头部标识是「要起哪家 agent」：Meowo 托管多家 CLI，图标跟着下面的 Agent 下拉变（与空会话
            欢迎面板同一套标识）。名字不再写一遍，下拉里已经有了。Kimi 在这放品牌字标，它只有一家。 */}
        <div className="nsp-hero">
          {/* 与设置页 agent 卡片同一套应用图标外框（provider-card-icon）：自带方块的（codex /
              opencode / kimi）铺满，裸 logomark（claude / gemini）留一圈边。此前一律套一块着色
              方底，codex 的黑方块被套成方中方、几家大小都不一样（用户实拍）。 */}
          <AgentAppIcon provider={provider} className="nsp-hero-mark" tiled />
          <h1 className="nsp-hero-title">{t.newSession.title}</h1>
        </div>
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
            onPaste={(event) => {
              const files = Array.from(event.clipboardData?.files ?? []);
              if (files.length === 0) return; // 纯文本粘贴照常走默认行为
              // 有文件就整次接管：默认行为会把文件名当正文粘进输入框。
              event.preventDefault();
              void pasteFiles(files);
            }}
          />
          {attachments.length > 0 && (
            <div className="chat-attachments nsp-attachments" data-testid="nsp-attachments">
              {attachments.map((file) => (
                <div className={"chat-attachment" + (file.image ? " is-image" : "")} key={file.path} data-tip={file.path}>
                  {file.image ? <ImageRef path={file.path} /> : <span className="chat-file-name">{file.name}</span>}
                  <button type="button" aria-label={`${t.chat.removeAttachment} ${file.name}`} disabled={busy}
                    onClick={() => setAttachments((items) => items.filter((item) => item.path !== file.path))}>×</button>
                </div>
              ))}
            </div>
          )}
          <div className="nsp-bar">
            <div className="nsp-chips">
              {/* 底部这排与对话页 composer 的权限 / 模型胶囊同一套（PillSelect）：短名上胶囊、
                  完整名 + 灰色说明进菜单，高风险档才标红。 */}
              {avail && avail.length > 0 && (
                <PillSelect
                  testId="nsp-agent"
                  variant="model"
                  label={t.newSession.agent}
                  value={provider}
                  options={avail.map((id) => {
                    const { Icon } = agentAssets(id);
                    return { value: id, label: agentName(agents ?? [], id), icon: <span className="nsp-agent-mark" style={tintStyle(id)}><Icon /></span> };
                  })}
                  onChange={(value) => { setProvider(value); setError(null); }}
                />
              )}
              {launchOptions.map((option) => {
                // relay 启用时 model 档由中转固定：置灰，悬停说明固定成了什么。
                const pinned = option.id === "model" ? relayPins[provider] : undefined;
                const optionLabel = t.newSession.launchOption[option.id] ?? option.id;
                // 权限一类（权限 / 审批）配盾牌图标、胶囊用对话页的短名；「默认」带上维度名，
                // 模型的默认档没有图标可认，写成「默认模型」。
                const guard = option.id === "permission" || option.id === "approval";
                return (
                  <PillSelect
                    key={option.id}
                    testId={"nsp-option-" + option.id}
                    variant={guard ? "mode" : "model"}
                    icon={guard ? <ShieldIcon /> : undefined}
                    label={optionLabel}
                    tip={pinned !== undefined ? t.newSession.relayPinnedModel(pinned) : optionLabel}
                    disabled={pinned !== undefined}
                    value={opts[option.id] ?? option.default}
                    options={option.choices.map((choice) => {
                      const name = t.newSession.launchChoice[`${option.id}.${choice.id}`] ?? choice.label;
                      return {
                        value: choice.id,
                        label: name,
                        short: t.chat.modeShortNames[`${option.id}.${choice.id}`]
                          ?? (choice.id === "default" && !guard ? t.newSession.defaultOf(optionLabel) : name),
                        risk: choice.risk,
                        desc: t.newSession.launchChoiceDesc[`${option.id}.${choice.id}`]
                          ?? (choice.risk ? t.newSession.riskyChoiceSub : undefined),
                      };
                    })}
                    onChange={(value) => setOpts((current) => ({ ...current, [option.id]: value }))}
                  />
                );
              })}
            </div>
            <button
              type="button"
              className="nsp-send"
              data-testid="nsp-send"
              aria-label={busy ? t.newSession.launching : t.newSession.start}
              data-tip={busy ? t.newSession.launching : t.newSession.start}
              disabled={busy || pasting > 0 || (avail?.length ?? 0) === 0}
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

        {recentSessions && recentSessions.items.length > 0 && onOpenSession && (
          <section className="nsp-recent" aria-label={t.newSession.recentSessions} data-testid="nsp-recent">
            <div className="nsp-recent-head">
              <span>{t.newSession.recentSessions}</span>
              {onMoreSessions && recentSessions.total > recentSessions.items.length && (
                <button type="button" className="nsp-recent-more" data-tip={t.newSession.moreSessionsTip} onClick={() => onMoreSessions(recentSessions.cwd)}>
                  {t.newSession.moreSessions}
                </button>
              )}
            </div>
            {recentSessions.items.map((session) => {
              const { Icon } = agentAssets(session.provider);
              return (
                <button type="button" key={session.session.id} className="nsp-recent-row" onClick={() => onOpenSession(session.session.id)}>
                  <span className="nsp-recent-agent" style={tintStyle(session.provider)} aria-hidden="true"><Icon /></span>
                  <span className="nsp-recent-title">{session.task_title || t.sticker.waitingFirstInput}</span>
                  <span className="nsp-recent-time">{fmtShortAgo(session.session.last_event_at, t)}</span>
                </button>
              );
            })}
          </section>
        )}
      </div>
    </div>
  );
}
