import { useT } from "../../i18n";
import type { Dict } from "../../i18n/zh";
import type { ToolKind } from "../../generated/contracts/ToolKind";
import { Icon } from "../../icons/Icon";
import { icGlobe, icPencil, icSearch, icTerminal } from "../../icons/glyphs";
import { icAgents, icFile, icList } from "../../icons/glyphs.chat";

/// 工具的语义类别。它由后端各家插件按自己的工具名表给出（ChatItem 的 `kind`），这里只消费：
/// 前端不看工具名去猜类别。此前按名字子串猜，规则是照 Claude 的命名调的，Gemini 的
/// read_file / replace 全落进「其他」，Codex 的 wait_agent 因为含 agent 被当成子任务。
/// 有扫描测试（toolKindSource.test.ts）拦着，别把按名字判断的写法加回来。
export type { ToolKind };

const KIND_GLYPH: Partial<Record<ToolKind, typeof icSearch>> = {
  search: icSearch,
  read: icFile,
  write: icPencil,
  edit: icPencil,
  web: icGlobe,
  // 子任务与待办用它们在别处已有的图标（子任务块、标题栏进度）：此前落到终端图标，
  // 一行「待办」顶着 >_，和上下的命令行分不开（2026-11-14 截图自验）。
  agent: icAgents,
  todo: icList,
};

/// 按工具类别分图标：搜索 / 读文件 / 改文件 / 网络 / 子任务 / 待办，其余用终端图标。
/// 一组操作若全是同一个 `>_`，用户在展开列表里无法一眼区分做了什么。
/// 形状走共用注册表、线宽由 Icon 统一：放大镜曾是填充轮廓（内外两圈），与笔/文件并排时明显发粗。
export function ToolIcon({ kind }: { kind: ToolKind }) {
  return (
    <span className="chat-tool-icon" data-kind={kind}>
      <Icon glyph={KIND_GLYPH[kind] ?? icTerminal} size={16} />
    </span>
  );
}

/// 工具行的简短动作名（对齐 Kimi 的 tools.label.*）：运行/读取/写入/编辑…
/// 未归类的工具没有动作名，显示它自己的名字（收录过的给译名，MCP 工具拆成「服务 · 工具」）。
export function toolShortName(name: string, kind: ToolKind, t: ReturnType<typeof useT>): string {
  if (kind === "other") {
    // MCP 工具原名是 `mcp__<服务>__<工具>`，双下划线读着像乱码（本机近两周出现数百次）。
    const mcp = /^mcp__(.+?)__(.+)$/.exec(name);
    if (mcp) return `${mcp[1]} · ${mcp[2]}`;
    return t.chat.toolOtherNames[name] ?? name;
  }
  return t.chat.toolShortNames[kind] ?? name;
}

/// 工具类别的完整叫法（「运行终端」「读取文件」…），给读屏用的隐藏标签。
/// 未归类的工具回落成它自己的名字，比编一个错译好。
export function toolKindLabel(name: string, kind: ToolKind, t: Dict): string {
  switch (kind) {
    case "run": return t.chat.runTerminal;
    case "read": return t.chat.readFile;
    // Write 与 Edit 分开叫：终端里就是两个词，「写入 225 行」挂在「编辑文件」下面读着别扭。
    case "write": return t.chat.writeFile;
    case "edit": return t.chat.editFile;
    case "search": return t.chat.searchFiles;
    case "web": return t.chat.fetchWeb;
    case "todo": return t.chat.updateTodos;
    case "agent": return t.chat.runAgent;
    default: return toolShortName(name, kind, t);
  }
}
