// 设置窗口的共享状态层：设置对象的默认占位与读写 hook。
// 各 section（general/appearance/account）与主 About 共用，避免从 About.tsx 反向导入成环。
import { useRef, useState, useEffect } from "react";
import { getSettings, setSettings, type Settings } from "../../api";

export const SETTINGS_DEFAULTS: Settings = {
  archive_hide_days: 0,
  notifications_enabled: true,
  theme: "dark",
  opacity: 94,
  ui_scale: 100,
  resume_terminal: "terminal",
  language: "auto",
  terminal_open_mode: "card",
  card_menu_mode: "button",
  preview_enabled: true,
  sticker_style: "elevated",
  sticker_color: "classic",
  // 首帧占位（get_settings() resolve 前）。真实默认值由后端 settings 给，前端不据此做任何判断。
  sticker_quota_providers: ["claude"],
  default_agent: "claude",
};

// 设置读写：本地保留完整对象，每次只 patch 改动字段后整对象写回（后端 set_settings 收整对象，
// 漏字段会被 serde 默认值覆盖 → 必须整对象提交）。写失败则回读后端保持一致。
export function useSettingsState() {
  const [settings, setSettingsState] = useState<Settings | null>(null);
  const ref = useRef<Settings>(SETTINGS_DEFAULTS);
  useEffect(() => {
    getSettings()
      .then((s) => {
        ref.current = s;
        setSettingsState(s);
      })
      .catch(() => {});
  }, []);
  const patch = (p: Partial<Settings>) => {
    // 真实设置尚未回填（首帧到 get_settings resolve 之间）时忽略：避免用默认值整对象覆盖磁盘。
    if (settings === null) return;
    const next = { ...ref.current, ...p };
    ref.current = next;
    setSettingsState(next);
    setSettings(next).catch(() => {
      getSettings()
        .then((s) => {
          ref.current = s;
          setSettingsState(s);
        })
        .catch(() => {});
    });
  };
  return [settings, patch] as const;
}
