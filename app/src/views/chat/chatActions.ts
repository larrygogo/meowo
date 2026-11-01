import { createContext } from "react";

/// 对话流里的消息需要回调到对话窗的动作（如出错卡的「继续」）。对话窗之外（子任务时间线、
/// 预览页）不提供，取到 null 时消息只做展示、不出按钮。
export type ChatActions = {
  /// 以用户身份发一条消息给当前会话，成功返回 true。
  sendText: (text: string) => Promise<boolean>;
  /// 撤回这条用户消息及之后的内容，原文（text）放回输入框。不支持或此刻不可用时缺省。
  undoFrom?: (id: string, text: string) => void;
  /// 此刻可撤回的用户消息 id（当前段、非插话、会话空闲）。
  undoableIds?: ReadonlySet<string>;
};

export const ChatActionsContext = createContext<ChatActions | null>(null);
