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
});

const three = () => [
  item({ id: 1, draft: { prompt: "一", attachments: [] } }),
  item({ id: 2, draft: { prompt: "二", attachments: [] } }),
  item({ id: 3, draft: { prompt: "三", attachments: [] } }),
];
const grips = () => screen.getAllByRole("button", { name: "拖拽排序，或按上下方向键调整" });

describe("排队消息拖拽排序（对照 Kimi Code 客户端）", () => {
  it("把第 3 条拖到第 1 条上方：按新顺序整批提交", async () => {
    const props = renderQueue(three());
    vi.mocked(promptQueueReorder).mockResolvedValue(true);
    const rows = document.querySelectorAll(".chat-queued-turn");
    const dataTransfer = { setData: vi.fn(), effectAllowed: "", dropEffect: "" };
    fireEvent.dragStart(grips()[2], { dataTransfer });
    // jsdom 的行高是 0：落点 y=0 不在下半，算「上方」。
    fireEvent.dragOver(rows[0], { dataTransfer, clientY: 0 });
    expect(rows[0].classList.contains("drop-before")).toBe(true);
    fireEvent.drop(rows[0], { dataTransfer });
    await waitFor(() => expect(promptQueueReorder).toHaveBeenCalledWith(42, [3, 1, 2]));
    expect(props.onChanged).toHaveBeenCalled();
    expect(props.onError).not.toHaveBeenCalled();
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
