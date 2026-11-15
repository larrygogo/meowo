import type { PendingApproval, RuntimeRequest } from "../../api";
import type { TerminalAttention } from "../../terminalAttention";

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

export function runtimeRequestTerminalLive(request: RuntimeRequest | null): boolean {
  return request?.responseCapability.type === "terminal_live";
}

export function asTerminalRuntimeRequest({ sessionId, provider, attention, writable }: {
  sessionId: number;
  provider: string;
  attention: TerminalAttention;
  writable: boolean;
}): RuntimeRequest {
  // 种类由规则声明（插件的 AttentionKind）或识别来源给出，不从 id 的后缀猜：
  // 提问是 question，命令审批与计划批准是 approval，其余（等你选择、信任页、启动步骤）是 prompt。
  const kind = attention.kind === "question" ? "question"
    : attention.kind === "approval" || attention.kind === "plan" ? "approval"
    : "prompt";
  return {
    sessionId,
    // 同一规则在屏幕重绘时更新原文和选项，不应因文本变化变成另一张请求卡。
    requestId: `terminal:${sessionId}:${attention.id}`,
    provider,
    kind,
    status: "pending",
    toolName: attention.id,
    description: null,
    input: attention.text,
    permissionSuggestions: [],
    responseCapability: writable
      ? { type: "terminal_live" }
      : { type: "not_resumable", reason: "terminal_read_only" },
    terminal: attention,
  };
}

export function runtimeRequestCommandUnavailable(error: unknown): boolean {
  const text = error instanceof Error ? error.message : String(error);
  if (text === "remote/unknown_command") return true;
  return text.includes("respond_runtime_request")
    && /unknown command|unknown_command|not found|unsupported/i.test(text);
}
