import { useRef, useState } from "react";
import {
  cancelLogin,
  createLoginOperationId,
  loginAgent,
  type AgentId,
  type LoginDone,
} from "../api";
import { useTauriEvent } from "./useTauriEvent";

export type LoginOperationState =
  | { phase: "pending"; operationId: string; profile?: string | null }
  | { phase: "done"; outcome: LoginDone["outcome"] }
  | { phase: "error"; action: "start" | "cancel" };

interface LoginOptions {
  terminal?: string;
  profile?: string | null;
}

/**
 * Owns the complete lifecycle of detached-terminal login operations.
 *
 * The state is keyed by provider because different agents may log in concurrently.
 * Consumers should mount this hook above provider-specific cards so switching cards
 * cannot forget an operation that is still being watched by the backend.
 */
export function useLoginOperations(onDone?: (event: LoginDone) => void) {
  const [states, setStates] = useState<Map<AgentId, LoginOperationState>>(new Map());
  // State updates are asynchronous; this map is the synchronous authority used to
  // reject double clicks and to match completion events to the current operation.
  const pendingRef = useRef<Map<AgentId, string>>(new Map());

  useTauriEvent<LoginDone>("login-done", (event) => {
    const { provider, operationId, outcome } = event.payload;
    if (pendingRef.current.get(provider) !== operationId) return;

    pendingRef.current.delete(provider);
    setStates((current) => new Map(current).set(provider, { phase: "done", outcome }));
    onDone?.(event.payload);
  });

  const start = async (provider: AgentId, options: LoginOptions = {}): Promise<boolean> => {
    if (pendingRef.current.has(provider)) return false;

    const operationId = createLoginOperationId(provider);
    pendingRef.current.set(provider, operationId);
    setStates((current) => new Map(current).set(provider, {
      phase: "pending",
      operationId,
      profile: options.profile,
    }));

    try {
      await loginAgent(provider, options.terminal, options.profile, operationId);
      return true;
    } catch (error) {
      // A newer operation must never be cleared by an older rejected promise.
      if (pendingRef.current.get(provider) === operationId) {
        pendingRef.current.delete(provider);
        setStates((current) => new Map(current).set(provider, { phase: "error", action: "start" }));
      }
      throw error;
    }
  };

  const cancel = async (provider: AgentId): Promise<boolean> => {
    const operationId = pendingRef.current.get(provider);
    if (!operationId) return false;

    try {
      await cancelLogin(provider, operationId);
      return true;
    } catch (error) {
      // If cancellation IPC itself fails, unlock the UI so the user can retry.
      if (pendingRef.current.get(provider) === operationId) {
        pendingRef.current.delete(provider);
        setStates((current) => new Map(current).set(provider, { phase: "error", action: "cancel" }));
      }
      throw error;
    }
  };

  return {
    states,
    start,
    cancel,
    isPending: (provider: AgentId) => pendingRef.current.has(provider),
  };
}
