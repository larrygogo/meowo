import { isRiskyCommand } from "./ApprovalCard";

/// 审批卡上的影响范围标签（对照 WorkBuddy：只在写工作区外、删除、执行脚本这几类时
/// 让人多看一眼）。只是提示，不改变任何决定回路。判定一律宁漏勿冤：漏判只是少一个
/// 标签，误判会让人对一条安全操作起疑。
export type ApprovalScopeTag =
  | { kind: "outside"; path: string }
  | { kind: "inside" }
  | { kind: "delete" }
  | { kind: "risky" }
  | { kind: "network" };

/** 写文件类工具参数里放路径的键（claude 的 Edit/Write/MultiEdit/NotebookEdit，kimi/gemini 同名）。 */
const PATH_KEYS = ["file_path", "path", "notebook_path", "filePath", "absolute_path"];

const DELETE_COMMAND = /(?:^|[\s;&|(])(?:rm|rmdir|unlink|shred|del|erase|rd|Remove-Item|ri)(?=\s|$)|\bgit\s+clean\b/i;
const NETWORK_COMMAND = /(?:^|[\s;&|(])(?:curl|wget|Invoke-WebRequest|Invoke-RestMethod|iwr|irm|scp|ssh|rsync|ftp)(?=\s|$)|\bgit\s+push\b|\b(?:npm|pnpm|yarn|bun|cargo)\s+publish\b/i;
/// 命令里的绝对路径：Windows 盘符路径、`~/…`，以及至少两段的 Unix 绝对路径。
/// 两段的门槛是为了不把 cmd 的开关（`/s`、`/q`）当成路径；/dev、/proc 是设备不是文件。
const COMMAND_PATH = /(?:^|[\s"'=(])((?:[a-zA-Z]:[\\/]|~[\\/])[^\s"'|;&<>]*|\/(?!dev\/|proc\/)[\w.-]+\/[^\s"'|;&<>]*)/g;

export function approvalScope(tool: string | null | undefined, input: string, dirs: readonly string[]): ApprovalScopeTag[] {
  let paths: string[] = [];
  let command: string | null = null;
  const parsed = parseObject(input);
  if (parsed) {
    paths = PATH_KEYS.map((key) => parsed[key]).filter((value): value is string => typeof value === "string" && value.length > 0);
    if (typeof parsed.command === "string") command = parsed.command;
  } else if (/edit|write/i.test(tool ?? "") && !/\s/.test(input.trim()) && input.trim()) {
    // 屏幕识别的编辑审批只给一个路径（没有 JSON 参数）。
    paths = [input.trim()];
  } else {
    command = input;
  }

  const tags: ApprovalScopeTag[] = [];
  const roots = dirs.map(normalize).filter(Boolean);
  const candidates = [...paths, ...(command ? commandPaths(command) : [])];
  const outside = roots.length > 0 ? candidates.filter((path) => !insideAny(path, roots)) : [];
  for (const path of dedupe(outside).slice(0, 2)) tags.push({ kind: "outside", path });
  if (command) {
    if (isRiskyCommand(command)) tags.push({ kind: "risky" });
    if (DELETE_COMMAND.test(command)) tags.push({ kind: "delete" });
    if (NETWORK_COMMAND.test(command)) tags.push({ kind: "network" });
  }
  // 明确是改文件、而且都落在工作区里：给一句定心话。命令类不给（看不全它会碰什么）。
  if (tags.length === 0 && paths.length > 0 && !command && roots.length > 0) tags.push({ kind: "inside" });
  return tags;
}

function parseObject(input: string): Record<string, unknown> | null {
  const text = input.trim();
  if (!text.startsWith("{")) return null;
  try {
    const value: unknown = JSON.parse(text);
    return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
  } catch {
    // reporter 把超过 16KB 的参数截断（attach.rs），JSON 就不完整了：从开头捞路径和命令。
    const loose: Record<string, unknown> = {};
    for (const key of [...PATH_KEYS, "command"]) {
      const match = new RegExp(`"${key}"\\s*:\\s*"((?:[^"\\\\]|\\\\.)*)"`).exec(text);
      if (match) {
        try { loose[key] = JSON.parse(`"${match[1]}"`); } catch { /* 半截转义,跳过 */ }
      }
    }
    return Object.keys(loose).length > 0 ? loose : null;
  }
}

function commandPaths(command: string): string[] {
  const out: string[] = [];
  for (const match of command.matchAll(COMMAND_PATH)) out.push(match[1].replace(/[),]+$/, ""));
  return out;
}

/** 正斜杠、小写、解析 `.`/`..`、去尾斜杠。`~` 原样保留（展不开就当工作区外）。 */
function normalize(path: string): string {
  const parts: string[] = [];
  const unified = path.replace(/\\/g, "/").toLowerCase();
  const absolute = unified.startsWith("/");
  for (const part of unified.split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") parts.pop();
    else parts.push(part);
  }
  return (absolute ? "/" : "") + parts.join("/");
}

function isAbsolute(path: string): boolean {
  return /^(?:[a-zA-Z]:[\\/]|[\\/]|~)/.test(path);
}

function insideAny(path: string, roots: readonly string[]): boolean {
  // 相对路径按主目录（dirs[0]）补全再比，`..\..\x` 这种能爬出去的照样算出界。
  const full = normalize(isAbsolute(path) ? path : `${roots[0]}/${path}`);
  return roots.some((root) => full === root || full.startsWith(root + "/"));
}

function dedupe(paths: string[]): string[] {
  const seen = new Set<string>();
  return paths.filter((path) => {
    const key = normalize(path);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}
