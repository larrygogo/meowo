import { invoke } from "@tauri-apps/api/core";

/// 设置窗记住上次分区用的 localStorage 键（About.tsx 读写）。
export const SETTINGS_SECTION_KEY = "meowo-settings-section";

/// 打开设置窗并落到指定分区。open_settings 不带参数：窗口没开时，新窗按这个键挑首屏分区；
/// 已经开着时它只会被聚焦，靠同源窗口间的 storage 事件让设置窗跟着切（About.tsx 订阅）。
/// 远程桥对 open_settings 返 null，调用方在手机上别给这个入口。
export function openSettingsAt(section: string): void {
  try { localStorage.setItem(SETTINGS_SECTION_KEY, section); } catch { /* 隐私模式禁写：退回打开上次的分区 */ }
  void invoke("open_settings").catch(() => {});
}
