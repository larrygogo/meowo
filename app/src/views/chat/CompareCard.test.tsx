import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatItem } from "../../api";
import { parseCompareReceipt } from "./CompareCard";
import { ToolActivity } from "./ToolActivity";
import { TurnLiveContext } from "./toolState";
import { Transcript } from "./Transcript";

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  convertFileSrc: (path: string) => `asset://local/${path}`,
}));

afterEach(cleanup);

const DIR = "C:\\Users\\me\\.meowo\\images\\sent\\";
const call = (id: string): Extract<ChatItem, { type: "tool_use" }> =>
  ({ type: "tool_use", id, timestamp: null, name: "mcp__meowo__compare_images", kind: "other", summary: "" } as Extract<ChatItem, { type: "tool_use" }>);
const result = (id: string, text: string, isError = false): Extract<ChatItem, { type: "tool_result" }> =>
  ({ type: "tool_result", id: `r-${id}`, timestamp: null, tool_use_id: id, text, is_error: isError });
const receipt = (summary: Record<string, unknown>, diff = true) => [
  "对照已发给用户……",
  "变化：0.80%（40 / 5000 像素）",
  `[Image: source: ${DIR}before.png]`,
  `[Image: source: ${DIR}after.png]`,
  ...(diff ? [`[Image: source: ${DIR}diff.png]`] : []),
  `[Image compare: ${JSON.stringify(summary)}]`,
].join("\n");
const CHANGED = { changed: "0.80%", before: [100, 50], after: [100, 50], bounds: [30, 20, 10, 4], caption: "按钮换成蓝色] 加高" };

describe("对照回执", () => {
  it("取出改前、改后、差异和摘要；说明里有 ] 也不截断", () => {
    const parsed = parseCompareReceipt(receipt(CHANGED))!;
    expect(parsed.before).toBe(`${DIR}before.png`);
    expect(parsed.after).toBe(`${DIR}after.png`);
    expect(parsed.diff).toBe(`${DIR}diff.png`);
    expect(parsed.summary).toEqual({ changed: "0.80%", before: [100, 50], after: [100, 50], bounds: [30, 20, 10, 4], caption: "按钮换成蓝色] 加高" });
  });

  it("没有变化时没有差异图；不是对照回执或摘要坏了返回 null", () => {
    const same = parseCompareReceipt(receipt({ changed: "0%", before: [20, 20], after: [20, 20], bounds: null, caption: null }, false))!;
    expect(same.diff).toBeNull();
    expect(same.summary.bounds).toBeNull();
    expect(parseCompareReceipt(`[Image: source: ${DIR}a.png]`)).toBeNull();
    expect(parseCompareReceipt(`[Image: source: ${DIR}a.png]\n[Image: source: ${DIR}b.png]\n[Image compare: {坏了]`)).toBeNull();
  });
});

describe("对照卡", () => {
  const loadAll = () => document.querySelectorAll(".chat-compare img").forEach((img) => {
    Object.defineProperty(img, "naturalWidth", { value: 100 });
    Object.defineProperty(img, "naturalHeight", { value: 50 });
    fireEvent.load(img);
  });

  it("按统一工具名显示成对照卡：改前改后叠放、红框圈出变化范围、变化比例和说明", () => {
    render(<ToolActivity item={call("c1")} result={result("c1", receipt(CHANGED))} />);
    const imgs = [...document.querySelectorAll(".chat-compare-img")].map((img) => img.getAttribute("src"));
    expect(imgs).toEqual([`asset://local/${DIR}before.png`, `asset://local/${DIR}after.png`]);
    const box = document.querySelector(".chat-compare-box") as HTMLElement;
    expect(box.style.left).toBe("30%");
    expect(box.style.top).toBe("40%");
    expect(box.style.width).toBe("10%");
    expect(screen.getByText("变化 0.80%")).toBeTruthy();
    expect(screen.getByText("范围 10×4")).toBeTruthy();
    expect(screen.getByText("按钮换成蓝色] 加高")).toBeTruthy();
    expect(document.querySelector("details.chat-tool")).toBeNull();
  });

  it("键盘左右移动把手；切到差异显示差异图", () => {
    render(<ToolActivity item={call("c2")} result={result("c2", receipt(CHANGED))} />);
    const slider = screen.getByRole("slider");
    expect(slider.getAttribute("aria-valuenow")).toBe("50");
    fireEvent.keyDown(slider, { key: "ArrowLeft", shiftKey: true });
    expect(slider.getAttribute("aria-valuenow")).toBe("40");
    expect((document.querySelector(".chat-compare-after") as HTMLElement).style.clipPath).toBe("inset(0 0 0 40%)");
    fireEvent.click(screen.getByRole("button", { name: "差异" }));
    expect(document.querySelector(".chat-compare-img")?.getAttribute("src")).toBe(`asset://local/${DIR}diff.png`);
  });

  it("点一下（不拖）打开大图，三张可翻：改前、改后、差异", () => {
    render(<ToolActivity item={call("c3")} result={result("c3", receipt(CHANGED))} />);
    loadAll();
    const stage = document.querySelector(".chat-compare-stage")!;
    fireEvent.pointerDown(stage, { button: 0, pointerId: 1, clientX: 10 });
    fireEvent.pointerUp(stage, { pointerId: 1, clientX: 11 });
    expect(document.querySelector(".chat-lightbox")).toBeTruthy();
    expect(screen.getByText("1 / 3")).toBeTruthy();
    expect(document.querySelector(".chat-lightbox-caption")?.textContent).toBe("改前");
  });

  it("没有变化：写「没有变化」，不给差异切换和红框", () => {
    render(<ToolActivity item={call("c4")} result={result("c4", receipt({ changed: "0%", before: [20, 20], after: [20, 20], bounds: null, caption: null }, false))} />);
    expect(screen.getByText("没有变化")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "差异" })).toBeNull();
    expect(document.querySelector(".chat-compare-box")).toBeNull();
  });

  it("失败回执按原因给人话；没有回执时是正在对比", () => {
    const { unmount } = render(<ToolActivity item={call("c5")} result={result("c5", "[Image error: not_found] 找不到文件：C:\\a.png", true)} />);
    expect(screen.getByText("对照没能发过来")).toBeTruthy();
    unmount();
    render(<TurnLiveContext.Provider value="running"><ToolActivity item={call("c6")} /></TurnLiveContext.Provider>);
    expect(screen.getByText("正在对比图片…")).toBeTruthy();
  });

  it("排在正文里，不收进过程块", () => {
    const items: ChatItem[] = [
      { type: "user_text", id: "u", timestamp: null, text: "改按钮" },
      { type: "tool_use", id: "b1", timestamp: null, name: "Bash", kind: "run", summary: "npm run build" } as ChatItem,
      result("b1", "ok"),
      call("c7"), result("c7", receipt(CHANGED)),
      { type: "assistant_text", id: "a", timestamp: null, text: "好了" },
    ];
    render(<Transcript sessionId={1} items={items} />);
    const card = document.querySelector(".chat-compare")!;
    expect(card).toBeTruthy();
    expect(card.closest(".turn-fold-body, details")).toBeNull();
  });
});
