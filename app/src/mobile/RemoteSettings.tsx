import { useEffect, useRef, useState } from "react";
import { getSettings, setKeepAwake, setPushPaused, type Settings, type ThemeMode } from "../api";
import { remoteThemeMode, setRemoteThemeMode } from "../appearance";
import { useDismissable } from "../hooks/useDismissable";
import { useT } from "../i18n";
import { REMOTE_SETTINGS_EVENT } from "../remoteMode";
import { remoteTurnFolding, setRemoteTurnFolding } from "../remotePrefs";
import { Segmented, Switch } from "../views/settings/widgets";
import { Icon } from "../icons/Icon";
import { icSettings } from "../icons/glyphs";

/// 手机页的设置面板（列表页头右上角的齿轮）。只放人不在电脑前也用得上的几项：
/// - 这台手机：明暗模式、消息自动折叠。只存手机浏览器里，不碰电脑。
/// - 电脑：运行时防止休眠、暂停手机推送。各走一个只改该字段的接口，整份 set_settings 远程仍禁止。
/// 代理、账号、远程访问本身这些不放：手机令牌泄露时影响太大，改错了还可能连不回来。
export function RemoteSettings() {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [theme, setTheme] = useState<ThemeMode>(remoteThemeMode);
  const [settings, setSettingsState] = useState<Settings | null>(null);
  const [fold, setFold] = useState<boolean | null>(remoteTurnFolding);
  const [error, setError] = useState(false);
  const boxRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  useDismissable(boxRef, { open, onClose: () => setOpen(false), escFocusReturn: () => buttonRef.current });

  // 打开时取一次最新设置；开着期间电脑那边改了（12s 轮询发现变化）也跟上。
  useEffect(() => {
    if (!open) return;
    let alive = true;
    setError(false);
    getSettings().then((s) => { if (alive) setSettingsState(s); }).catch(() => {});
    const onRemote = (e: Event) => setSettingsState((e as CustomEvent<Settings>).detail);
    window.addEventListener(REMOTE_SETTINGS_EVENT, onRemote);
    return () => {
      alive = false;
      window.removeEventListener(REMOTE_SETTINGS_EVENT, onRemote);
    };
  }, [open]);

  // 电脑上的开关先改界面再写，写失败退回原值并提示。
  const toggleDesktop = (patch: Partial<Settings>, write: () => Promise<void>) => {
    if (!settings) return;
    const before = settings;
    setError(false);
    setSettingsState({ ...settings, ...patch });
    write().catch(() => {
      setSettingsState(before);
      setError(true);
    });
  };

  const foldOn = fold ?? settings?.chat_turn_folding === true;
  const keepAwake = settings?.keep_awake_while_running ?? true;
  const pushOn = !(settings?.bark_paused ?? false);
  // 没配推送地址时开关没有意义（地址只能在电脑上填），不显示。
  const hasPush = !!settings?.bark_url?.trim();

  return (
    <div className="remote-settings" ref={boxRef}>
      <button
        ref={buttonRef}
        type="button"
        className="remote-settings-btn"
        aria-label={t.remote.settingsLabel}
        aria-expanded={open}
        aria-haspopup="dialog"
        onClick={() => setOpen((v) => !v)}
      >
        <Icon glyph={icSettings} size={20} />
      </button>
      {open && (
        <div className="remote-settings-panel" role="dialog" aria-label={t.remote.settingsLabel}>
          <div className="remote-settings-group">{t.remote.settingsPhone}</div>
          <div className="remote-settings-row">
            <div className="remote-settings-text">
              <div className="remote-settings-label">{t.remote.themeLabel}</div>
            </div>
            <Segmented<ThemeMode>
              value={theme}
              options={[
                { value: "system", label: t.settings.themeSystem },
                { value: "light", label: t.settings.themeLight },
                { value: "dark", label: t.settings.themeDark },
              ]}
              onChange={(mode) => {
                setTheme(mode);
                setRemoteThemeMode(mode);
              }}
              label={t.remote.themeLabel}
            />
          </div>
          <div className="remote-settings-row">
            <div className="remote-settings-text">
              <div className="remote-settings-label">{t.settings.turnFolding}</div>
              <div className="remote-settings-desc">{t.remote.foldDesc}</div>
            </div>
            <Switch
              checked={foldOn}
              onChange={() => {
                setFold(!foldOn);
                setRemoteTurnFolding(!foldOn);
              }}
              label={t.settings.turnFolding}
            />
          </div>

          <div className="remote-settings-group">{t.remote.settingsDesktop}</div>
          <div className="remote-settings-row">
            <div className="remote-settings-text">
              <div className="remote-settings-label">{t.settings.keepAwake}</div>
              <div className="remote-settings-desc">{t.remote.keepAwakeDesc}</div>
            </div>
            <Switch
              checked={keepAwake}
              disabled={!settings}
              onChange={() => toggleDesktop({ keep_awake_while_running: !keepAwake }, () => setKeepAwake(!keepAwake))}
              label={t.settings.keepAwake}
            />
          </div>
          {hasPush && (
            <div className="remote-settings-row">
              <div className="remote-settings-text">
                <div className="remote-settings-label">{t.remote.pushToggle}</div>
                <div className="remote-settings-desc">{t.remote.pushToggleDesc}</div>
              </div>
              <Switch
                checked={pushOn}
                disabled={!settings}
                onChange={() => toggleDesktop({ bark_paused: pushOn }, () => setPushPaused(pushOn))}
                label={t.remote.pushToggle}
              />
            </div>
          )}
          {error && <div className="remote-settings-error" role="alert">{t.remote.settingFailed}</div>}
        </div>
      )}
    </div>
  );
}
