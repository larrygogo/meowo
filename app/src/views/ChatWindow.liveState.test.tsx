// 对话窗的会话状态与时间线运行态接线：后台子任务数、等待审批。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { respondWithHistory } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { sessionTone, type ChatItem } from "../api";

const FIXTURES = join(__dirname, "../../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay");
const claude = JSON.parse(readFileSync(join(FIXTURES, "claude.items.json"), "utf8")) as ChatItem[];

describe("ChatWindow 会话状态与侧栏同源", () => {
  const base = {
    title: "后台子任务", provider: "claude", cwd: "C:/repo", supported: true, offset: 10, reset: false,
    pendingReview: null, connected: true, screenState: "idle", screenAssumed: false, errored: false,
    items: [
      { type: "user_text", id: "u1", timestamp: null, text: "开始" },
      { type: "assistant_text", id: "a1", timestamp: null, text: "子任务派出去了，我先停一下。" },
    ],
  };

  it("主回合停了但后台子任务还在跑：对话窗和侧栏一样是运行中，运行条在", async () => {
    // 侧栏拿同样的事实（LiveSession.busy_subagents）算出来的状态。
    expect(sessionTone(true, "waiting", null, false, 2, "idle", false)).toBe("running");
    window.history.replaceState({}, "", "/?sessionId=61");
    respondWithHistory({ ...base, sessionId: 61, status: "waiting", busySubagents: 2 });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("子任务派出去了，我先停一下。")).toBeTruthy());
    await waitFor(() => expect(document.querySelector(".chat-running")).not.toBeNull());
  });

  it("没有后台子任务时，同样的会话是等待输入，没有运行条", async () => {
    expect(sessionTone(true, "waiting", null, false, 0, "idle", false)).toBe("waiting");
    window.history.replaceState({}, "", "/?sessionId=62");
    respondWithHistory({ ...base, sessionId: 62, status: "waiting", busySubagents: 0 });
    render(<ChatWindow />);
    await waitFor(() => expect(screen.getByText("子任务派出去了，我先停一下。")).toBeTruthy());
    expect(document.querySelector(".chat-running")).toBeNull();
  });
});

describe("ChatWindow 等待审批时的时间线", () => {
  it("过程块不收起，未批准的调用写待处理、不计入已运行", async () => {
    // claude 真实样本截到写文件的调用为止：读、搜索已跑完，写文件正等审批。
    const cut = claude.findIndex((item) => item.type === "tool_use" && item.kind === "write");
    window.history.replaceState({}, "", "/?sessionId=63");
    respondWithHistory({
      sessionId: 63, title: "等审批", status: "running", provider: "claude", cwd: "C:/repo", supported: true,
      offset: 10, reset: false, pendingReview: "approval", connected: true, items: claude.slice(0, cut + 1),
    });
    render(<ChatWindow />);
    await waitFor(() => expect(document.querySelector(".turn-fold")).not.toBeNull());
    const fold = document.querySelector<HTMLElement>(".turn-fold")!;
    expect(fold.classList.contains("is-open")).toBe(true);
    expect(fold.querySelector(".turn-fold-lead")?.textContent).toBe("待处理");
    expect(fold.querySelector(".turn-fold-summary")?.textContent).not.toContain("已写入");
    const rows = [...fold.querySelectorAll<HTMLElement>("details.chat-tool")];
    expect(rows[rows.length - 1].getAttribute("data-state")).toBe("waiting");
    expect(fold.textContent).not.toContain("运行中…");
  });

  it("会话已结束：没回执的调用是已中断", async () => {
    const cut = claude.findIndex((item) => item.type === "tool_use" && item.kind === "read");
    window.history.replaceState({}, "", "/?sessionId=64");
    respondWithHistory({
      sessionId: 64, title: "历史", status: "ended", provider: "claude", cwd: "C:/repo", supported: true,
      offset: 10, reset: false, pendingReview: null, connected: false, items: claude.slice(0, cut + 1),
    });
    render(<ChatWindow />);
    await waitFor(() => expect(document.querySelector("details.chat-tool")).not.toBeNull());
    const row = document.querySelector<HTMLElement>("details.chat-tool")!;
    expect(row.getAttribute("data-state")).toBe("interrupted");
    expect(row.querySelector(".chat-tool-out")?.textContent).toBe("已中断");
  });
});
