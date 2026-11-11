import { useT } from "../../i18n";
import { Icon } from "../../icons/Icon";
import { icGlobe, icPencil, icSearch, icTerminal } from "../../icons/glyphs";
import { icFile } from "../../icons/glyphs.chat";

export type ToolKind = "search" | "read" | "write" | "edit" | "run" | "web" | "todo" | "agent" | "other";

/// 工具的语义类别，驱动时间线上的类型标签与配色（data-kind）。判定顺序与
/// friendlyToolName 一致：网络类先于搜索类（web_search 含 "search"）。
export function toolKind(name: string): ToolKind {
  const normalized = name.toLowerCase();
  if (normalized === "bash" || normalized === "exec" || normalized.includes("shell") || normalized.includes("terminal")) return "run";
  if (normalized.includes("web") || normalized.includes("fetch") || normalized.includes("http")) return "web";
  if (normalized.includes("grep") || normalized.includes("glob") || normalized.includes("search")) return "search";
  if (normalized === "read" || normalized.includes("view_image")) return "read";
  if (normalized === "write") return "write";
  if (normalized === "edit" || normalized.includes("patch") || normalized.includes("notebook")) return "edit";
  if (normalized.includes("todo")) return "todo";
  if (normalized === "task" || normalized.includes("agent")) return "agent";
  return "other";
}

/// 按工具类型分图标：搜索 / 读文件 / 改文件 / 终端 / 网络 / 通用。
/// 一组操作若全是同一个 `>_`，用户在展开列表里无法一眼区分做了什么。
/// 形状走共用注册表、线宽由 Icon 统一：放大镜曾是填充轮廓（内外两圈），与笔/文件并排时明显发粗。
export function ToolIcon({ name }: { name: string }) {
  const normalized = name.toLowerCase();
  const glyph = normalized.includes("grep") || normalized.includes("glob") || normalized.includes("search")
    ? icSearch
    : normalized === "read" || normalized.includes("view_image") || normalized.includes("notebook")
    ? icFile
    : normalized === "write" || normalized === "edit" || normalized.includes("patch")
    ? icPencil
    : normalized.includes("fetch") || normalized.includes("web") || normalized.includes("http")
    ? icGlobe
    : icTerminal;
  return (
    <span className="chat-tool-icon" data-kind={toolKind(name)}>
      <Icon glyph={glyph} size={16} />
    </span>
  );
}

/// 工具行的简短动作名（对齐 Kimi 的 tools.label.*）：运行/读取/写入/编辑…
export function toolShortName(name: string, kind: ToolKind, t: ReturnType<typeof useT>): string {
  if (kind === "other") {
    // MCP 工具原名是 `mcp__<服务>__<工具>`，双下划线读着像乱码（本机近两周出现数百次）。
    const mcp = /^mcp__(.+?)__(.+)$/.exec(name);
    if (mcp) return `${mcp[1]} · ${mcp[2]}`;
    return t.chat.toolOtherNames[name] ?? name;
  }
  return t.chat.toolShortNames[kind] ?? name;
}
