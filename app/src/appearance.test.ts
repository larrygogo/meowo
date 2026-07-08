import { describe, expect, it } from "vitest";
import { STICKER_COLORS, STICKER_COLOR_KEYS, stickerBgRgb } from "./appearance";

describe("stickerBgRgb", () => {
  it("已知预设按主题取深/浅一套底色 RGB", () => {
    expect(stickerBgRgb("slate", "dark")).toBe(STICKER_COLORS.slate.dark);
    expect(stickerBgRgb("slate", "light")).toBe(STICKER_COLORS.slate.light);
    expect(stickerBgRgb("amber", "dark")).toBe(STICKER_COLORS.amber.dark);
  });

  it("未知 key 回退默认预设（无色）", () => {
    expect(stickerBgRgb("does-not-exist", "dark")).toBe(STICKER_COLORS.neutral.dark);
    expect(stickerBgRgb("", "light")).toBe(STICKER_COLORS.neutral.light);
  });

  it("经典预设的深色底与 styles.css 初值一致（升级零变化）", () => {
    expect(stickerBgRgb("classic", "dark")).toBe("38, 38, 36");
    expect(stickerBgRgb("classic", "light")).toBe("250, 249, 245");
  });

  it("每个预设都含 swatch / dark / light 三个非空字段", () => {
    for (const k of STICKER_COLOR_KEYS) {
      const p = STICKER_COLORS[k];
      expect(p.swatch).toMatch(/^#[0-9a-fA-F]{6}$/);
      expect(p.dark).toMatch(/^\d+, \d+, \d+$/);
      expect(p.light).toMatch(/^\d+, \d+, \d+$/);
    }
  });
});
