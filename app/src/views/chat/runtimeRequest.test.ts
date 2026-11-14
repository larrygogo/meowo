import { describe, expect, it } from "vitest";
import type { PendingApproval, RuntimeRequest } from "../../api";
import { asRuntimeApproval, asRuntimeRequest, asTerminalRuntimeRequest, runtimeRequestBrokerLive, runtimeRequestCommandUnavailable, runtimeRequestTerminalLive } from "./runtimeRequest";

const legacy: PendingApproval = {
  sessionId: 7,
  requestId: "q1",
  provider: "claude",
  toolName: "AskUserQuestion",
  description: null,
  input: "{}",
  permissionSuggestions: [],
  answerable: true,
};

describe("RuntimeRequest compatibility", () => {
  it("converts a legacy answerable question to broker_live", () => {
    const request = asRuntimeRequest(legacy);
    expect(request.kind).toBe("question");
    expect(request.status).toBe("pending");
    expect(runtimeRequestBrokerLive(request)).toBe(true);
  });

  it("keeps a legacy display-only question not_resumable", () => {
    const request = asRuntimeRequest({ ...legacy, answerable: false });
    expect(request.responseCapability).toEqual({ type: "not_resumable", reason: "legacy_backend" });
    expect(runtimeRequestBrokerLive(request)).toBe(false);
  });

  it("preserves an already normalized request", () => {
    const request: RuntimeRequest = {
      ...legacy,
      kind: "question",
      status: "pending",
      responseCapability: { type: "broker_live" },
    };
    expect(asRuntimeRequest(request)).toBe(request);
  });

  it("treats a legacy pending ordinary approval as broker_live", () => {
    const request = asRuntimeApproval({ ...legacy, toolName: "Bash", answerable: false });
    expect(request.kind).toBe("approval");
    expect(runtimeRequestBrokerLive(request)).toBe(true);
  });

  /// 种类由规则声明（attention.kind），不从 id 的后缀猜。信任页此前因为「id 以 provider: 开头且有
  /// 选项」被算成审批，现在照实是提示。
  it("屏幕请求的种类取自规则声明：提问、审批（含计划批准）、其余提示", () => {
    const base = { text: "screen", provider: "kimi", sessionId: 7, writable: true };
    expect(asTerminalRuntimeRequest({ ...base, attention: { id: "kimi:question", kind: "question", text: "question" } }).kind).toBe("question");
    expect(asTerminalRuntimeRequest({ ...base, attention: { id: "kimi:command-approval", kind: "approval", text: "approval", details: "arrow_panel" } }).kind).toBe("approval");
    expect(asTerminalRuntimeRequest({ ...base, attention: { id: "claude:plan-approval", kind: "plan", text: "plan" } }).kind).toBe("approval");
    expect(asTerminalRuntimeRequest({ ...base, attention: { id: "provider:trust", kind: "trust", text: "trust", options: [{ label: "Yes", input: "1" }] } }).kind).toBe("prompt");
    expect(asTerminalRuntimeRequest({ ...base, attention: { id: "claude:long-session-resume", kind: "choice", text: "continue" } }).kind).toBe("prompt");
    expect(asTerminalRuntimeRequest({ ...base, attention: { id: "gemini:usage-limit", kind: "choice", text: "limit" } }).kind).toBe("prompt");
  });

  it("keeps terminal identity stable across repaints and preserves native options", () => {
    const first = asTerminalRuntimeRequest({
      sessionId: 42,
      provider: "gemini",
      writable: true,
      attention: { id: "gemini:command-approval", kind: "approval", text: "Allow once?", details: "framed_box", options: [{ label: "Allow once", input: "\r", focused: true }] },
    });
    const repainted = asTerminalRuntimeRequest({
      sessionId: 42,
      provider: "gemini",
      writable: true,
      attention: { id: "gemini:command-approval", kind: "approval", text: "Allow once?\nnew detail", details: "framed_box", options: [{ label: "Allow once", input: "\r" }] },
    });
    expect(first.requestId).toBe("terminal:42:gemini:command-approval");
    expect(repainted.requestId).toBe(first.requestId);
    expect(repainted.terminal?.options?.[0].input).toBe("\r");
    expect(runtimeRequestTerminalLive(repainted)).toBe(true);
    expect(runtimeRequestBrokerLive(repainted)).toBe(false);
  });

  it("marks a read-only terminal request not_resumable without discarding its text", () => {
    const request = asTerminalRuntimeRequest({
      sessionId: 7,
      provider: "opencode",
      writable: false,
      attention: { id: "opencode:command-approval", kind: "approval", text: "Allow this action?" },
    });
    expect(request.responseCapability).toEqual({ type: "not_resumable", reason: "terminal_read_only" });
    expect(request.terminal?.text).toBe("Allow this action?");
    expect(runtimeRequestTerminalLive(request)).toBe(false);
  });

  it("falls back only when the runtime command is unavailable", () => {
    expect(runtimeRequestCommandUnavailable("remote/unknown_command")).toBe(true);
    expect(runtimeRequestCommandUnavailable("Command respond_runtime_request not found")).toBe(true);
    expect(runtimeRequestCommandUnavailable("runtime_request/already_resolved")).toBe(false);
    expect(runtimeRequestCommandUnavailable("runtime_request/not_resumable")).toBe(false);
  });
});
