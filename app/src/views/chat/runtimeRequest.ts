import type { PendingApproval, RuntimeRequest } from "../../api";

export function asRuntimeRequest(request: PendingApproval | RuntimeRequest): RuntimeRequest {
  if ("responseCapability" in request) return request;
  return {
    ...request,
    kind: "question",
    status: "pending",
    responseCapability: request.answerable
      ? { type: "broker_live" }
      : { type: "not_resumable", reason: "legacy_backend" },
  };
}

export function asRuntimeApproval(request: PendingApproval | RuntimeRequest): RuntimeRequest {
  if ("responseCapability" in request) return request;
  return {
    ...request,
    kind: "approval",
    status: "pending",
    // 旧后端的 pending-approval / approval 只吐 broker 正持有的普通审批；answerable 恒 false
    // 是旧题面字段的约定，不能拿来否定普通审批的实时回答能力。
    responseCapability: { type: "broker_live" },
  };
}

export function runtimeRequestBrokerLive(request: RuntimeRequest | null): boolean {
  return request?.responseCapability.type === "broker_live";
}

export function runtimeRequestCommandUnavailable(error: unknown): boolean {
  const text = error instanceof Error ? error.message : String(error);
  if (text === "remote/unknown_command") return true;
  return text.includes("respond_runtime_request")
    && /unknown command|unknown_command|not found|unsupported/i.test(text);
}
