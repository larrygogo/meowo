import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";

/// 做空审查：在仓库子目录（如 repo/app）勾「在新 worktree 里开」，会话却落在 worktree 根目录。
/// 后端建好后给出 sessionCwd（worktree 里同一相对子目录，基点里没有就是根目录），前端照它开会话；
/// 子目录基点里没有时预览里先说明会退回根目录。

const api = vi.hoisted(() => ({
  newSession: vi.fn(),
  recentCwds: vi.fn(),
  checkProviderHooks: vi.fn(),
  getSettings: vi.fn(),
  listAgents: vi.fn(),
  getAccounts: vi.fn(),
  listSubdirectories: vi.fn(),
  getSessionAdminPage: vi.fn(),
  savePastedAttachment: vi.fn(),
  worktreePreview: vi.fn(),
  worktreeCreate: vi.fn(),
}));
vi.mock("../api", async (orig) => ({ ...(await orig<typeof import("../api")>()), ...api }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/api/core", async (orig) => ({ ...(await orig<typeof import("@tauri-apps/api/core")>()), convertFileSrc: (path: string) => `asset://${path}` }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}), emit: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ close: vi.fn() }) }));

import { NewSessionPanel } from "./NewSessionPanel";
import { descriptors } from "../test/agents";
import { zh } from "../i18n/zh";
import { en } from "../i18n/en";

const preview = (extra: Record<string, unknown> = {}) => ({
  repoRoot: "C:/work/meowo",
  originRoot: "C:/work/meowo",
  repoName: "meowo",
  slug: "fix-login",
  branch: "meowo/fix-login",
  path: "C:/work/meowo-fix-login",
  base: "origin/main",
  dirExists: false,
  branchExists: false,
  branchCheckedOut: false,
  suggestedSlug: null,
  suggestedBranch: null,
  hasSetup: false,
  subDir: "app",
  subDirMissing: false,
  ...extra,
});
const created = (extra: Record<string, unknown> = {}) => ({
  taskId: "wt-1-1",
  path: "C:/work/meowo-fix-login",
  sessionCwd: "C:/work/meowo-fix-login/app",
  branch: "meowo/fix-login",
  base: "origin/main",
  originRoot: "C:/work/meowo",
  copied: 0,
  copySkipped: 0,
  setup: { taskId: "wt-1-1", path: "C:/work/meowo-fix-login", state: "skipped", reason: "none" },
  ...extra,
});

beforeEach(() => {
  Object.values(api).forEach((m) => m.mockReset());
  api.recentCwds.mockResolvedValue([]);
  api.checkProviderHooks.mockResolvedValue("installed");
  api.getSettings.mockResolvedValue({ default_agent: "claude" });
  api.listAgents.mockResolvedValue(descriptors(["claude"]));
  api.getAccounts.mockResolvedValue([]);
  api.getSessionAdminPage.mockResolvedValue({ items: [], total: 0 });
  api.newSession.mockResolvedValue(undefined);
});
afterEach(() => cleanup());

async function launchIn(dir: string) {
  render(<NewSessionPanel />);
  fireEvent.change(await screen.findByTestId("ns-dir"), { target: { value: dir } });
  fireEvent.click(screen.getByTestId("wt-toggle"));
  await waitFor(() => expect(screen.getByTestId("wt-meta")).toBeTruthy());
}

describe("在仓库子目录里勾「在新 worktree 里开」", () => {
  it("会话开在 worktree 里同一相对子目录，预览写明", async () => {
    api.worktreePreview.mockResolvedValue(preview());
    api.worktreeCreate.mockResolvedValue(created());
    await launchIn("C:/work/meowo/app");
    expect(screen.getByTestId("wt-meta").textContent).toContain(zh.wtnew.subDir("app"));
    fireEvent.click(screen.getByTestId("ns-launch"));
    await waitFor(() => expect(api.newSession).toHaveBeenCalledWith("C:/work/meowo-fix-login/app", "claude", {}));
  });

  it("子目录在基点里没有：预览说明会退回根目录，会话开在根目录", async () => {
    api.worktreePreview.mockResolvedValue(preview({ subDir: "scratch", subDirMissing: true }));
    api.worktreeCreate.mockResolvedValue(created({ sessionCwd: "C:/work/meowo-fix-login" }));
    await launchIn("C:/work/meowo/scratch");
    expect(screen.getByTestId("wt-subdir-missing").textContent).toBe(zh.wtnew.subDirMissing("scratch"));
    fireEvent.click(screen.getByTestId("ns-launch"));
    await waitFor(() => expect(api.newSession).toHaveBeenCalledWith("C:/work/meowo-fix-login", "claude", {}));
  });

  it("旧后端没下发 sessionCwd：照旧开在 worktree 根目录", async () => {
    api.worktreePreview.mockResolvedValue(preview({ subDir: undefined, subDirMissing: undefined }));
    api.worktreeCreate.mockResolvedValue(created({ sessionCwd: undefined }));
    await launchIn("C:/work/meowo");
    fireEvent.click(screen.getByTestId("ns-launch"));
    await waitFor(() => expect(api.newSession).toHaveBeenCalledWith("C:/work/meowo-fix-login", "claude", {}));
  });

  it("文案中英都有，不超过 20 字", () => {
    for (const dict of [zh, en]) {
      expect(dict.wtnew.subDir("app")).toBeTruthy();
      expect(dict.wtnew.subDirMissing("scratch")).toBeTruthy();
    }
    expect(zh.wtnew.subDirMissing("x").length).toBeLessThanOrEqual(20);
    expect(zh.wtnew.subDir("x").length).toBeLessThanOrEqual(20);
  });
});
