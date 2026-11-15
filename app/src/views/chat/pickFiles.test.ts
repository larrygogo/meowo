import { afterEach, describe, expect, it, vi } from "vitest";
import { pickFilesViaInput } from "./pickFiles";

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

describe("pickFilesViaInput", () => {
  it("选中文件后给出文件并摘掉临时 input", async () => {
    vi.spyOn(HTMLInputElement.prototype, "click").mockImplementation(() => {});
    const picked = pickFilesViaInput();
    const input = document.querySelector<HTMLInputElement>('input[type="file"]')!;
    expect(input.multiple).toBe(true);
    const file = new File(["x"], "a.png");
    Object.defineProperty(input, "files", { value: [file] });
    input.dispatchEvent(new Event("change"));
    expect(await picked).toEqual([file]);
    expect(document.querySelector('input[type="file"]')).toBeNull();
  });

  it("取消选择：窗口重新获焦一秒后给出空数组，不让 Promise 永挂", async () => {
    vi.useFakeTimers();
    vi.spyOn(HTMLInputElement.prototype, "click").mockImplementation(() => {});
    const picked = pickFilesViaInput();
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(1000);
    expect(await picked).toEqual([]);
    expect(document.querySelector('input[type="file"]')).toBeNull();
  });
});
