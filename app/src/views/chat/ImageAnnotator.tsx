import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { AppModal } from "../AppModal";
import { useT } from "../../i18n";

/** 一个框：坐标按原图像素记，画到屏幕上再按缩放比换算。 */
type Box = { x: number; y: number; w: number; h: number; note: string };

const BOX_COLOR = "#ff3b30";
/** 小于这个边长（原图像素）的拖动算误点，不成框。 */
const MIN_BOX = 6;

/// 截图标注：在图上拖出几个框，每个框写一句话，完成后把框和编号画进图里另存一张，
/// 文字按编号交给输入框（对照 Kimi 桌面端的「截图框选加评论」）。图和字分开给，
/// agent 看图时找得到编号，读字时知道指的是哪块。
export function ImageAnnotator({ path, onDone, onClose }: {
  path: string;
  /** boxes 为空 = 用户没画框，调用方照原图处理。 */
  onDone: (result: { png: Blob | null; notes: string[] }) => void;
  onClose: () => void;
}) {
  const t = useT();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [image, setImage] = useState<ImageBitmap | null>(null);
  const [failed, setFailed] = useState(false);
  const [boxes, setBoxes] = useState<Box[]>([]);
  const [draft, setDraft] = useState<Box | null>(null);
  const [saving, setSaving] = useState(false);
  const dragRef = useRef<{ x: number; y: number } | null>(null);

  // 走 asset 协议读回原图：协议带同源 CORS 头，画进 canvas 不会被污染，toBlob 才导得出来。
  // fetch 归 CSP 的 connect-src 管（<img> 归 img-src），tauri.conf.json 里两处都要放行 asset。
  // 0.6.16 只放了 img-src，正式版标注窗一律「图片读取失败」；dev 走 devUrl 不带 CSP，测不出来。
  useEffect(() => {
    let cancelled = false;
    fetch(convertFileSrc(path))
      .then((response) => (response.ok ? response.blob() : Promise.reject(new Error(String(response.status)))))
      .then((blob) => createImageBitmap(blob))
      .then((bitmap) => { if (!cancelled) setImage(bitmap); })
      .catch(() => { if (!cancelled) setFailed(true); });
    return () => { cancelled = true; };
  }, [path]);

  // 屏幕上的画布按可用区域等比缩小；框的坐标始终是原图像素。
  const scale = image ? Math.min(1, 760 / image.width, (window.innerHeight * 0.62) / image.height) : 1;
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !image) return;
    canvas.width = Math.round(image.width * scale);
    canvas.height = Math.round(image.height * scale);
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.drawImage(image, 0, 0, canvas.width, canvas.height);
    drawBoxes(ctx, draft ? [...boxes, draft] : boxes, scale);
  }, [image, boxes, draft, scale]);

  const toImage = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    return { x: (event.clientX - rect.left) / scale, y: (event.clientY - rect.top) / scale };
  };
  const onPointerDown = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    if (!image || event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = toImage(event);
  };
  const onPointerMove = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    const start = dragRef.current;
    if (!start) return;
    const end = toImage(event);
    setDraft(normalizeBox(start, end));
  };
  const onPointerUp = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    const start = dragRef.current;
    dragRef.current = null;
    setDraft(null);
    if (!start) return;
    const box = normalizeBox(start, toImage(event));
    if (box.w >= MIN_BOX && box.h >= MIN_BOX) setBoxes((list) => [...list, box]);
  };

  const finish = async () => {
    if (!image || boxes.length === 0) {
      onDone({ png: null, notes: [] });
      return;
    }
    setSaving(true);
    // 导出用原图尺寸重画一遍，不用屏幕上缩小过的画布：小字和细节不能因为标注变糊。
    const canvas = document.createElement("canvas");
    canvas.width = image.width;
    canvas.height = image.height;
    const ctx = canvas.getContext("2d");
    if (!ctx) { setSaving(false); return; }
    ctx.drawImage(image, 0, 0);
    drawBoxes(ctx, boxes, 1);
    const png = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
    setSaving(false);
    onDone({ png, notes: boxes.map((box) => box.note.trim()) });
  };

  return (
    <AppModal label={t.chat.annotateTitle} className="chat-annotator" onClose={onClose}>
      <div className="chat-modal-title">{t.chat.annotateTitle}</div>
      {failed ? (
        <div className="chat-modal-error">{t.chat.annotateLoadFailed}</div>
      ) : (
        <>
          <div className="chat-annotator-hint">{t.chat.annotateHint}</div>
          <div className="chat-annotator-stage">
            <canvas
              ref={canvasRef}
              aria-label={t.chat.annotateTitle}
              onPointerDown={onPointerDown}
              onPointerMove={onPointerMove}
              onPointerUp={onPointerUp}
              onPointerCancel={() => { dragRef.current = null; setDraft(null); }}
            />
          </div>
          {boxes.length > 0 && (
            <ol className="chat-annotator-notes">
              {boxes.map((box, index) => (
                <li key={index}>
                  <span className="chat-annotator-num">{index + 1}</span>
                  <input
                    type="text"
                    value={box.note}
                    placeholder={t.chat.annotateNotePlaceholder}
                    autoFocus={index === boxes.length - 1}
                    onChange={(event) => {
                      const note = event.target.value;
                      setBoxes((list) => list.map((item, i) => (i === index ? { ...item, note } : item)));
                    }}
                  />
                  <button type="button" aria-label={t.chat.annotateRemove} data-tip={t.chat.annotateRemove} onClick={() => setBoxes((list) => list.filter((_, i) => i !== index))}>×</button>
                </li>
              ))}
            </ol>
          )}
        </>
      )}
      <div className="chat-modal-actions">
        <button type="button" className="ns-btn" onClick={onClose}>{t.chat.annotateCancel}</button>
        <button type="button" className="ns-btn is-primary" disabled={!image || saving} onClick={() => void finish()}>
          {boxes.length > 0 ? t.chat.annotateDone : t.chat.annotateSkip}
        </button>
      </div>
    </AppModal>
  );
}

function normalizeBox(a: { x: number; y: number }, b: { x: number; y: number }): Box {
  return { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(a.x - b.x), h: Math.abs(a.y - b.y), note: "" };
}

/// 框和编号的画法。线宽与字号按原图短边定，导出的大图上也看得清；scale 只在屏幕预览时
/// 把这些尺寸一起缩小，保证预览和导出长得一样。
function drawBoxes(ctx: CanvasRenderingContext2D, boxes: readonly Box[], scale: number) {
  const base = Math.max(2, Math.round(Math.min(ctx.canvas.width / scale, ctx.canvas.height / scale) / 300));
  const line = base * scale;
  const badge = base * 9 * scale;
  ctx.save();
  boxes.forEach((box, index) => {
    const x = box.x * scale;
    const y = box.y * scale;
    ctx.strokeStyle = BOX_COLOR;
    ctx.lineWidth = line;
    ctx.strokeRect(x, y, box.w * scale, box.h * scale);
    // 编号贴在框的左上角外侧；贴着图顶时放进框里，不然会被裁掉。
    const bx = x;
    const by = y - badge >= 0 ? y - badge : y;
    ctx.fillStyle = BOX_COLOR;
    ctx.fillRect(bx, by, badge, badge);
    ctx.fillStyle = "#fff";
    ctx.font = `600 ${Math.round(badge * 0.7)}px system-ui, sans-serif`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(String(index + 1), bx + badge / 2, by + badge / 2);
  });
  ctx.restore();
}
