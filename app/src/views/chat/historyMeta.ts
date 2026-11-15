import type { ChatHistory } from "../../api";

/// 两次 history 的**渲染相关**字段是否完全一致。稳态轮询里这些值一轮都不变，
/// 据此保留旧引用即可跳过整窗重渲染。
///
/// **排除法**而非白名单:除 items/offset/reset(由 setItems 单独短路)外,next 的
/// 所有字段默认参与比较——后端新增 DTO 字段自动被覆盖,无需记得回来改这里。
/// 此前的逐字段清单同一类漏加事故出了三回(todos、connected、ptyManaged/errored),
/// 每次都表现为「数据变了但界面不动」,不再给第四次机会。
const HISTORY_META_EXCLUDED: ReadonlySet<string> = new Set(["items", "offset", "reset"]);
export function sameHistoryMeta(prev: ChatHistory | null, next: ChatHistory): boolean {
  if (prev === null) return false;
  for (const key of Object.keys(next) as (keyof ChatHistory)[]) {
    if (HISTORY_META_EXCLUDED.has(key)) continue;
    if (key === "agentModes") {
      if (prev.agentModes.length !== next.agentModes.length) return false;
      if (!prev.agentModes.every((mode, index) => mode.dimension === next.agentModes[index]?.dimension && mode.value === next.agentModes[index]?.value)) return false;
    } else if (key === "todos") {
      // 容错取值：旧后端没有这个字段，不能因为版本错配就整窗崩掉。
      if ((prev.todos?.length ?? 0) !== (next.todos?.length ?? 0)) return false;
      if (!(prev.todos ?? []).every((todo, index) =>
        todo.content === next.todos?.[index]?.content && todo.status === next.todos?.[index]?.status)) return false;
    } else if (prev[key] !== next[key]) {
      return false;
    }
  }
  return true;
}

// 交接注入语（两种界面语言的模板前缀，见 i18n handoffPrompt）+ 交接目录标记：识别
// transcript 里那条「请读交接文件」的机器消息。前序段历史内联进时间线后，它再以用户
// 气泡出现只是重复分隔条已说的内容——显示层滤掉；items 记账层保留原文，水位线等
// 逻辑不受影响。
const HANDOFF_PROMPT_PREFIXES = ["请先完整阅读文件 ", "Please read the file "];
export function isHandoffInjectedPrompt(text: string): boolean {
  return text.includes("meowo-handoff") && HANDOFF_PROMPT_PREFIXES.some((prefix) => text.startsWith(prefix));
}
