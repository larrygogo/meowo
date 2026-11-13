import { emptyComposerFrame } from "../chatWindow.harness";
import { describe, expect, it, vi } from "vitest";
import { chatUi } from "../../test/agents";
import { confirmComposerEmpty } from "./composerConfirmation";

describe("原生 composer 的保守确认", () => {
  it("实拍非占位空框与原生光标起点一致才允许，不改变原帧", async () => {
    const frame = emptyComposerFrame("kimi")!;
    const before = structuredClone(frame);
    const read = vi.fn(async () => frame);
    expect(await confirmComposerEmpty({ read }, chatUi("kimi")!.composer_guard)).toBe("empty");
    expect(read).toHaveBeenCalledTimes(1);
    expect(frame).toEqual(before);
  });

  it.each(["codex", "gemini", "opencode"])("%s 占位与同文草稿可能具有相同文字和光标，未知样式不能放行", async (provider) => {
    const nativeEmpty = emptyComposerFrame(provider)!;
    const sameTextDraftAtHome = structuredClone(nativeEmpty);
    const guard = chatUi(provider)!.composer_guard;
    expect(await confirmComposerEmpty({ read: async () => nativeEmpty }, guard)).toBe("unknown");
    expect(await confirmComposerEmpty({ read: async () => sameTextDraftAtHome }, guard)).toBe("unknown");
    expect(sameTextDraftAtHome).toEqual(nativeEmpty);
  });

  it("非空原稿保留并返回 occupied，不清空或尝试编辑", async () => {
    const frame = emptyComposerFrame("kimi")!;
    frame.lines[1] = frame.lines[1].replace("> ", "> 尚未提交的原稿");
    const before = structuredClone(frame);
    expect(await confirmComposerEmpty({ read: async () => frame }, chatUi("kimi")!.composer_guard)).toBe("occupied");
    expect(frame).toEqual(before);
  });

  it("缺能力不读取帧，更不能默认允许", async () => {
    const read = vi.fn(async () => emptyComposerFrame("kimi"));
    expect(await confirmComposerEmpty({ read }, undefined)).toBe("unknown");
    expect(read).not.toHaveBeenCalled();
  });

  it("帧缺失或读取失败时保持 unknown", async () => {
    const guard = chatUi("kimi")!.composer_guard;
    expect(await confirmComposerEmpty({ read: async () => null }, guard)).toBe("unknown");
    expect(await confirmComposerEmpty({ read: async () => { throw new Error("offline"); } }, guard)).toBe("unknown");
  });

  it.each([
    { cursorVisible: false }, { cursorRow: 0 }, { cursorCol: 0 },
  ])("光标隐藏或不在起点不放行：%j", async (change) => {
    const frame = { ...emptyComposerFrame("kimi")!, ...change };
    expect(await confirmComposerEmpty({ read: async () => frame }, chatUi("kimi")!.composer_guard)).toBe("unknown");
  });

  it("未识别框体不能用正确光标坐标补成空框", async () => {
    const frame = { ...emptyComposerFrame("kimi")!, lines: ["unrecognized overlay"] };
    expect(await confirmComposerEmpty({ read: async () => frame }, chatUi("kimi")!.composer_guard)).toBe("unknown");
  });

  it("测试固件只提供已实拍版本的声明，未知 provider 没有空框", () => {
    for (const [provider, version] of Object.entries({ codex: "0.167.2", kimi: "2.5.3", gemini: "0.65.0", opencode: "1.19.3" })) {
      expect(chatUi(provider)!.version).toBe(version);
      expect(chatUi(provider)!.composer_guard).toBeDefined();
    }
    expect(emptyComposerFrame("unknown")).toBeNull();
  });
});
