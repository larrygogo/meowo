import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { adapter, invoke } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import type { QueuedPromptDto } from "../api";
import { markRemoteUi } from "../remoteMode";

vi.mock("./ManagedTerminal", () => ({ ManagedTerminal: () => <div>PTY</div> }));

function queueHarness() {
  markRemoteUi();
  let status = "running";
  let queued: QueuedPromptDto[] = [];
  const base = adapter.setup(771, "claude", status);
  invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
    if (command === "get_chat_history") return Promise.resolve(adapter.history(771, "claude", status));
    if (command === "prompt_queue_list") return Promise.resolve(queued.filter((entry) => ["held", "delivering", "failed"].includes(entry.state)));
    if (command === "prompt_queue_enqueue") {
      const draft = args?.draft as QueuedPromptDto["draft"];
      const item: QueuedPromptDto = { id: 1, sessionId: 771, draft, state: "held", createdAt: 100, updatedAt: 100 };
      queued = [...queued, item];
      return Promise.resolve(item);
    }
    if (command === "prompt_queue_claim") {
      const found = queued.find((entry) => entry.id === args?.id && entry.state === "held");
      if (!found) return Promise.resolve(null);
      queued = queued.map((entry) => entry.id === found.id ? { ...entry, state: "delivering" } : entry);
      return Promise.resolve({ ...found, state: "delivering" });
    }
    if (command === "prompt_queue_finish") {
      queued = queued.map((entry) => entry.id === args?.id ? { ...entry, state: args.delivered ? "delivered" : "failed" } : entry);
      return Promise.resolve(true);
    }
    if (command === "prompt_queue_cancel") {
      const found = queued.find((entry) => entry.id === args?.id);
      if (!found || (found.state !== "held" && found.state !== "failed")) return Promise.resolve(false);
      queued = queued.map((entry) => entry.id === found.id ? { ...entry, state: "cancelled" } : entry);
      return Promise.resolve(true);
    }
    return base(command, args);
  });
  return {
    writes: () => adapter.writes(771),
    idle: () => { status = "waiting"; },
    seed: (state: string) => {
      queued = [{ id: 1, sessionId: 771, draft: { prompt: "卡住的消息", attachments: [] }, state, createdAt: 100, updatedAt: 100 }];
    },
    state: () => queued[0]?.state,
  };
}

describe("ChatWindow 持久队列", () => {
  it("回合结束后才领取并写入，结算后不再重复投递", async () => {
    const harness = queueHarness();
    render(<ChatWindow />);
    const box = await adapter.box();
    fireEvent.change(box, { target: { value: "回合结束后继续" } });
    fireEvent.keyDown(box, { key: "Enter" });
    await screen.findByText("回合结束后继续", { selector: ".chat-queue-body" });
    expect(harness.writes()).toEqual([]);
    expect(harness.state()).toBe("held");

    harness.idle();
    await waitFor(() => expect(harness.writes().some((text) => String(text).includes("回合结束后继续"))).toBe(true), { timeout: 4_000 });
    await waitFor(() => expect(harness.state()).toBe("delivered"));
    const delivered = harness.writes().filter((text) => String(text).includes("回合结束后继续")).length;
    await new Promise((resolve) => setTimeout(resolve, 800));
    expect(harness.writes().filter((text) => String(text).includes("回合结束后继续"))).toHaveLength(delivered);
  });

  it("卡在正在交给 Agent 的消息可放回输入框，不向 CLI 重发", async () => {
    const harness = queueHarness();
    harness.seed("delivering");
    render(<ChatWindow />);
    const restore = await screen.findByRole("button", { name: "放回输入框" });
    fireEvent.click(restore);
    await waitFor(() => expect(harness.state()).toBe("cancelled"));
    await waitFor(() => expect(screen.queryByText("卡住的消息", { selector: ".chat-queue-body" })).toBeNull(), { timeout: 4_000 });
    expect((await adapter.box()).value).toBe("卡住的消息");
    expect(harness.writes()).toEqual([]);
  });

  it("用户在运行中主动提前交付才写官方 TUI", async () => {
    const harness = queueHarness();
    render(<ChatWindow />);
    const box = await adapter.box();
    fireEvent.change(box, { target: { value: "现在看一眼" } });
    fireEvent.keyDown(box, { key: "Enter" });
    const button = await screen.findByRole("button", { name: "现在交给 Agent" });
    expect(harness.writes()).toEqual([]);
    fireEvent.click(button);
    await waitFor(() => expect(harness.writes().some((text) => String(text).includes("现在看一眼"))).toBe(true));
    await waitFor(() => expect(harness.state()).toBe("delivered"));
  });
});
