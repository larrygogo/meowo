import { createContext } from "react";

/// 会话工作目录：工具行里落在它下面的路径显示成相对路径（终端里就是这样），
/// 绝对路径的长前缀会把真正有用的文件名挤出可视区（2026-10 用户实拍）。回复里行内代码的相对路径也按它定位。
/// 单独成模块：ChatMarkdown 也要读它，从 ToolActivity 引会形成循环依赖。
export const ChatCwdContext = createContext<string | null>(null);
