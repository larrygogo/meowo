import { describe, expect, it } from "vitest";
import type { PendingApproval, RuntimeRequest } from "../../api";
import { asRuntimeApproval, asRuntimeRequest, runtimeRequestBrokerLive, runtimeRequestCommandUnavailable } from "./runtimeRequest";

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

  it("falls back only when the runtime command is unavailable", () => {
    expect(runtimeRequestCommandUnavailable("remote/unknown_command")).toBe(true);
    expect(runtimeRequestCommandUnavailable("Command respond_runtime_request not found")).toBe(true);
    expect(runtimeRequestCommandUnavailable("runtime_request/already_resolved")).toBe(false);
    expect(runtimeRequestCommandUnavailable("runtime_request/not_resumable")).toBe(false);
  });
});
