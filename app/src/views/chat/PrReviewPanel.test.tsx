import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { prComment, prLineComment, prMonitorSet, prMonitorWake, prReview, type PrReviewDto } from "../../api";
import { PrReviewPanel } from "./PrReviewPanel";

vi.mock("../../api", async (original) => ({
  ...(await original<typeof import("../../api")>()),
  prReview: vi.fn(), prComment: vi.fn(), prLineComment: vi.fn(), prMonitorSet: vi.fn(), prMonitorWake: vi.fn(),
}));

const review: PrReviewDto = {
  pr: { number: 12, state: "open", url: "https://github.com/acme/repo/pull/12", title: "Fix" },
  comments: Array.from({ length: 200 }, (_, index) => ({
    id: `c${index}`, author: "alice", body: `comment ${index}`, createdAt: "2026-01-01T00:00:00Z", url: null, state: null, path: null, line: null, side: null,
  })),
  reviews: [{ id: "r1", author: "bob", body: "Looks good", createdAt: "2026-01-01T00:00:00Z", url: null, state: "APPROVED", path: null, line: null, side: null }],
  files: [{ path: "src/a.ts", status: "modified", additions: 1, deletions: 1, patch: "@@ -1 +1 @@\n-old\n+new" }],
  monitor: { key: "acme/repo#12", enabled: true, lastError: null, events: [{ id: "e1", kind: "comment", summary: "alice: new comment", createdAt: 1, wakeState: "pending" }] },
};

beforeEach(() => {
  vi.mocked(prReview).mockResolvedValue(review);
  vi.mocked(prComment).mockResolvedValue();
  vi.mocked(prLineComment).mockResolvedValue();
  vi.mocked(prMonitorSet).mockResolvedValue();
  vi.mocked(prMonitorWake).mockResolvedValue("queued");
});
afterEach(() => { cleanup(); vi.clearAllMocks(); });

function renderPanel() {
  return render(<PrReviewPanel cwd="C:/repo" sessionCwd={null} sessionId={42} number={12} active />);
}

describe("PR review", () => {
  it("200 条评论可读；失败保留草稿，成功后才清空", async () => {
    renderPanel();
    expect((await screen.findAllByText(/^comment /)).length).toBe(200);
    expect(screen.getByText("Looks good")).toBeTruthy();
    const box = screen.getByRole("textbox", { name: "写下评论…" });
    fireEvent.change(box, { target: { value: "要改这里" } });
    vi.mocked(prComment).mockRejectedValueOnce("offline");
    fireEvent.click(screen.getByRole("button", { name: "发表评论" }));
    fireEvent.click(await screen.findByRole("button", { name: "确认提交到 GitHub" }));
    await waitFor(() => expect(prComment).toHaveBeenCalledWith("C:/repo", 12, "要改这里", false, null));
    expect((box as HTMLTextAreaElement).value).toBe("要改这里");
    fireEvent.click(screen.getByRole("button", { name: "发表评论" }));
    fireEvent.click(await screen.findByRole("button", { name: "确认提交到 GitHub" }));
    await waitFor(() => expect((box as HTMLTextAreaElement).value).toBe(""));
  });

  it("逐行评论提交 path/line/side，失败不丢正文", async () => {
    vi.mocked(prLineComment).mockRejectedValueOnce("offline");
    renderPanel();
    fireEvent.click(await screen.findByRole("tab", { name: "文件" }));
    fireEvent.click((await screen.findAllByRole("button", { name: "提交逐行评论" }))[0]);
    const box = screen.getByRole("textbox", { name: "写下评论…" });
    fireEvent.change(box, { target: { value: "检查边界" } });
    fireEvent.click(screen.getAllByRole("button", { name: "提交逐行评论" }).at(-1)!);
    fireEvent.click(await screen.findByRole("button", { name: "确认提交到 GitHub" }));
    await waitFor(() => expect(prLineComment).toHaveBeenCalledWith("C:/repo", 12, "src/a.ts", 1, "OLD", "检查边界", null));
    expect((box as HTMLTextAreaElement).value).toBe("检查边界");
  });

  it("监控默认只列事件，逐条点击才唤醒原会话", async () => {
    renderPanel();
    fireEvent.click(await screen.findByRole("tab", { name: "监控" }));
    expect(prMonitorWake).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "交给原会话处理" }));
    await waitFor(() => expect(prMonitorWake).toHaveBeenCalledWith(42, "acme/repo#12", "e1"));
    fireEvent.click(screen.getByRole("button", { name: "停止监控" }));
    await waitFor(() => expect(prMonitorSet).toHaveBeenCalledWith("C:/repo", 42, false, null, "acme/repo#12"));
  });
});
