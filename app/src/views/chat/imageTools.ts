/** 对照工具在对话页上的统一名字（五家插件都换成这个形态，见 meowo-agent images.rs canonical_meowo_tool）。 */
export const COMPARE_TOOL = "mcp__meowo__compare_images";
/** 「点一个」方案卡（offer_choices）。 */
export const CHOICES_TOOL = "mcp__meowo__offer_choices";
/** 交文件（send_file）与主动提醒（notify_user）。 */
export const FILE_TOOL = "mcp__meowo__send_file";
export const NOTIFY_TOOL = "mcp__meowo__notify_user";

/// 说给人看的消息卡（发图、改前改后对照、方案卡、交文件、提醒）：排在正文里，不收进过程块，运行条不露工具原名。
export function isImageMessage(item: { kind: string; name: string }): boolean {
  return item.kind === "image" || item.name === COMPARE_TOOL || item.name === CHOICES_TOOL || item.name === FILE_TOOL || item.name === NOTIFY_TOOL;
}

export function isFileTool(item: { name: string }): boolean {
  return item.name === FILE_TOOL;
}

export function isNotifyTool(item: { name: string }): boolean {
  return item.name === NOTIFY_TOOL;
}

export function isCompareTool(item: { name: string }): boolean {
  return item.name === COMPARE_TOOL;
}

export function isChoicesTool(item: { name: string }): boolean {
  return item.name === CHOICES_TOOL;
}
