import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
const api = vi.hoisted(() => ({ managedConversationRecoveries: vi.fn() }));
vi.mock("../../api", async (original) => ({ ...await original<typeof import("../../api")>(), ...api }));
import { ManagedRecoveries, mergeRecoveryChunks } from "./ManagedRecoveries";
import type { ManagedNativeRecoveryChunkDto } from "../../generated/contracts/ManagedNativeRecoveryChunkDto";
const chunk = (text: string, textByteOffset: number, complete: boolean): ManagedNativeRecoveryChunkDto => ({ sequence: 7, createdAt: 1, messageIndex: 0, role: "assistant", text, textByteOffset, complete });
afterEach(() => { cleanup(); vi.resetAllMocks(); });

it("joins native pages by UTF-8 offset without duplicating retried chunks", () => {
  const first = mergeRecoveryChunks([], [chunk("中文", 0, false)]);
  const result = mergeRecoveryChunks(first, [chunk("中文", 0, false), chunk("🙂tail", 6, true)]);
  expect(result[0].text).toBe("中文🙂tail");
  expect(result[0].complete).toBe(true);
  expect(first[0].text).toBe("中文");
});

it("rejects gaps and conflicting pages while preserving existing text", () => {
  const first = mergeRecoveryChunks([], [chunk("中文", 0, false)]);
  expect(() => mergeRecoveryChunks(first, [chunk("tail", 2, true)])).toThrow("conversation/recovery_payload_invalid");
  expect(() => mergeRecoveryChunks(first, [chunk("other", 0, false)])).toThrow("conversation/recovery_payload_invalid");
  expect(first[0].text).toBe("中文");
});

it("loads only on request and follows the native cursor in a separate region", async () => {
  const cursor = { sequence: 7, messageIndex: 0, textByteOffset: 6 };
  api.managedConversationRecoveries.mockResolvedValueOnce({ chunks: [chunk("中文", 0, false)], nextCursor: cursor })
    .mockResolvedValueOnce({ chunks: [chunk("tail", 6, true)], nextCursor: null });
  render(<ManagedRecoveries sessionId={9} />);
  expect(api.managedConversationRecoveries).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button"));
  await waitFor(() => expect(api.managedConversationRecoveries).toHaveBeenCalledWith(9, null, 50));
  await screen.findByText("中文");
  const buttons = screen.getAllByRole("button");
  fireEvent.click(buttons[buttons.length - 1]);
  await waitFor(() => expect(api.managedConversationRecoveries).toHaveBeenLastCalledWith(9, cursor, 50));
  expect(await screen.findByText("中文tail")).toBeTruthy();
  expect(screen.getAllByRole("region")).toHaveLength(1);
});
