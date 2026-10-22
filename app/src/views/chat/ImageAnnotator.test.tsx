import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ImageAnnotator } from "./ImageAnnotator";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}));

beforeEach(() => {
  // jsdom 没有图片解码和 canvas:给一张 100×50 的假图,画布上下文为 null(画的那步跳过)。
  vi.stubGlobal("fetch", vi.fn(async () => ({ ok: true, blob: async () => new Blob() })));
  vi.stubGlobal("createImageBitmap", vi.fn(async () => ({ width: 100, height: 50 })));
  HTMLElement.prototype.setPointerCapture = () => {};
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("ImageAnnotator", () => {
  it("没画框时按钮是「不标注，直接附上」,点了原样交回", async () => {
    const onDone = vi.fn();
    render(<ImageAnnotator path="C:\\tmp\\a.png" onDone={onDone} onClose={() => {}} />);
    const skip = await screen.findByRole("button", { name: "不标注，直接附上" });
    await waitFor(() => expect((skip as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(skip);
    expect(onDone).toHaveBeenCalledWith({ png: null, notes: [] });
  });

  it("拖出一个框就多一行说明;太小的拖动不算;× 删框", async () => {
    render(<ImageAnnotator path="C:\\tmp\\a.png" onDone={() => {}} onClose={() => {}} />);
    const canvas = screen.getByLabelText("标注截图", { selector: "canvas" });
    await waitFor(() => expect((screen.getByRole("button", { name: "不标注，直接附上" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.pointerDown(canvas, { button: 0, clientX: 10, clientY: 10, pointerId: 1 });
    fireEvent.pointerUp(canvas, { clientX: 12, clientY: 12, pointerId: 1 });
    expect(screen.queryByPlaceholderText("这里要改什么")).toBeNull();

    fireEvent.pointerDown(canvas, { button: 0, clientX: 10, clientY: 10, pointerId: 1 });
    fireEvent.pointerMove(canvas, { clientX: 40, clientY: 30, pointerId: 1 });
    fireEvent.pointerUp(canvas, { clientX: 60, clientY: 40, pointerId: 1 });
    expect(screen.getByPlaceholderText("这里要改什么")).toBeTruthy();
    expect(screen.getByRole("button", { name: "附上" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "删除框" }));
    expect(screen.queryByPlaceholderText("这里要改什么")).toBeNull();
  });

  it("图读不出来时给提示,不留一块空画布", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: false, status: 404 })));
    render(<ImageAnnotator path="C:\\tmp\\gone.png" onDone={() => {}} onClose={() => {}} />);
    expect(await screen.findByText("图片读取失败，无法标注")).toBeTruthy();
  });
});
