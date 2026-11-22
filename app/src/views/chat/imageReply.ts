import { createContext } from "react";

/// 「圈出来回复」：大图里点了之后，把这张图交给输入框所在的对话窗去打开标注层。
/// 对话窗（ChatWindow）提供；没有输入框的地方（只读回放、独立预览）不提供，按钮就不出现。
export const ImageReplyContext = createContext<((path: string) => void) | null>(null);
