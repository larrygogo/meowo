import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { CheckpointTurnDto, GitDiffSummaryDto } from "../../api";
import { checkpointRestore, checkpointTurnDetail, checkpointTurns } from "../../api";
import { zh } from "../../i18n/zh";
import { CheckpointPanel } from "./CheckpointPanel";

vi.mock("../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api")>()),
  checkpointTurns: vi.fn(),
  checkpointTurnDetail: vi.fn(),
  checkpointRestore: vi.fn(),
  checkpointFileDiff: vi.fn(),
}));
// 差异视图本身另有测试；这里只看它拿到的摘要（分支名）。
vi.mock("./lazyDiff", () => ({
  LazyChangesView: ({ summary }: { summary: GitDiffSummaryDto }) => <div data-testid="changes">{summary.branch ?? "(无分支)"} · {summary.files.length}</div>,
}));

const turn = (n: number): CheckpointTurnDto => ({
  turn: n,
  repos: [{ root: "C:/repo", name: "repo", startRef: `refs/meowo/${n}/start`, endRef: `refs/meowo/${n}/end`, error: null,
    files: [{ path: "a.ts", status: "M", additions: 1, deletions: 0, index_status: null, worktree_status: null, untracked_files: null }] }],
} as unknown as CheckpointTurnDto);

const summary = { gitAvailable: true, isRepo: true, branch: "feat/current", ahead: 1, behind: 0, files: [], unpushed: 0 } as unknown as GitDiffSummaryDto;

afterEach(() => { cleanup(); vi.clearAllMocks(); });

/** 2026-11-16 用户反馈：「每次查看快照和切换的时候就立刻切换代码了，我可能只是想看看预览」。
 *  实际上查看从不改文件（库里没有一条恢复记录），是界面没说清：列表和当前改动长得一样，还挂着当前分支名。 */
describe("快照页是只读预览", () => {
  it("说明只是预览、不挂当前分支名；前后切换回合不恢复任何文件", async () => {
    vi.mocked(checkpointTurns).mockResolvedValue([turn(2), turn(1)]);
    vi.mocked(checkpointTurnDetail).mockImplementation(async (_sid, n) => turn(n));
    render(<CheckpointPanel sessionId={7} cwd="C:/repo" summary={summary} comments={[]} onAddComment={() => {}} onRemoveComment={() => {}} escOverrideRef={{ current: null }} onChanged={() => {}} />);
    expect(await screen.findByText(zh.chat.checkpointPreviewNote)).toBeTruthy();
    expect((await screen.findByTestId("changes")).textContent).toBe("(无分支) · 1");
    fireEvent.click(screen.getByRole("button", { name: zh.chat.checkpointNext }));
    await waitFor(() => expect(checkpointTurnDetail).toHaveBeenCalledWith(7, 1));
    fireEvent.click(screen.getByRole("button", { name: zh.chat.checkpointPrevious }));
    await waitFor(() => expect(vi.mocked(checkpointTurnDetail).mock.calls.length).toBeGreaterThanOrEqual(3));
    expect(checkpointRestore).not.toHaveBeenCalled();
  });

  it("恢复要先在确认框里再点一次", async () => {
    vi.mocked(checkpointTurns).mockResolvedValue([turn(1)]);
    vi.mocked(checkpointTurnDetail).mockResolvedValue(turn(1));
    vi.mocked(checkpointRestore).mockResolvedValue({ undoId: 3, results: [{ root: "C:/repo", restored: true, error: null }] } as never);
    render(<CheckpointPanel sessionId={7} cwd="C:/repo" summary={summary} comments={[]} onAddComment={() => {}} onRemoveComment={() => {}} escOverrideRef={{ current: null }} onChanged={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: zh.chat.checkpointRestoreStart }));
    expect(checkpointRestore).not.toHaveBeenCalled();
    const buttons = screen.getAllByRole("button", { name: zh.chat.checkpointRestoreStart });
    fireEvent.click(buttons[buttons.length - 1]);
    await waitFor(() => expect(checkpointRestore).toHaveBeenCalledWith(7, 1, "start"));
  });
});
