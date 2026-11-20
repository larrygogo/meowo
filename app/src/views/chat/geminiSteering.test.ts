/// Gemini 0.68.0 回合中途插话（modelSteering）：真机取证见 test/fixtures/gemini-steering.json。
///
/// 现象（2026-11-20 tests/probe_gemini_walk.rs，ping -n 45 在跑时送进第二句）：
/// - 屏幕上立刻多出一行 ` 💡 Steering Hint: <原话>`；
/// - 会话记录要等这一步的调用跑完才写这句（回车 07:10:09.737，落盘 07:10:54.608，隔了 44.9 秒），
///   提交 hook 也没有。对话页的确认时限是 15 秒，修前这段时间里这句被报成「未确认送达」。
/// 修法：插件把这一行的行首声明为 user_line_marker，超时后读一次画面，见到就标「已排队」。

import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatItem, ChatUi } from "../../api";

const api = vi.hoisted(() => ({
  managedTerminalScreen: vi.fn<(sessionId: number) => Promise<string[] | null>>(),
  writeManagedTerminal: vi.fn<(sessionId: number, data: string) => Promise<void>>(),
}));
vi.mock("../../api", () => api);

import { chatUi } from "../../test/agents";
import steering from "../../test/fixtures/gemini-steering.json";
import { shownAsLatestUserRow } from "./composer";
import { anchorFrom, ECHO_CONFIRM_MS, outgoingStore, reconcileOutgoing, type OutgoingPrompt } from "./outgoingPrompts";
import { useOutgoingUpkeep } from "./useOutgoingUpkeep";

const geminiUi = {
  composer: chatUi("gemini")!.composer,
  queued_hint: chatUi("gemini")!.queued_hint,
  user_line_marker: chatUi("gemini")!.user_line_marker,
} as unknown as ChatUi;

const sentAt = Date.parse(steering.sentAt);
const recordedAt = Date.parse(steering.recordedAt);
/// 发送那一刻对话页手里最后一条记录：ping 那一轮的用户消息（会话记录 07:06:47）。
const prior: ChatItem[] = [{ type: "user_text", id: "gemini-user-ping", timestamp: "2026-11-20T07:06:47.060Z", text: "Run this exact shell command and nothing else: ping -n 45 127.0.0.1" } as ChatItem];

const queued = (id: number, phase: OutgoingPrompt["phase"], at = sentAt): OutgoingPrompt => ({
  id, kind: "queued", text: steering.text, attachmentOnly: false, at, phase, settledAt: null, anchor: anchorFrom(prior, prior[0].type === "user_text" ? prior[0].text : null), bodyWritten: true,
});

afterEach(() => { cleanup(); vi.clearAllMocks(); outgoingStore.reset(); localStorage.clear(); });

describe("Gemini 插话：真机取证", () => {
  it("插话要等这一步跑完才落盘，比对话页的确认时限久", () => {
    expect(recordedAt - sentAt).toBe(44_871);
    expect(recordedAt - sentAt).toBeGreaterThan(ECHO_CONFIRM_MS);
  });

  it("只看会话记录：没落盘之前，超过时限就被判成未确认送达（这就是要靠画面补的那段）", () => {
    const [after] = reconcileOutgoing([queued(1, "pending")], { items: prior, acks: [], lastUserText: null, ended: false, now: sentAt + ECHO_CONFIRM_MS + 1 });
    expect(after.phase).toBe("uncertain");
  });

  it("插话那一行认得出：送进去之后的屏幕命中，送进去之前的屏幕不命中", () => {
    const marker = geminiUi.user_line_marker;
    expect(marker).toBe("💡 Steering Hint: ");
    expect(shownAsLatestUserRow(steering.steeringScreen, marker, steering.text)).toBe(true);
    expect(shownAsLatestUserRow(steering.runningScreen, marker, steering.text)).toBe(false);
    // 用户消息（` > …`）不当成插话行。
    expect(shownAsLatestUserRow(steering.runningScreen, marker, "Run this exact shell command")).toBe(false);
  });
});

describe("Gemini 插话：对话页读画面", () => {
  const mount = (outgoing: OutgoingPrompt[]) => renderHook(() => useOutgoingUpkeep({
    sessionId: 7, outgoing, loading: false, items: prior, acks: [], lastUserText: null, ended: false,
    chatUi: geminiUi, isActive: () => true, running: () => true, onError: () => {},
  }));

  it("超时还没落盘、屏幕上有插话行：标已排队，不报未确认送达", async () => {
    api.managedTerminalScreen.mockResolvedValue(steering.steeringScreen);
    const id = outgoingStore.add(7, { kind: "queued", text: steering.text, attachmentOnly: false, anchor: queued(0, "uncertain").anchor, phase: "uncertain", bodyWritten: true });
    const { result } = mount([{ ...queued(id, "uncertain"), at: Date.now() - ECHO_CONFIRM_MS - 1_000 }]);
    await waitFor(() => expect(outgoingStore.get(7)[0].phase).toBe("accepted"));
    expect(result.current.stuck.size).toBe(0);
  });

  it("屏幕上没有这句（还是送进去之前的画面）：照旧未确认，不凭空标已排队", async () => {
    api.managedTerminalScreen.mockResolvedValue(steering.runningScreen);
    const id = outgoingStore.add(7, { kind: "queued", text: steering.text, attachmentOnly: false, anchor: queued(0, "uncertain").anchor, phase: "uncertain", bodyWritten: true });
    mount([{ ...queued(id, "uncertain"), at: Date.now() - ECHO_CONFIRM_MS - 1_000 }]);
    await waitFor(() => expect(api.managedTerminalScreen).toHaveBeenCalled());
    await act(async () => {});
    expect(outgoingStore.get(7)[0].phase).toBe("uncertain");
  });
});
