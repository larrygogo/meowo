// 面板里「交给 agent」：把一句现成的话放进对话输入框，用户看过再自己发（walkthrough P-1，2026-11-12）。
// 冲突这类 Meowo 不替用户做决定的事，出路是交给正在这个仓库里干活的 agent，而不是叫人去终端。
// ChatWindow 经 SidePanel 提供；没人提供（老的 GitDiffView 面板、单测）就不出这个按钮。
import { createContext } from "react";

export const PanelPromptContext = createContext<((text: string) => void) | null>(null);
