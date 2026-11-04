import { useT } from "../../i18n";

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
/// 全部按 1.8 描边画：放大镜曾是填充轮廓（内外两圈），与笔/文件并排时明显发粗。
export function ToolIcon({ name }: { name: string }) {
  const normalized = name.toLowerCase();
  const path = normalized.includes("grep") || normalized.includes("glob") || normalized.includes("search")
    ? "M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13zM15.5 15.5 20 20"
    : normalized === "read" || normalized.includes("view_image") || normalized.includes("notebook")
    ? "M6 3h8l4 4v14H6zM14 3v5h5"
    : normalized === "write" || normalized === "edit" || normalized.includes("patch")
    ? "M4 20h4L19.5 8.5a2.1 2.1 0 0 0-3-3L5 17zM13.5 6.5l3 3"
    : normalized.includes("fetch") || normalized.includes("web") || normalized.includes("http")
    ? "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zm-9 9h18M12 3c2.5 2.4 3.8 5.6 3.8 9s-1.3 6.6-3.8 9c-2.5-2.4-3.8-5.6-3.8-9s1.3-6.6 3.8-9z"
    : "M4 17l6-6-6-6M12 19h8";
  return (
    <span className="chat-tool-icon" data-kind={toolKind(name)}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><path d={path} /></svg>
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
