import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fitEdge, isHeic, prepareUpload, prepareUploadQueued, UploadPrepareError } from "./prepareUpload";

// jsdom 没有图片解码和 canvas 导出：createImageBitmap 按测试给尺寸；toBlob 交回一个带类型的假图。
let drawn: { width: number; height: number } | null = null;
beforeEach(() => {
  drawn = null;
  HTMLCanvasElement.prototype.getContext = function getContext(this: HTMLCanvasElement) {
    const canvas = this as HTMLCanvasElement;
    return { drawImage: () => { drawn = { width: canvas.width, height: canvas.height }; }, imageSmoothingQuality: "low" } as unknown as CanvasRenderingContext2D;
  } as unknown as typeof HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.toBlob = function toBlob(callback: BlobCallback, type?: string) {
    callback(new Blob(["x"], { type: type ?? "image/png" }));
  };
});
afterEach(() => vi.unstubAllGlobals());

const bitmap = (width: number, height: number) => vi.fn(async () => ({ width, height, close: () => {} }));

describe("手机发照片前的处理", () => {
  it("长边超过 2048 的按比例缩到 2048", () => {
    expect(fitEdge(4032, 3024)).toEqual({ width: 2048, height: 1536 });
    expect(fitEdge(1170, 2532)).toEqual({ width: 946, height: 2048 });
    expect(fitEdge(1200, 800)).toEqual({ width: 1200, height: 800 });
  });

  it("大照片缩小并转成 jpg；小图、GIF、非图片原样", async () => {
    vi.stubGlobal("createImageBitmap", bitmap(4032, 3024));
    const big = await prepareUpload(new File(["a"], "IMG_1.jpeg", { type: "image/jpeg" }));
    expect(big.name).toBe("IMG_1.jpg");
    expect(big.type).toBe("image/jpeg");
    expect(drawn).toEqual({ width: 2048, height: 1536 });

    vi.stubGlobal("createImageBitmap", bitmap(800, 600));
    const small = new File(["a"], "s.jpg", { type: "image/jpeg" });
    expect(await prepareUpload(small)).toBe(small);
    const gif = new File(["a"], "a.gif", { type: "image/gif" });
    expect(await prepareUpload(gif)).toBe(gif);
    const text = new File(["a"], "a.txt", { type: "text/plain" });
    expect(await prepareUpload(text)).toBe(text);
  });

  it("按 EXIF 方向解码；可能带透明的 webp 缩小后用 png，不变黑底", async () => {
    const decode = bitmap(4000, 3000);
    vi.stubGlobal("createImageBitmap", decode);
    const out = await prepareUpload(new File(["a"], "sticker.webp", { type: "image/webp" }));
    expect(decode).toHaveBeenCalledWith(expect.any(File), { imageOrientation: "from-image" });
    expect(out.type).toBe("image/png");
    expect(out.name).toBe("sticker.png");
  });

  it("多张照片一张一张处理，不同时解码", async () => {
    let active = 0;
    let peak = 0;
    vi.stubGlobal("createImageBitmap", vi.fn(async () => {
      active += 1;
      peak = Math.max(peak, active);
      await new Promise((resolve) => setTimeout(resolve, 5));
      active -= 1;
      return { width: 4000, height: 3000, close: () => {} };
    }));
    const files = Array.from({ length: 4 }, (_, i) => new File(["a"], `p${i}.jpg`, { type: "image/jpeg" }));
    const out = await Promise.all(files.map((file) => prepareUploadQueued(file)));
    expect(out.map((file) => file.name)).toEqual(["p0.jpg", "p1.jpg", "p2.jpg", "p3.jpg"]);
    expect(peak).toBe(1);
  });

  it("截图（png）缩小后仍是 png，文字不糊", async () => {
    vi.stubGlobal("createImageBitmap", bitmap(2532, 1170));
    const out = await prepareUpload(new File(["a"], "shot.png", { type: "image/png" }));
    expect(out.type).toBe("image/png");
    expect(out.name).toBe("shot.png");
  });

  it("HEIC 小图也转 jpg；浏览器解不开时报出来，不把模型读不了的格式发过去", async () => {
    vi.stubGlobal("createImageBitmap", bitmap(1000, 800));
    const heic = new File(["a"], "IMG_2.HEIC", { type: "" });
    expect(isHeic(heic)).toBe(true);
    const out = await prepareUpload(heic);
    expect(out.name).toBe("IMG_2.jpg");
    expect(out.type).toBe("image/jpeg");

    vi.stubGlobal("createImageBitmap", vi.fn(async () => { throw new Error("decode"); }));
    await expect(prepareUpload(new File(["a"], "IMG_3.heic", { type: "image/heic" }))).rejects.toBeInstanceOf(UploadPrepareError);
    const odd = new File(["a"], "broken.jpg", { type: "image/jpeg" });
    expect(await prepareUpload(odd)).toBe(odd);
  });
});
