import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { QueuedPromptDto } from "../../api";
import { promptQueueCancel, promptQueueEdit, promptQueueFinish } from "../../api";
import { QueuedPrompts } from "./QueuedPrompts";

vi.mock("../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api")>()),
  promptQueueEdit: vi.fn(),
  promptQueueCancel: vi.fn(),
  promptQueueFinish: vi.fn(),
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

function renderQueue(entry = item()) {
  const props = {
    sessionId: 42,
    items: [entry],
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
