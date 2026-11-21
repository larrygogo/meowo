import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fitEdge, isHeic, prepareUpload, UploadPrepareError } from "./prepareUpload";

// jsdom 没有图片解码和 canvas 导出：createImageBitmap 按测试给尺寸；toBlob 交回一个带类型的假图。
let drawn: { width: number; height: number } | null = null;
beforeEach(() => {
  drawn = null;
  HTMLCanvasElement.prototype.getContext = function getContext() {
    const canvas = this as HTMLCanvasElement;
    return { drawImage: () => { drawn = { width: canvas.width, height: canvas.height }; }, imageSmoothingQuality: "low" } as unknown as CanvasRenderingContext2D;
  } as typeof HTMLCanvasElement.prototype.getContext;
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
