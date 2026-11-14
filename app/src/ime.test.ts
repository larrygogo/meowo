import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { imeComposing } from "./ime";

describe("输入法守卫", () => {
  it("组字中算输入法按键", () => {
    expect(imeComposing({ isComposing: true, keyCode: 13 })).toBe(true);
    expect(imeComposing({ nativeEvent: { isComposing: true }, keyCode: 13 })).toBe(true);
  });

  it("WebKit 确认候选词的回车 isComposing 为 false，靠 keyCode 229 认出", () => {
    expect(imeComposing({ isComposing: false, keyCode: 229 })).toBe(true);
    expect(imeComposing({ nativeEvent: { isComposing: false, keyCode: 229 } })).toBe(true);
    expect(imeComposing({ nativeEvent: { isComposing: false }, keyCode: 229 })).toBe(true);
  });

  it("普通回车不拦", () => {
    expect(imeComposing({ isComposing: false, keyCode: 13 })).toBe(false);
    expect(imeComposing({ nativeEvent: { isComposing: false, keyCode: 13 }, keyCode: 13 })).toBe(false);
    expect(imeComposing({})).toBe(false);
  });

  // 对话窗输入框、新建页输入框、贴纸行内编辑必须走同一个助手：各写各的时漏过 229 兜底（走查输入 A7）。
  it.each([
    "views/ChatWindow.tsx",
    "views/chat/NewSessionPage.tsx",
    "views/sticker/helpers.ts",
  ])("%s 不直接读 isComposing", (file) => {
    const source = readFileSync(resolve(__dirname, file), "utf8");
    expect(source).not.toMatch(/\bisComposing\b/);
    expect(source).not.toMatch(/keyCode === 229/);
    expect(source).toMatch(/imeComposing\(/);
  });
});
