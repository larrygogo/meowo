// 视图偏好：用户在终端就显示终端、在对话就显示对话（实拍反馈）。只记**明确的**视图
// 选择（顶栏「对话/终端」切换、Ctrl+1/2）；程序性跳转（审批引导、后台会话兜底、
// 探测失败落终端）不算——偏好表达「用户平时住在哪」，不是「上次被带去过哪」。
const VIEW_PREF_KEY = "meowo-chat-view-pref";
export function storedViewPref(): "chat" | "terminal" {
  return localStorage.getItem(VIEW_PREF_KEY) === "terminal" ? "terminal" : "chat";
}
export function rememberViewPref(view: "chat" | "terminal") {
  localStorage.setItem(VIEW_PREF_KEY, view);
}
