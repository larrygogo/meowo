import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../platform", () => ({ isMac: () => false }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { ChatSidebar } from "./ChatSidebar";
import type { LiveSession } from "../api";
import { I18nProvider } from "../i18n";
import { zh } from "../i18n/zh";
import { en } from "../i18n/en";

const REMOTE_FLAG = "__MEOWO_REMOTE__";

function session(id: number, title: string, status: string, extra: Partial<LiveSession> = {}): LiveSession {
  return {
    session: { id, cc_session_id: `cc-${id}`, status },
    project_name: "meowo",
    task_title: title,
    connected: true,
    pending_review: null,
    cwd: "C:/Users/me/workspace/meowo",
    provider: "claude",
    ...extra,
  } as unknown as LiveSession;
}

/// 验收 1、18：侧栏会话行的状态词取自词表，桌面（状态点的名字）与手机（行尾状态字）是同一个词，
/// 中英文各验一遍。五家共用这一行，Provider 不影响状态词。
describe.each([["zh", zh], ["en", en]] as const)("侧栏状态词取自词表（%s）", (lang, t) => {
  beforeEach(() => {
    invoke.mockReset();
    localStorage.clear();
    invoke.mockImplementation((command: string) => command === "get_live_sessions_page"
      ? Promise.resolve([
        session(1, "alpha", "running", { provider: "codex" }),
        session(2, "beta", "waiting", { provider: "kimi" }),
        session(3, "gamma", "waiting", { provider: "gemini" }),
        session(4, "delta", "waiting", { provider: "opencode", errored: true }),
      ])
      : Promise.resolve());
  });
  afterEach(() => {
    cleanup();
    delete (globalThis as Record<string, unknown>)[REMOTE_FLAG];
  });

  const mount = () => render(
    <I18nProvider initial={lang}>
      <ChatSidebar activeId={1} approvalAwaitingIds={new Set([2])} onSelect={() => {}} onCollapse={() => {}} />
    </I18nProvider>,
  );
  const expected = { alpha: t.vocab.session.running, beta: t.vocab.request.approval, gamma: t.vocab.session.waiting, delta: t.vocab.session.error };

  it("桌面：状态点的读屏名与悬停提示", async () => {
    mount();
    for (const [title, word] of Object.entries(expected)) {
      const dot = (await screen.findByRole("button", { name: new RegExp(title) })).querySelector(".chat-sidebar-dot");
      expect(dot?.getAttribute("aria-label"), title).toBe(word);
      expect(dot?.getAttribute("data-tip"), title).toBe(word);
    }
  });

  it("手机：行尾状态字与桌面同词", async () => {
    (globalThis as Record<string, unknown>)[REMOTE_FLAG] = true;
    mount();
    for (const [title, word] of Object.entries(expected)) {
      const row = await screen.findByRole("button", { name: new RegExp(title) });
      expect(row.querySelector(".chat-sidebar-status")?.textContent, title).toBe(word);
    }
  });
});
