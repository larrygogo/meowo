// 托管终端里的 Ctrl+B（调研 i4，关卡 A 定案）：焦点在托管终端时交给 PTY（claude 转后台），
// 对话视图里仍收侧栏。
import { describe, expect, it } from "vitest";
import { isSidebarToggleKey, isTerminalAppNavKey } from "./terminalKeys";

const key = (code: string, mods: Partial<Record<"ctrlKey" | "metaKey" | "altKey" | "shiftKey", boolean>> = {}) =>
  ({ code, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...mods });

describe("托管终端放行给窗口的组合键", () => {
  it("Ctrl+B 不放行：xterm 照常把 \\x02 发给 PTY", () => {
    expect(isTerminalAppNavKey(key("KeyB", { ctrlKey: true }))).toBe(false);
  });
  it("⌘B（macOS 收侧栏）仍放行给窗口", () => {
    expect(isTerminalAppNavKey(key("KeyB", { metaKey: true }))).toBe(true);
  });
  it("Ctrl+K/N/1/2 维持应用占用", () => {
    for (const code of ["KeyK", "KeyN", "Digit1", "Digit2"]) expect(isTerminalAppNavKey(key(code, { ctrlKey: true })), code).toBe(true);
  });
  it("带 Alt/Shift 或不带修饰键的不算", () => {
    expect(isTerminalAppNavKey(key("KeyK", { ctrlKey: true, shiftKey: true }))).toBe(false);
    expect(isTerminalAppNavKey(key("KeyK", { ctrlKey: true, altKey: true }))).toBe(false);
    expect(isTerminalAppNavKey(key("KeyK"))).toBe(false);
  });
});

describe("窗口级 Ctrl+B 收侧栏", () => {
  const terminal = document.createElement("div");
  terminal.className = "managed-terminal";
  const textarea = document.createElement("textarea");
  terminal.appendChild(textarea);
  const composer = document.createElement("textarea");
  document.body.append(terminal, composer);

  it("焦点在对话视图（composer 等）：Ctrl+B 收侧栏", () => {
    expect(isSidebarToggleKey(key("KeyB", { ctrlKey: true }), composer)).toBe(true);
    expect(isSidebarToggleKey(key("KeyB", { ctrlKey: true }), document.body)).toBe(true);
  });
  it("焦点在托管终端里：Ctrl+B 不收侧栏，⌘B 照收", () => {
    expect(isSidebarToggleKey(key("KeyB", { ctrlKey: true }), textarea)).toBe(false);
    expect(isSidebarToggleKey(key("KeyB", { metaKey: true }), textarea)).toBe(true);
  });
});

describe("接线", () => {
  // 判定函数只有被两处真正调用才算数：托管终端的 xterm 键处理与窗口快捷键监听。
  it("ManagedTerminal 与 ChatWindow 都走这两个判定", async () => {
    const { readFileSync } = await import("node:fs");
    const { dirname, join } = await import("node:path");
    const { fileURLToPath } = await import("node:url");
    const here = dirname(fileURLToPath(import.meta.url));
    expect(readFileSync(join(here, "ManagedTerminal.tsx"), "utf8")).toMatch(/isTerminalAppNavKey\(event\)/);
    expect(readFileSync(join(here, "ChatWindow.tsx"), "utf8")).toMatch(/isSidebarToggleKey\(e, document\.activeElement\)/);
  });
});
