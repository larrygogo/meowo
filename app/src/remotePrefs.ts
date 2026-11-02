// 手机页自己的偏好（只存在这台手机的浏览器里，不写回电脑）。
//
// 「消息自动折叠」放这里而不是走桌面设置：手机屏小，往往想折；电脑上未必。两边各管各的，
// 手机上没拨过就跟着电脑的设置走。明暗模式同理，见 appearance.ts 的 remoteThemeMode。

const TURN_FOLDING_KEY = "meowo-remote-turn-folding";

/** 手机上拨了折叠开关后派发（detail 无），ChatWindow 据此重算。 */
export const REMOTE_PREFS_EVENT = "meowo:remote-prefs-changed";

// 本次打开内拨过的值。localStorage 写不进去（隐私模式）时靠它让这次照样生效。
let session: boolean | null = null;

/** 手机上是否折叠。没拨过返回 null，调用方用电脑的设置。 */
export function remoteTurnFolding(): boolean | null {
  if (session != null) return session;
  try {
    const v = localStorage.getItem(TURN_FOLDING_KEY);
    if (v === "on") return true;
    if (v === "off") return false;
  } catch {
    /* 隐私模式读不到：跟电脑 */
  }
  return null;
}

export function setRemoteTurnFolding(on: boolean): void {
  session = on;
  try {
    localStorage.setItem(TURN_FOLDING_KEY, on ? "on" : "off");
  } catch {
    /* 写不进去只影响下次打开，本次照样生效 */
  }
  window.dispatchEvent(new CustomEvent(REMOTE_PREFS_EVENT));
}
