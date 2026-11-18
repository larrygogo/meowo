import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FEEDBACK_MS, useFlash } from "./useFlash";

/// U3：「已复制」「已保存」这类成功提示统一 2.5 秒消失，由一个常量控制。

describe("useFlash", () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it("常量是 2.5 秒", () => {
    expect(FEEDBACK_MS).toBe(2500);
  });

  it("flash 后停留 FEEDBACK_MS 再复位", () => {
    const { result } = renderHook(() => useFlash());
    act(() => result.current.flash());
    expect(result.current.shown).toBe(true);
    act(() => { vi.advanceTimersByTime(FEEDBACK_MS - 1); });
    expect(result.current.shown).toBe(true);
    act(() => { vi.advanceTimersByTime(1); });
    expect(result.current.shown).toBe(false);
  });

  it("再次 flash 重新计时；hide 立即复位", () => {
    const { result } = renderHook(() => useFlash());
    act(() => result.current.flash());
    act(() => { vi.advanceTimersByTime(2000); });
    act(() => result.current.flash());
    act(() => { vi.advanceTimersByTime(2000); });
    expect(result.current.shown).toBe(true);
    act(() => result.current.hide());
    expect(result.current.shown).toBe(false);
  });

  it("卸载后计时器清掉", () => {
    const { result, unmount } = renderHook(() => useFlash());
    act(() => result.current.flash());
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("成功提示不再各写各的时长", () => {
  // 改造前散在 ApprovalCard / GitDiffView / Message / ChatMarkdown / ForgeAccountsSection /
  // RemoteAccessCard 的 `setTimeout(() => setCopied(false), 1500)`，以及设置「已保存」、诊断「已导出」的 2000。
  const src = join(dirname(fileURLToPath(import.meta.url)), "..");
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const p = join(dir, name);
      if (statSync(p).isDirectory()) { if (name !== "generated" && name !== "node_modules") walk(p); }
      else if (/\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name)) files.push(p);
    }
  };
  walk(src);

  it("没有手写的 setCopied/setSaved/setDiagExported(false) 计时器", () => {
    const hits = files.flatMap((file) => readFileSync(file, "utf8").split("\n")
      .filter((line) => /setTimeout\(\s*\(\)\s*=>\s*(\{[^}]*)?set(Copied|Saved|DiagExported)\(false\)/.test(line))
      .map((line) => `${file}: ${line.trim()}`));
    expect(hits).toEqual([]);
  });
});
