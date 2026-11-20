import { existsSync } from "node:fs";
import { join } from "node:path";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatItem } from "../../api";
import { reduceChatEvents } from "../../chat/reducer";
import { REPLAY_AGENTS, replaySamples, type ReplayAgent } from "../../test/replaySamples";
import { resetChatImageCache } from "./ChatImage";
import { ChatCwdContext } from "./chatCwd";
import { resetFoldMemory } from "./foldMemory";
import { Transcript } from "./Transcript";

/// 五家发图的端到端回放（验收 #3 #5 #8）：真机探针抓到的会话记录 → 后端真实解析器（replay_tests.rs 产出的
/// `<家>.<会话>.items.json`，不手写、不手改）→ 归并 → 对话页组件。证明的是「五家 CLI 发的图在对话页里是一张
/// agent 发来的图片消息」，桌面与手机都成立；截图、读图类回执在工具行上直接露缩略图。
///
/// 回执里的图片路径指向探针时的持久目录（`C:\probe\images\sent\…`）或单测的图片目录，在这台机器上不存在：
/// convertFileSrc 统一映射到仓库里的一张真图，但传进去的路径必须就是回执引用行里的那个。

const REAL_IMAGE = join(__dirname, "../../../src-tauri/icons/128x128@2x.png");
const shown = vi.hoisted(() => [] as string[]);
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  convertFileSrc: (path: string) => {
    shown.push(path);
    return `file:///${REAL_IMAGE.replace(/\\/g, "/")}`;
  },
}));
const saveDialog = vi.hoisted(() => vi.fn(async () => null as string | null));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: saveDialog, open: vi.fn() }));

const CWD = "C:\\probe\\work";
const IMAGE_REF = /\[Image: source: ([^\]]+?)\]/g;
type ToolUse = Extract<ChatItem, { type: "tool_use" }>;
type ToolResult = Extract<ChatItem, { type: "tool_result" }>;

/** 界面上不该出现的东西：工具原名、Meowo 内部路径、CLI 的模板文字与包装标签。 */
const LEAKS = ["send_image", "browser_screenshot", "[Image", "untrusted_context", "provided the following image data", "C:\\probe", ".meowo", "{}"];

/** 一家里含发图调用的样本，以及含截图、读图类回执（非发图调用的回执带图片引用行）的样本。 */
function imageSamples(agent: ReplayAgent) {
  return replaySamples(agent).flatMap((sample) => {
    const results = new Map(sample.items.filter((item): item is ToolResult => item.type === "tool_result").map((item) => [item.tool_use_id, item]));
    const calls = sample.items.filter((item): item is ToolUse => item.type === "tool_use");
    const sends = calls.filter((call) => call.kind === "image").map((call) => ({ call, result: results.get(call.id) }));
    const receipts = calls
      .filter((call) => call.kind !== "image" && !!results.get(call.id)?.text.match(IMAGE_REF))
      .map((call) => ({ call, result: results.get(call.id)! }));
    return sends.length + receipts.length > 0 ? [{ ...sample, sends, receipts }] : [];
  });
}

const SENT = REPLAY_AGENTS.flatMap((agent) => imageSamples(agent).filter((sample) => sample.sends.length > 0).map((sample) => [agent, sample.name] as const));
const RECEIPTS = REPLAY_AGENTS.flatMap((agent) => imageSamples(agent).filter((sample) => sample.receipts.length > 0).map((sample) => [agent, sample.name] as const));

function sampleOf(agent: ReplayAgent, name: string) {
  return imageSamples(agent).find((sample) => sample.name === name)!;
}

function renderSample(items: ChatItem[]) {
  const view = render(
    <ChatCwdContext.Provider value={CWD}>
      <Transcript sessionId={4242} items={reduceChatEvents([], items, true)} />
    </ChatCwdContext.Provider>,
  );
  return view;
}

/** 让 <img> 「读到」尺寸：jsdom 不解码图片，按一张 1280×800 的截图给。 */
function loadImages(root: ParentNode) {
  for (const img of root.querySelectorAll("img")) {
    Object.defineProperty(img, "naturalWidth", { value: 1280, configurable: true });
    Object.defineProperty(img, "naturalHeight", { value: 800, configurable: true });
    fireEvent.load(img);
  }
}

afterEach(() => {
  cleanup();
  resetFoldMemory();
  resetChatImageCache();
  shown.length = 0;
  saveDialog.mockClear();
  delete (globalThis as Record<string, unknown>).__MEOWO_REMOTE__;
});

describe("五家发图回放：真机记录 → 真实解析 → 对话页", () => {
  it("映射用的真图在仓库里", () => {
    expect(existsSync(REAL_IMAGE)).toBe(true);
  });

  it("五家都有发图的真机样本", () => {
    expect([...new Set(SENT.map(([agent]) => agent))].sort()).toEqual([...REPLAY_AGENTS].sort());
  });

  describe.each([
    ["桌面", false],
    ["手机", true],
  ] as const)("%s", (_layout, mobile) => {
    it.each(SENT)("%s（%s）：显示成 agent 发来的图片消息，带说明，点开有灯箱与保存", async (agent, name) => {
      if (mobile) (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = true;
      const sample = sampleOf(agent, name);
      const { container } = renderSample(sample.items);
      const messages = [...container.querySelectorAll<HTMLElement>(".chat-sent")];
      expect(messages).toHaveLength(sample.sends.length);
      for (const [index, { call, result }] of sample.sends.entries()) {
        const message = messages[index];
        // 不是收起的工具行：不在 <details> 里，也不在过程块里。
        expect(message.closest("details")).toBeNull();
        expect(message.closest(".turn-fold")).toBeNull();
        const img = message.querySelector<HTMLImageElement>("img.chat-sent-img");
        expect(img, `${name} 没有图`).toBeTruthy();
        expect(message.querySelector(".chat-sent-failed")).toBeNull();
        // 图来自回执里的引用行（真实解析结果），不是别处。
        const refs = [...(result?.text ?? "").matchAll(IMAGE_REF)].map((match) => match[1].trim());
        expect(refs.length).toBeGreaterThan(0);
        expect(shown).toEqual(expect.arrayContaining(refs));
        // 说明取自解析出的 detail.caption。
        const caption = call.detail?.kind === "image" ? call.detail.caption : null;
        if (caption) expect(message.querySelector(".chat-sent-caption")?.textContent).toBe(caption.trim());
        for (const leak of LEAKS) expect(message.textContent, `${name} 露出了 ${leak}`).not.toContain(leak);
        loadImages(message);
        fireEvent.click(message.querySelector(".chat-sent-open")!);
        const box = document.querySelector<HTMLElement>(".chat-lightbox");
        expect(box, `${name} 点开没有灯箱`).toBeTruthy();
        if (caption) expect(box!.querySelector(".chat-lightbox-caption")?.textContent).toBe(caption.trim());
        expect(screen.getAllByRole("button", { name: "保存" }).length).toBeGreaterThan(0);
        if (mobile) {
          expect(screen.queryByRole("button", { name: "在文件夹中显示" })).toBeNull();
          expect(document.body.textContent).not.toContain("在文件夹中显示");
        } else {
          expect(screen.getAllByRole("button", { name: "在文件夹中显示" }).length).toBeGreaterThan(0);
        }
        await act(async () => { fireEvent.keyDown(document, { key: "Escape" }); });
      }
      // 整条时间线也不露原始工具名与模板文字。
      for (const leak of ["untrusted_context", "provided the following image data", "[Image"]) {
        expect(container.textContent, `${name} 露出了 ${leak}`).not.toContain(leak);
      }
    });

    it.each(RECEIPTS)("%s（%s）：截图、读图类回执在工具行上直接露缩略图，不用展开", (agent, name) => {
      if (mobile) (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = true;
      const sample = sampleOf(agent, name);
      const { container } = renderSample(sample.items);
      // 过程块收着时，摘要行下也露缩略图（FoldImageStrip）。
      const refs = sample.receipts.flatMap(({ result }) => [...result.text.matchAll(IMAGE_REF)].map((match) => match[1].trim()));
      const thumbs = container.querySelectorAll(".chat-tool-media img");
      expect(thumbs.length, `${name} 没有缩略图`).toBeGreaterThan(0);
      expect(shown).toEqual(expect.arrayContaining(refs));
      for (const thumb of thumbs) expect(thumb.closest("details:not([open])")).toBeNull();
      // 展开过程块后，工具行本身也露（不在收起的 <details> 正文里），摘要与输出不露原名、模板与路径。
      for (let guard = 0; guard < 20; guard += 1) {
        const closed = container.querySelector<HTMLElement>(".turn-fold:not(.is-open) > .turn-fold-head");
        if (!closed) break;
        fireEvent.click(closed);
      }
      const tools = [...container.querySelectorAll<HTMLElement>("details.chat-tool")];
      expect(tools.length).toBeGreaterThan(0);
      for (const tool of tools) {
        const head = tool.querySelector("summary")?.textContent ?? "";
        for (const leak of LEAKS) expect(head, `${name} 工具行露出了 ${leak}`).not.toContain(leak);
      }
      // 缩略图紧跟在工具行后面（<details> 之外），不在收起的正文里。
      const rowMedia = tools.flatMap((tool) => (tool.nextElementSibling?.matches(".chat-tool-media") ? [...tool.nextElementSibling.querySelectorAll("img")] : []));
      expect(rowMedia.length).toBe(refs.length);
      for (const leak of ["untrusted_context", "provided the following image data", "[Image"]) {
        expect(container.textContent, `${name} 露出了 ${leak}`).not.toContain(leak);
      }
    });
  });
});
