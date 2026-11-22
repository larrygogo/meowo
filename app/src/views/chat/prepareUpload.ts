// 手机发照片：上传前把图改成模型收得下、传得动的样子。
// 手机原图长边常有 4000px 以上、几 MB，iPhone 还可能给 HEIC；五家模型只认 png/jpeg/gif/webp，
// 而且大图进模型前也会被缩到两千像素上下（多花的只是上传时间和流量）。
// 只在手机页用：桌面粘贴的截图保持原样，细节由用户自己决定。

/** 长边上限：与主流模型读图前的缩放档一致，再大模型也看不到更多细节。 */
export const UPLOAD_MAX_EDGE = 2048;
const JPEG_QUALITY = 0.85;

const HEIC = /\.(heic|heif)$/i;

export function isHeic(file: File): boolean {
  return /^image\/hei[cf]/i.test(file.type) || HEIC.test(file.name);
}

/** 缩放后的尺寸；不用缩时原样返回。 */
export function fitEdge(width: number, height: number, max = UPLOAD_MAX_EDGE): { width: number; height: number } {
  const edge = Math.max(width, height);
  if (edge <= max) return { width, height };
  const ratio = max / edge;
  return { width: Math.max(1, Math.round(width * ratio)), height: Math.max(1, Math.round(height * ratio)) };
}

export class UploadPrepareError extends Error {
  constructor(readonly code: "heic_unsupported") {
    super(code);
  }
}

/// 要处理的才处理：GIF 不碰（会丢动画），非图片不碰，小于上限的 png/jpeg/webp 不碰。
/// HEIC 解不开（浏览器不支持）时抛 UploadPrepareError，别把模型读不了的格式发过去；
/// 其他格式解不开就原样上传，交给后端和 CLI 按原来的路径处理。
export async function prepareUpload(file: File): Promise<File> {
  const heic = isHeic(file);
  const image = heic || /^image\/(png|jpe?g|webp|bmp)$/i.test(file.type);
  if (!image) return file;
  let bitmap: ImageBitmap;
  try {
    // 显式按 EXIF 方向转正：缩小后 EXIF 就没了，旧版 Safari 默认不转，竖拍会变横。
    bitmap = await createImageBitmap(file, { imageOrientation: "from-image" });
  } catch {
    if (heic) throw new UploadPrepareError("heic_unsupported");
    return file;
  }
  try {
    const size = fitEdge(bitmap.width, bitmap.height);
    const resize = size.width !== bitmap.width || size.height !== bitmap.height;
    if (!resize && !heic) return file;
    const canvas = document.createElement("canvas");
    canvas.width = size.width;
    canvas.height = size.height;
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      if (heic) throw new UploadPrepareError("heic_unsupported");
      return file;
    }
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(bitmap, 0, 0, size.width, size.height);
    // 截图（png）缩小后仍用 png，文字边缘不糊；可能带透明的 webp/bmp 也用 png（转 jpeg 透明处会变黑）；
    // 照片（jpeg、HEIC）用 jpeg。
    const type = !heic && /png|webp|bmp/i.test(file.type) ? "image/png" : "image/jpeg";
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, type, JPEG_QUALITY));
    if (!blob) {
      if (heic) throw new UploadPrepareError("heic_unsupported");
      return file;
    }
    const base = (file.name || "photo").replace(/\.[^.]+$/, "");
    return new File([blob], `${base}.${type === "image/png" ? "png" : "jpg"}`, { type });
  } finally {
    bitmap.close?.();
  }
}

/// 一次只处理一张：一次选十张 1200 万像素的照片，同时解码要近 500MB，iOS 会直接杀掉页面（做空）。
let queue: Promise<unknown> = Promise.resolve();
export function prepareUploadQueued(file: File): Promise<File> {
  const run = queue.then(() => prepareUpload(file), () => prepareUpload(file));
  queue = run.catch(() => undefined);
  return run;
}
