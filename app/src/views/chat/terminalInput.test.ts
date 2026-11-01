import { afterEach, describe, expect, it, vi } from "vitest";
import type { ManagedTerminalSnapshot } from "../../api";
import { pasteImagesNatively, writeInterruptKeys, type NativePasteIo } from "./terminalInput";

afterEach(() => vi.useRealTimers());

const b64 = (text: string) => btoa(String.fromCharCode(...new TextEncoder().encode(text)));
const snap = (endOffset: number, data = "", active = true): ManagedTerminalSnapshot =>
  ({ sessionId: 1, active, managed: true, data, startOffset: 0, endOffset, exited: false, exitCode: null } as unknown as ManagedTerminalSnapshot);

/// 假终端：每次粘贴键写入后，下一次增量快照吐出第 N 个占位符（landAfter 控制哪几张落地）。
function fakeTerminal(landing: boolean[]) {
  const writes: string[] = [];
  const clipboard: string[] = [];
  let offset = 10;
  let pending = "";
  let pasted = 0;
  const io: NativePasteIo = {
    setClipboardImage: async (path) => { clipboard.push(path); },
    write: async (data) => {
      writes.push(data);
      if (landing[pasted]) pending = `[Image #${pasted + 1}] `;
      pasted += 1;
    },
    snapshot: async (since) => {
      if (since === undefined) return snap(offset);
      const data = pending;
      pending = "";
      offset += data.length;
      return snap(offset, b64(data));
    },
  };
  return { io, writes, clipboard };
}

const base = {
  marker: String.raw`\[Image #\d+\]`,
  pasteInput: "\x16",
  abort: () => null,
  onPasteStarted: () => {},
  landingPolls: 3,
  unconfirmed: "图片附加未完成",
};

describe("writeInterruptKeys", () => {
  /// opencode 首按 Esc 只提示、第二下才停；两个 Esc 合并成一次写入会被当成 Alt+Esc。
  it("按声明次数分次写，不合并", async () => {
    const writes: string[] = [];
    await writeInterruptKeys(async (data) => { writes.push(data); }, "\x1b", 2);
    expect(writes).toEqual(["\x1b", "\x1b"]);
  });

  it("次数缺省或非法时至少按一次", async () => {
    const writes: string[] = [];
    await writeInterruptKeys(async (data) => { writes.push(data); }, "\x1b", 0);
    expect(writes).toEqual(["\x1b"]);
  });
});

describe("pasteImagesNatively", () => {
  it("逐张写剪贴板、发粘贴键、等到第 N 个占位符再贴下一张", async () => {
    const { io, writes, clipboard } = fakeTerminal([true, true]);
    const started = vi.fn();
    const outcome = await pasteImagesNatively({ ...base, images: ["a.png", "b.png"], io, abandon: async () => true, onPasteStarted: started });
    expect(outcome).toBe("landed");
    expect(clipboard).toEqual(["a.png", "b.png"]);
    expect(writes).toEqual(["\x16", "\x16"]);
    expect(started).toHaveBeenCalledTimes(1);
  });

  /// 首张就写不进剪贴板：还没向终端粘贴过任何东西，安全降级到文本路径，不必清残图。
  it("首张剪贴板失败直接降级，不碰终端", async () => {
    const { io, writes } = fakeTerminal([true]);
    io.setClipboardImage = async () => { throw new Error("clipboard busy"); };
    const abandon = vi.fn(async () => true);
    expect(await pasteImagesNatively({ ...base, images: ["a.png"], io, abandon })).toBe("fallback");
    expect(writes).toEqual([]);
    expect(abandon).not.toHaveBeenCalled();
  });

  /// 第二张没落地：输入框里已有第一张，必须先清干净才能降级；清不掉就报错，绝不带着残图降级。
  it("中途没落地：残图清干净才降级，清不掉报错", async () => {
    const cleaned = fakeTerminal([true, false]);
    expect(await pasteImagesNatively({ ...base, images: ["a.png", "b.png"], io: cleaned.io, abandon: async () => true })).toBe("fallback");
    const stuck = fakeTerminal([true, false]);
    await expect(pasteImagesNatively({ ...base, images: ["a.png", "b.png"], io: stuck.io, abandon: async () => false })).rejects.toThrow("图片附加未完成");
  });

  it("中途有交互提示弹出（abort 非空）就中止", async () => {
    const { io } = fakeTerminal([true, true]);
    let calls = 0;
    const abort = () => (++calls > 2 ? "终端在等你确认" : null);
    await expect(pasteImagesNatively({ ...base, images: ["a.png", "b.png"], io, abort, abandon: async () => true })).rejects.toThrow("终端在等你确认");
  });
});
