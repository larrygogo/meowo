import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";

/// 包 5：新建会话「在新 worktree 里开」。默认不勾，不勾时行为与改前完全一致（不多调任何命令）；
/// 勾上后先建 worktree，再把会话开在新目录里。新建页（对话窗）和面板（独立窗、手机）共用一套。

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
const closeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ close: closeMock }) }));

import { NewSessionPage } from "./chat/NewSessionPage";
import { NewSessionPanel } from "./NewSessionPanel";
import { descriptors } from "../test/agents";
import { zh } from "../i18n/zh";
import { normalizePath } from "../paths";
import { localSlug } from "./useWorktreeChoice";

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
  hasSetup: true,
  ...extra,
});
const created = {
  taskId: "wt-1-1",
  path: "C:/work/meowo-fix-login",
  branch: "meowo/fix-login",
  base: "origin/main",
  originRoot: "C:/work/meowo",
  copied: 2,
  copySkipped: 0,
  setup: { taskId: "wt-1-1", path: "C:/work/meowo-fix-login", state: "running" },
};

beforeEach(() => {
  Object.values(api).forEach((m) => m.mockReset());
  closeMock.mockReset();
  api.recentCwds.mockResolvedValue(["C:/work/meowo"]);
  api.checkProviderHooks.mockResolvedValue("installed");
  api.getSettings.mockResolvedValue({ default_agent: "claude" });
  api.listAgents.mockResolvedValue(descriptors(["claude", "codex"]));
  api.getAccounts.mockResolvedValue([{ provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true }]);
  api.getSessionAdminPage.mockResolvedValue({ items: [], total: 0 });
  api.worktreePreview.mockResolvedValue(preview());
  api.worktreeCreate.mockResolvedValue(created);
});
afterEach(() => cleanup());

async function openPage() {
  const onLaunched = vi.fn();
  render(<NewSessionPage prefill={{}} onLaunched={onLaunched} />);
  await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("meowo"));
  return onLaunched;
}

describe("新建会话页：在新 worktree 里开", () => {
  it("默认不勾，不勾时和改前一样：不查预览、不建 worktree，会话开在选中的目录", async () => {
    api.newSession.mockResolvedValue(-7);
    const onLaunched = await openPage();
    const toggle = screen.getByTestId("wt-toggle") as HTMLInputElement;
    expect(toggle.checked).toBe(false);
    expect(screen.queryByTestId("wt-slug")).toBeNull();
    fireEvent.click(screen.getByTestId("nsp-send"));
    await waitFor(() => expect(onLaunched).toHaveBeenCalledWith(-7, "claude", "", []));
    expect(api.newSession).toHaveBeenCalledWith(normalizePath("C:/work/meowo"), "claude", {});
    expect(api.worktreePreview).not.toHaveBeenCalled();
    expect(api.worktreeCreate).not.toHaveBeenCalled();
  });

  it("勾上后显示建议名、基点和目录，开始时先建 worktree 再把会话开在新目录里", async () => {
    api.newSession.mockResolvedValue(-9);
    const onLaunched = await openPage();
    fireEvent.click(screen.getByTestId("wt-toggle"));
    const slug = screen.getByTestId("wt-slug") as HTMLInputElement;
    expect(slug.placeholder).toBe(zh.wtnew.slugPlaceholder(localSlug()));
    fireEvent.change(slug, { target: { value: "Fix Login" } });
    await waitFor(() => expect(screen.getByTestId("wt-meta").textContent).toContain("origin/main"));
    expect(screen.getByTestId("wt-meta").textContent).toContain("C:/work/meowo-fix-login");
    expect(screen.getByTestId("wt-meta").textContent).toContain(zh.wtnew.hasSetup);
    expect((screen.getByTestId("wt-branch") as HTMLInputElement).placeholder).toBe("meowo/fix-login");
    expect(api.worktreePreview).toHaveBeenLastCalledWith(normalizePath("C:/work/meowo"), "Fix Login", null, localSlug());
    fireEvent.change(screen.getByTestId("nsp-input"), { target: { value: "修一下登录" } });
    fireEvent.click(screen.getByTestId("nsp-send"));
    await waitFor(() => expect(onLaunched).toHaveBeenCalledWith(-9, "claude", "修一下登录", []));
    expect(api.worktreeCreate).toHaveBeenCalledWith(expect.objectContaining({
      repoCwd: normalizePath("C:/work/meowo"),
      slug: "Fix Login",
      branch: null,
      provider: "claude",
    }));
    expect(api.newSession).toHaveBeenCalledWith("C:/work/meowo-fix-login", "claude", {});
  });

  it("撞名时提示并给建议名，点一下就用建议名", async () => {
    api.worktreePreview.mockImplementation((_dir: string, slug: string | null) =>
      Promise.resolve(slug === "mine" ? preview({ slug: "mine", dirExists: true, suggestedSlug: "mine-2", suggestedBranch: "meowo/mine-2" }) : preview({ slug: slug ?? "x" })));
    await openPage();
    fireEvent.click(screen.getByTestId("wt-toggle"));
    fireEvent.change(screen.getByTestId("wt-slug"), { target: { value: "mine" } });
    const conflict = await screen.findByTestId("wt-conflict");
    expect(conflict.textContent).toContain(zh.wtnew.dirExists);
    fireEvent.click(screen.getByTestId("wt-use-suggestion"));
    expect((screen.getByTestId("wt-slug") as HTMLInputElement).value).toBe("mine-2");
    await waitFor(() => expect(screen.queryByTestId("wt-conflict")).toBeNull());
  });

  it("建好了 worktree 但会话没起来，再点一次沿用同一个请求号，不会建第二个", async () => {
    api.newSession.mockRejectedValueOnce("目录不存在").mockResolvedValueOnce(-4);
    const onLaunched = await openPage();
    fireEvent.click(screen.getByTestId("wt-toggle"));
    fireEvent.click(screen.getByTestId("nsp-send"));
    await screen.findByTestId("nsp-error");
    fireEvent.click(screen.getByTestId("nsp-send"));
    await waitFor(() => expect(onLaunched).toHaveBeenCalled());
    const ids = api.worktreeCreate.mock.calls.map(([args]) => (args as { requestId: string }).requestId);
    expect(ids).toHaveLength(2);
    expect(ids[0]).toBe(ids[1]);
    expect(ids[0]).toBeTruthy();
  });

  it("用户填的分支撞了：报错带建议名，不悄悄改名", async () => {
    api.worktreeCreate.mockRejectedValue("worktree/branch_exists: feature/z-2");
    await openPage();
    fireEvent.click(screen.getByTestId("wt-toggle"));
    fireEvent.change(screen.getByTestId("wt-branch"), { target: { value: "feature/z" } });
    fireEvent.click(screen.getByTestId("nsp-send"));
    const error = await screen.findByTestId("nsp-error");
    expect(error.textContent).toContain("分支已存在，可改用");
    expect(error.textContent).toContain("feature/z-2");
    expect(api.newSession).not.toHaveBeenCalled();
  });
});

describe("新建会话面板（独立窗、手机页内）：在新 worktree 里开", () => {
  it("默认不勾，启动只调 newSession；勾上后先建 worktree", async () => {
    api.newSession.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.change(await screen.findByTestId("ns-dir"), { target: { value: "C:/proj" } });
    expect((screen.getByTestId("wt-toggle") as HTMLInputElement).checked).toBe(false);
    fireEvent.click(screen.getByTestId("ns-launch"));
    await waitFor(() => expect(api.newSession).toHaveBeenCalledWith("C:/proj", "claude", {}));
    expect(api.worktreeCreate).not.toHaveBeenCalled();
    cleanup();

    api.newSession.mockClear();
    render(<NewSessionPanel />);
    fireEvent.change(await screen.findByTestId("ns-dir"), { target: { value: "C:/proj" } });
    fireEvent.click(screen.getByTestId("wt-toggle"));
    fireEvent.click(screen.getByTestId("ns-launch"));
    await waitFor(() => expect(api.newSession).toHaveBeenCalledWith("C:/work/meowo-fix-login", "claude", {}));
    expect(api.worktreeCreate).toHaveBeenCalledWith(expect.objectContaining({ repoCwd: "C:/proj" }));
  });
});
