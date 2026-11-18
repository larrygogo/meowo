import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { QueuedPromptDto } from "../../api";
import { promptQueueCancel, promptQueueEdit, promptQueueFinish, promptQueueReorder } from "../../api";
import { QueuedPrompts } from "./QueuedPrompts";

vi.mock("../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api")>()),
  promptQueueEdit: vi.fn(),
  promptQueueCancel: vi.fn(),
  promptQueueFinish: vi.fn(),
  promptQueueReorder: vi.fn(),
}));

const item = (over: Partial<QueuedPromptDto> = {}): QueuedPromptDto => ({
  id: 1,
  sessionId: 42,
  draft: { prompt: "继续", attachments: [] },
  state: "held",
  createdAt: 100,
  updatedAt: 100,
  ...over,
});

function renderQueue(entry: QueuedPromptDto | QueuedPromptDto[] = item(), extra: { deliverLabel?: string | null } = {}) {
  const props = {
    sessionId: 42,
    items: Array.isArray(entry) ? entry : [entry],
    ...extra,
    busy: false,
    onDeliver: vi.fn(),
    onRetry: vi.fn(),
    onRestore: vi.fn(),
    onChanged: vi.fn(),
    onError: vi.fn(),
  };
  render(<QueuedPrompts {...props} />);
  return props;
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.restoreAllMocks();
});

const three = () => [
  item({ id: 1, draft: { prompt: "一", attachments: [] } }),
  item({ id: 2, draft: { prompt: "二", attachments: [] } }),
  item({ id: 3, draft: { prompt: "三", attachments: [] } }),
];
const grips = () => screen.getAllByRole("button", { name: "拖拽排序，或按上下方向键调整" });

describe("排队消息拖拽排序（对照 Kimi Code 客户端）", () => {
  function touchEvent(element: HTMLElement, type: string, y: number, pointerType = "touch", button = 0) {
    const event = new MouseEvent(type, { bubbles: true, clientX: 20, clientY: y, button });
    Object.defineProperties(event, { pointerId: { value: 1 }, pointerType: { value: pointerType }, isPrimary: { value: true } });
    fireEvent(element, event);
  }

  // Windows 上对话窗接管了系统拖放（文件拖入），网页内的 HTML 拖放不触发：鼠标也必须走指针事件。
  it("鼠标把第 3 条拖到第 1 条上方：按新顺序整批提交，拖动中标出落点", async () => {
    const props = renderQueue(three());
    vi.mocked(promptQueueReorder).mockResolvedValue(true);
    const handles = touchRows();
    expect(handles[2].getAttribute("draggable")).toBeNull();
    touchEvent(handles[2], "pointerdown", 250, "mouse");
    touchEvent(handles[2], "pointermove", 20, "mouse");
    expect(document.querySelectorAll(".chat-queued-turn")[0].classList.contains("drop-before")).toBe(true);
    touchEvent(handles[2], "pointerup", 20, "mouse");
    await waitFor(() => expect(promptQueueReorder).toHaveBeenCalledWith(42, [3, 1, 2]));
    expect(props.onChanged).toHaveBeenCalled();
    expect(props.onError).not.toHaveBeenCalled();
  });

  it("鼠标右键按住拖不算排序", () => {
    renderQueue(three());
    const handles = touchRows();
    touchEvent(handles[2], "pointerdown", 250, "mouse", 2);
    touchEvent(handles[2], "pointermove", 20, "mouse", 2);
    touchEvent(handles[2], "pointerup", 20, "mouse", 2);
    expect(promptQueueReorder).not.toHaveBeenCalled();
  });

  // U7：松手即按新顺序摆好，不等后端回执再跳；排序被拒时撤回原顺序。
  const order = () => [...document.querySelectorAll(".chat-queue-body")].map((el) => el.textContent);
  it("松手后不等后端回执，当帧就按新顺序摆放（无拖尾、不弹回）", async () => {
    let settle: (ok: boolean) => void = () => {};
    vi.mocked(promptQueueReorder).mockReturnValue(new Promise<boolean>((resolve) => { settle = resolve; }));
    renderQueue(three());
    const handles = touchRows();
    touchEvent(handles[2], "pointerdown", 250, "mouse");
    touchEvent(handles[2], "pointermove", 20, "mouse");
    touchEvent(handles[2], "pointerup", 20, "mouse");
    expect(order()).toEqual(["三", "一", "二"]);
    settle(true);
    await waitFor(() => expect(promptQueueReorder).toHaveBeenCalledWith(42, [3, 1, 2]));
    expect(order()).toEqual(["三", "一", "二"]);
  });
  it("排序被拒时撤回乐观顺序", async () => {
    vi.mocked(promptQueueReorder).mockResolvedValue(false);
    const props = renderQueue(three());
    const handles = touchRows();
    touchEvent(handles[2], "pointerdown", 250, "mouse");
    touchEvent(handles[2], "pointermove", 20, "mouse");
    touchEvent(handles[2], "pointerup", 20, "mouse");
    await waitFor(() => expect(props.onError).toHaveBeenCalledWith("queue/not_reorderable"));
    expect(order()).toEqual(["一", "二", "三"]);
  });

  function touchRows() {
    const handles = grips();
    for (const handle of handles) Object.defineProperty(handle, "setPointerCapture", { value: vi.fn(), configurable: true });
    const stack = document.querySelector(".chat-prompt-queue")!;
    vi.spyOn(stack, "getBoundingClientRect").mockReturnValue({ left: 0, right: 300, top: 0, bottom: 300 } as DOMRect);
    document.querySelectorAll(".chat-queued-turn").forEach((row, index) => {
      vi.spyOn(row, "getBoundingClientRect").mockReturnValue({ top: index * 100, bottom: (index + 1) * 100, height: 100 } as DOMRect);
    });
    return handles;
  }

  it("触摸手柄拖动提交新顺序，不依赖 HTML 拖放", async () => {
    renderQueue(three());
    vi.mocked(promptQueueReorder).mockResolvedValue(true);
    const handles = touchRows();
    touchEvent(handles[2], "pointerdown", 250);
    touchEvent(handles[2], "pointermove", 20);
    touchEvent(handles[2], "pointerup", 20);
    await waitFor(() => expect(promptQueueReorder).toHaveBeenCalledWith(42, [3, 1, 2]));
  });

  it.each(["pointercancel", "lostpointercapture"])("轻触或 %s 不改变队列", (cancelEvent) => {
    renderQueue(three());
    const handles = touchRows();
    touchEvent(handles[0], "pointerdown", 20);
    touchEvent(handles[0], "pointerup", 20);
    touchEvent(handles[0], "pointerdown", 20);
    touchEvent(handles[0], "pointermove", 250);
    touchEvent(handles[0], cancelEvent, 250);
    touchEvent(handles[0], "pointerup", 250);
    expect(promptQueueReorder).not.toHaveBeenCalled();
    expect(document.querySelector(".is-dragging")).toBeNull();
  });

  it.each([-20, 320])("拖出队列纵向边界 %i 后松手不提交", (endY) => {
    renderQueue(three());
    const handles = touchRows();
    touchEvent(handles[1], "pointerdown", 150);
    touchEvent(handles[1], "pointermove", 20);
    touchEvent(handles[1], "pointerup", endY);
    expect(promptQueueReorder).not.toHaveBeenCalled();
    expect(document.querySelector(".is-dragging")).toBeNull();
  });

  it("拖动期间队列被领取或切换会话时取消旧手势", () => {
    const props = {
      sessionId: 42, items: three(), busy: false,
      onDeliver: vi.fn(), onRetry: vi.fn(), onRestore: vi.fn(), onChanged: vi.fn(), onError: vi.fn(),
    };
    const { rerender } = render(<QueuedPrompts {...props} />);
    const handles = touchRows();
    touchEvent(handles[0], "pointerdown", 20);
    touchEvent(handles[0], "pointermove", 250);
    rerender(<QueuedPrompts {...props} sessionId={43} items={three().slice(0, 2)} />);
    touchEvent(handles[0], "pointerup", 250);
    expect(promptQueueReorder).not.toHaveBeenCalled();
    expect(document.querySelector(".is-dragging")).toBeNull();
  });

  it("忙碌时触摸或键盘都不排序", () => {
    render(<QueuedPrompts sessionId={42} items={three()} busy onDeliver={() => {}} onRetry={() => {}} onRestore={() => {}} onChanged={() => {}} onError={() => {}} />);
    const handles = touchRows();
    touchEvent(handles[0], "pointerdown", 20);
    touchEvent(handles[0], "pointermove", 250);
    touchEvent(handles[0], "pointerup", 250);
    fireEvent.keyDown(handles[0], { key: "ArrowDown" });
    expect(promptQueueReorder).not.toHaveBeenCalled();
  });

  it("键盘：手柄上按上方向键挪一格", async () => {
    renderQueue(three());
    vi.mocked(promptQueueReorder).mockResolvedValue(true);
    fireEvent.keyDown(grips()[1], { key: "ArrowUp" });
    await waitFor(() => expect(promptQueueReorder).toHaveBeenCalledWith(42, [2, 1, 3]));
    vi.mocked(promptQueueReorder).mockClear();
    // 已在队尾再往下：不动。
    fireEvent.keyDown(grips()[2], { key: "ArrowDown" });
    expect(promptQueueReorder).not.toHaveBeenCalled();
  });

  it("有一条刚被领走：顺序没调成，如实报错", async () => {
    const props = renderQueue(three());
    vi.mocked(promptQueueReorder).mockResolvedValue(false);
    fireEvent.keyDown(grips()[2], { key: "ArrowUp" });
    await waitFor(() => expect(props.onError).toHaveBeenCalledWith("queue/not_reorderable"));
  });

  it("只有一条在等、或正在投递的消息：没有手柄", () => {
    renderQueue([item({ id: 1 }), item({ id: 2, state: "delivering" })]);
    expect(screen.queryByRole("button", { name: "拖拽排序，或按上下方向键调整" })).toBeNull();
  });
});

describe("队首发送钮按这家 CLI 的处理方式说", () => {
  it("下一步就读到：写「立即插入当前回合」", () => {
    renderQueue(item(), { deliverLabel: "立即插入当前回合" });
    expect(screen.getByRole("button", { name: "立即插入当前回合" })).toBeTruthy();
  });

  it("运行中不收输入：不给发送钮", () => {
    renderQueue(item(), { deliverLabel: null });
    expect(screen.queryByRole("button", { name: "现在交给 Agent" })).toBeNull();
    expect(screen.queryByRole("button", { name: "立即插入当前回合" })).toBeNull();
  });
});

describe("持久排队消息", () => {
  it("保存正文与附件编辑后的唯一版本", async () => {
    const props = renderQueue(item({
      draft: {
        prompt: "旧正文",
        attachments: [{ path: "C:/a.png", name: "a.png", image: true }],
      },
    }));
    vi.mocked(promptQueueEdit).mockResolvedValue(true);
    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByRole("textbox", { name: "编辑排队消息" }), { target: { value: "新正文" } });
    fireEvent.click(screen.getByRole("button", { name: "移除附件" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(promptQueueEdit).toHaveBeenCalledWith(42, 1, { prompt: "新正文", attachments: [] }));
    expect(props.onError).not.toHaveBeenCalled();
  });

  it("取消已被领取的消息时如实报错", async () => {
    const props = renderQueue();
    vi.mocked(promptQueueCancel).mockResolvedValue(false);
    fireEvent.click(screen.getByRole("button", { name: "取消排队" }));
    await waitFor(() => expect(props.onError).toHaveBeenCalledWith("queue/not_cancellable"));
  });

  it("正在投递时不给编辑和取消排队入口，但发送卡死时仍可仅隐藏", async () => {
    const props = renderQueue(item({ state: "delivering" }));
    expect(screen.queryByRole("button", { name: "编辑" })).toBeNull();
    expect(screen.queryByRole("button", { name: "取消排队" })).toBeNull();
    expect(screen.queryByRole("button", { name: "现在交给 Agent" })).toBeNull();
    expect(props.onDeliver).not.toHaveBeenCalled();

    vi.mocked(promptQueueFinish).mockResolvedValue(true);
    vi.mocked(promptQueueCancel).mockResolvedValue(true);
    fireEvent.click(screen.getByRole("button", { name: "隐藏，不会取消" }));
    await waitFor(() => expect(promptQueueCancel).toHaveBeenCalledWith(42, 1));
    expect(promptQueueFinish).toHaveBeenCalledWith(42, 1, false);
    expect(props.onError).not.toHaveBeenCalled();
  });

  it("卡住的消息可放回输入框，但不自动重发", async () => {
    const props = renderQueue(item({ state: "delivering" }));
    vi.mocked(promptQueueFinish).mockResolvedValue(true);
    vi.mocked(promptQueueCancel).mockResolvedValue(true);
    fireEvent.click(screen.getByRole("button", { name: "放回输入框" }));
    await waitFor(() => expect(props.onRestore).toHaveBeenCalledWith(expect.objectContaining({ id: 1 })));
    expect(promptQueueFinish).toHaveBeenCalledWith(42, 1, false);
    expect(props.onRetry).not.toHaveBeenCalled();
    expect(props.onDeliver).not.toHaveBeenCalled();
  });

  it("投递失败可清理，不必先重发", async () => {
    const props = renderQueue(item({ state: "failed" }));
    vi.mocked(promptQueueCancel).mockResolvedValue(true);
    fireEvent.click(screen.getByRole("button", { name: "隐藏，不会取消" }));
    await waitFor(() => expect(promptQueueCancel).toHaveBeenCalledWith(42, 1));
    expect(promptQueueFinish).not.toHaveBeenCalled();
    expect(props.onRetry).not.toHaveBeenCalled();
    expect(props.onError).not.toHaveBeenCalled();
  });
});
