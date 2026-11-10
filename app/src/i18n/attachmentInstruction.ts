// 附件指令头单独成模块：Message.tsx 要同时认两种语言写下的历史消息（见 ATTACHMENT_INSTRUCTION_HEADERS），
// 原先为此静态 import 整份 zh + en 字典，把非当前语言的字典又拖回了首屏（i18n 改成按需加载后）。
// 两份字典的 chat.attachmentInstruction 直接引用这里，文案仍只有一处真源。
export const attachmentInstructionZh = (files: string) => `请读取并结合以下本地附件完成任务（图片请使用图像读取能力）：\n${files}`;
export const attachmentInstructionEn = (files: string) => `Read the following local attachments and use them to complete the task (use image reading for images):\n${files}`;
