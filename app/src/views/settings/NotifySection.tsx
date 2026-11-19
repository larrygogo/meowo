// 设置 › 通知（设置页精简 2026-11-19）：此前同样四个事件在「通用 · 通知这些事」和「外观 · 自动弹出提醒」
// 各列一遍，手机推送又在「手机远程」，用户要关一个提醒得先猜它在哪页。现在集中到这一页：
// 桌面通知、提示音、任务栏提醒；四个事件只列一次，每行两列开关（系统通知 / 灵动岛弹出，对照 Slack
// 通知设置的「事件 × 渠道」矩阵）；最后是手机推送（Bark）。
//
// 存储字段一个没改：系统通知列读写 notify_kinds，灵动岛列读写 island_alerts，两列各存各的。
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { testBarkPush } from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { SETTINGS_DEFAULTS, useSettingsState } from "./state";
import { Switch, SettingsError, SavedNote, useSavedFlash } from "./widgets";

// 与 About.tsx 同一判定：WKWebView 的 UA 含 "Mac"、WebView2 含 "Win"。
const IS_MAC = typeof navigator !== "undefined" && /Mac/i.test(navigator.userAgent);
const IS_WIN = typeof navigator !== "undefined" && /Win/i.test(navigator.userAgent);

export const ALERT_KINDS = ["waiting", "approval", "question", "error"] as const;

/// 手机推送（Bark）一行：地址草稿失焦/回车才落盘，旁边「发一条测试」直接用草稿测——填完先测通
/// 再走开，不用等保存。远程访问关着也能用：推送只要电脑能连上 Bark（所以从「手机远程」搬来通知页）。
/// 填了地址后右侧多一个开关：手机设置面板能暂停推送，电脑这边得看得见、拨得回来。
export function BarkPushRow({ value, paused, onCommit, onTogglePaused }: {
  value: string;
  paused: boolean;
  onCommit: (url: string) => Promise<string | null>;
  onTogglePaused: () => void;
}) {
  const t = useT();
  const [text, setText] = useState(value);
  const [status, setStatus] = useState<{ kind: "testing" | "ok" } | { kind: "error"; message: string } | null>(null);
  const saved = useSavedFlash();
  useEffect(() => { setText(value); }, [value]);
  const commit = () => {
    const next = text.trim();
    if (next !== value) void onCommit(next).then((err) => { if (!err) saved.flash(); });
  };
  const test = () => {
    if (!text.trim() || status?.kind === "testing") return;
    setStatus({ kind: "testing" });
    testBarkPush(text.trim())
      .then(() => setStatus({ kind: "ok" }))
      .catch((error) => setStatus({ kind: "error", message: formatBackendError(error, t.locale) }));
  };
  return (
    <div className="row remote-push-row">
      <div className="row-text">
        <div className="row-label">{t.remote.pushLabel}</div>
        <div className="row-desc">{t.remote.pushDesc}</div>
        <div className="remote-url-row">
          <input
            className="ns-input remote-push-input"
            type="url"
            autoComplete="off"
            spellCheck={false}
            placeholder={t.remote.pushPlaceholder}
            aria-label={t.remote.pushLabel}
            value={text}
            onChange={(e) => { setText(e.target.value); setStatus(null); saved.hide(); }}
            onBlur={commit}
            // T-5：回车提交、焦点留在框里。
            onKeyDown={(e) => { if (e.key === "Enter") commit(); }}
          />
          <SavedNote shown={saved.shown} fx={saved.fx} />
          {/* 测试进行中不用 disabled：焦点就在这颗钮上，disabled 会把焦点踢到 body（第三轮焦点准则）。
              test() 自己拦重复点击。 */}
          <button type="button" className="remote-copy" disabled={!text.trim()} aria-disabled={status?.kind === "testing" || undefined} onClick={test}>
            {status?.kind === "testing" ? t.remote.pushTesting : t.remote.pushTest}
          </button>
        </div>
        {status?.kind === "ok" && <div className="row-desc" role="status">{t.remote.pushTestOk}</div>}
        {status?.kind === "error" && <div className="proxy-err" role="alert">{status.message}</div>}
        {/* 内容出本机的事实写在旁边：项目名、会话标题、在等什么都会经过 Bark 服务器。 */}
        {text.trim() && <div className="row-desc">{t.remote.pushPrivacy}</div>}
      </div>
      {value.trim() && <Switch checked={!paused} onChange={onTogglePaused} label={t.remote.pushToggle} />}
    </div>
  );
}

export function NotifySection() {
  const t = useT();
  const [settings, patch, patchError, clearPatchError] = useSettingsState();
  const notifyOn = settings?.notifications_enabled ?? true;
  const soundOn = settings?.notification_sound ?? true;
  const flashOn = settings?.attention_flash_enabled ?? true;
  // 旧设置文件缺字段时按默认（四类全开）显示，与后端 serde 默认一致。
  const notifyKinds = settings?.notify_kinds ?? SETTINGS_DEFAULTS.notify_kinds;
  const islandAlerts = settings?.island_alerts ?? SETTINGS_DEFAULTS.island_alerts;
  // 灵动岛只在 Windows / macOS 上有（Linux 没有岛），没有岛就不给那一列。
  const hasIsland = IS_WIN || IS_MAC;
  // macOS 系统层通知授权：被用户在系统设置拒绝后通知静默不弹，而本开关照常可开——
  // 挂载时查一次，denied 时在开关旁给「打开系统设置」直达（T-6 permission_denied 同款范式）。
  const [sysNotifyDenied, setSysNotifyDenied] = useState(false);
  useEffect(() => {
    if (!IS_MAC) return;
    invoke<string>("notification_authorization_status")
      .then((s) => setSysNotifyDenied(s === "denied"))
      .catch(() => {});
  }, []);
  const toggleIn = (list: readonly string[], kind: string) =>
    list.includes(kind) ? list.filter((k) => k !== kind) : [...list, kind];

  return (
    <>
      <div className="row-card">
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.notify}</div>
            <div className="row-desc">{t.settings.notifyDesc}</div>
          </div>
          <Switch checked={notifyOn} onChange={() => patch({ notifications_enabled: !notifyOn })} label={t.settings.notify} />
        </div>
        {/* 系统层授权被拒时，应用内开关再开也是静默——必须在此处可见并给出直达入口。 */}
        {IS_MAC && sysNotifyDenied && (
          <div className="row">
            <div className="row-text">
              <div className="row-desc">{t.settings.notifySystemDenied}</div>
            </div>
            <button type="button" className="sbtn" onClick={() => { invoke("open_notification_settings").catch(() => {}); }}>
              {t.settings.openSystemSettings}
            </button>
          </div>
        )}
        {/* 提示音只跟系统通知走：桌面通知关着时置灰而不是藏起来，免得用户以为这项没了。 */}
        <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.notifySound}</div>
            <div className="row-desc">{t.settings.notifySoundDesc}</div>
          </div>
          <Switch checked={soundOn} disabled={!notifyOn} onChange={() => patch({ notification_sound: !soundOn })} label={t.settings.notifySound} />
        </div>
        {/* 任务栏闪烁仅 Windows 有效（macOS 由菜单栏徽章承担同一职责），别的平台不显示无效开关。 */}
        {IS_WIN && <div className="row">
          <div className="row-text">
            <div className="row-label">{t.settings.attentionFlash}</div>
            <div className="row-desc">{t.settings.attentionFlashDesc}</div>
          </div>
          <Switch checked={flashOn} onChange={() => patch({ attention_flash_enabled: !flashOn })} label={t.settings.attentionFlash} />
        </div>}
      </div>

      <div className="sec-caption">{t.settings.notifyEventsGroup}</div>
      {/* 表头行 + 四个 .row-sub 事件行：设置搜索把它们当一组（searchFilter），搜「出错」带出列头，
          搜「灵动岛」命中列头带出全部事件，不会留下没有开关的空表头。 */}
      <div className="row-card notify-matrix" data-testid="notify-matrix">
        <div className="row notify-matrix-head">
          <div className="row-text" />
          <div className="notify-cols">
            <span className={"notify-col" + (notifyOn ? "" : " is-off")}>{t.settings.notifyColSystem}</span>
            {hasIsland && <span className="notify-col">{t.settings.notifyColIsland}</span>}
          </div>
        </div>
        {ALERT_KINDS.map((kind) => {
          const label = t.settings.islandAlertKinds[kind];
          return (
            <div className="row row-sub notify-matrix-row" key={kind}>
              <div className="row-text"><div className="row-label">{label}</div></div>
              <div className="notify-cols">
                <span className="notify-cell">
                  {/* 桌面通知总开关关着：这一列置灰但保留各自的勾选，重新打开时原样恢复。 */}
                  <Switch
                    checked={notifyKinds.includes(kind)}
                    disabled={!notifyOn}
                    onChange={() => patch({ notify_kinds: toggleIn(notifyKinds, kind) })}
                    label={`${t.settings.notifyColSystem} · ${label}`}
                  />
                </span>
                {hasIsland && <span className="notify-cell">
                  <Switch
                    checked={islandAlerts.includes(kind)}
                    onChange={() => patch({ island_alerts: toggleIn(islandAlerts, kind) })}
                    label={`${t.settings.notifyColIsland} · ${label}`}
                  />
                </span>}
              </div>
            </div>
          );
        })}
      </div>

      <div className="sec-caption">{t.settings.notifyPushGroup}</div>
      <div className="row-card">
        <BarkPushRow
          value={settings?.bark_url ?? ""}
          paused={settings?.bark_paused ?? false}
          onCommit={(url) => patch({ bark_url: url })}
          onTogglePaused={() => { void patch({ bark_paused: !(settings?.bark_paused ?? false) }); }}
        />
      </div>
      <SettingsError preformatted error={patchError} onDismiss={clearPatchError} />
    </>
  );
}
