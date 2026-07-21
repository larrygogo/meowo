import { type ChatItem } from "../../api";

export type ToolUseItem = Extract<ChatItem, { type: "tool_use" }>;
export type ToolResultItem = Extract<ChatItem, { type: "tool_result" }>;
