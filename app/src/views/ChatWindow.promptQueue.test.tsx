import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { adapter, invoke } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import type { QueuedPromptDto } from "../api";
import { markRemoteUi } from "../remoteMode";
import { anchorFrom, outgoingStore } from "./chat/outgoingPrompts";
import { zh } from "../i18n/zh";

vi.mock("./ManagedTerminal", () => ({ ManagedTerminal: () => <div>PTY</div> }));

function queueHarness(initial: { status?: string; screenState?: string | null; busySubagents?: number } = {}) {
  markRemoteUi();
  let status = initial.status ?? "running";
  let queued: QueuedPromptDto[] = [];
  const base = adapter.setup(771, "claude", status);
  invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
    if (command === "get_chat_history") {
      return Promise.resolve({ ...adapter.history(771, "claude", status, initial.screenState ?? null), busySubagents: initial.busySubagents ?? 0 });
    }
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

  // 实拍 2026-11-19（会话 1823）：主回合已结束（DB waiting），2 个后台子代理还在跑，排队区挂着
  // 一条插话。宿主修好后屏幕发布 idle（输入框上方是「Waiting for 2 background agents to finish」），
  // 排队区这条要自动送出，用 Enter 直接提交，不是排队键；运行条只交代后台还在跑。
  it("主回合空闲、多个后台子代理在跑时，排队区已有的消息自动送出", async () => {
    const harness = queueHarness({ status: "waiting", screenState: "idle", busySubagents: 2 });
    harness.seed("held");
    render(<ChatWindow />);
    await waitFor(() => expect(harness.writes().some((text) => String(text).includes("卡住的消息"))).toBe(true), { timeout: 4_000 });
    await waitFor(() => expect(harness.state()).toBe("delivered"));
    expect(harness.writes()).toContain("\r");
    expect(document.body.textContent).not.toContain("等这一步完成后插入你的插话");
    await waitFor(() => expect(document.querySelector(".chat-running.is-background")?.textContent).toContain(zh.chat.backgroundOnlyRunning));
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

  // 2026-11-17 实拍：排队区里「未确认送达」的消息挂着 ↻ 重发，点了只会报「正文可能仍在终端输入框」——
  // retryOutgoing 对未确认的一律拒绝（CLI 可能已收下只是没轮到，重发会进两遍）。只给放回输入框。
  it("排队区里未确认送达的消息不给重发，只给放回输入框", async () => {
    queueHarness();
    outgoingStore.add(771, { kind: "queued", text: "继续", attachmentOnly: false, anchor: anchorFrom([], null), phase: "uncertain", bodyWritten: false, draft: { prompt: "继续", attachments: [] } });
    render(<ChatWindow />);
    await screen.findByText("继续", { selector: ".chat-queue-body" });
    const turn = document.querySelector(".chat-queued-turn")!;
    expect(turn.querySelector('[aria-label="重发"]')).toBeNull();
    expect(turn.querySelector('[aria-label="放回输入框"]')).not.toBeNull();
    outgoingStore.reset();
  });

  it("用户在运行中主动提前交付才写官方 TUI", async () => {
    const harness = queueHarness();
    render(<ChatWindow />);
    const box = await adapter.box();
    fireEvent.change(box, { target: { value: "现在看一眼" } });
    fireEvent.keyDown(box, { key: "Enter" });
    // 测试夹具的 Claude 声明下一步就读到排队输入：队首钮说「立即插入当前回合」（对照 Kimi Code）。
    const button = await screen.findByRole("button", { name: "立即插入当前回合" });
    expect(harness.writes()).toEqual([]);
    fireEvent.click(button);
    await waitFor(() => expect(harness.writes().some((text) => String(text).includes("现在看一眼"))).toBe(true));
    await waitFor(() => expect(harness.state()).toBe("delivered"));
  });
});
