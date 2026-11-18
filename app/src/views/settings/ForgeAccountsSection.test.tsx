// 设置 › 代码托管账号：github.com 的连接方式照旧在最上面；其他平台的账号列表、添加（令牌 / gitlab.com 设备码）、
// 令牌失效时重新添加、退出前确认。
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor, act, within } from "@testing-library/react";

const api = vi.hoisted(() => ({
  githubStatus: vi.fn(),
  forgeAccounts: vi.fn(),
  forgeAccountAdd: vi.fn(),
  forgeAccountRemove: vi.fn(),
  forgeLoginStart: vi.fn(),
  forgeLoginCancel: vi.fn(),
  openLink: vi.fn(),
}));
vi.mock("../../api", async (o) => ({ ...(await o<typeof import("../../api")>()), ...api }));
const confirm = vi.hoisted(() => ({ appConfirm: vi.fn(), appAlert: vi.fn() }));
vi.mock("../../confirm", () => confirm);
const events = vi.hoisted(() => ({ handlers: new Map<string, (e: { payload: unknown }) => void>() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (e: { payload: unknown }) => void) => {
    events.handlers.set(name, handler);
    return Promise.resolve(() => events.handlers.delete(name));
  },
}));

import { ForgeAccountList, ForgeAccountsSection, cleanHost, tokenPage } from "./ForgeAccountsSection";
import { zh } from "../../i18n/zh";
import type { ForgeAccountsDto } from "../../api";

const f = zh.forgeAccounts;
const empty: ForgeAccountsDto = { accounts: [], credentialSupported: true, gitlabDevice: false, pending: null };
const gitlab = { platform: "gitlab", host: "gitlab.com", login: "alice", name: "Alice", needsRelogin: false, oauth: false };
const ghe = { platform: "github", host: "ghe.corp.example", login: "bob", name: null, needsRelogin: true, oauth: false };

beforeEach(() => {
  Object.values(api).forEach((m) => m.mockReset());
  confirm.appConfirm.mockReset();
  events.handlers.clear();
  api.openLink.mockResolvedValue(undefined);
  api.forgeLoginCancel.mockResolvedValue(undefined);
  api.githubStatus.mockResolvedValue({ mode: "auto", active: "gh", account: null, needsRelogin: false, ghInstalled: true, credentialSupported: true, pending: null });
});
afterEach(() => cleanup());

describe("代码托管账号", () => {
  it("分区里 github.com 的连接方式在上，其他平台在下", async () => {
    api.forgeAccounts.mockResolvedValue(empty);
    render(<ForgeAccountsSection />);
    expect(await screen.findByText(f.githubCaption)).toBeTruthy();
    expect(screen.getByText(f.othersCaption)).toBeTruthy();
    expect(await screen.findByRole("radio", { name: zh.github.modeAuto })).toBeTruthy();
    expect(await screen.findByTestId("forge-empty")).toBeTruthy();
  });

  it("列出账号：平台、主机、账号名；令牌失效的标出来，给重新添加", async () => {
    api.forgeAccounts.mockResolvedValue({ ...empty, accounts: [gitlab, ghe] });
    render(<ForgeAccountList />);
    const gl = await screen.findByTestId("forge-account-gitlab-gitlab.com");
    expect(gl.textContent).toContain("Alice · alice");
    expect(gl.textContent).toContain("GitLab · gitlab.com");
    const gh = screen.getByTestId("forge-account-github-ghe.corp.example");
    expect(gh.textContent).toContain(f.reloginDesc);
    fireEvent.click(within(gh).getByRole("button", { name: f.readd }));
    // 重新添加：表单按这个账号的平台与主机预填。
    const host = (await screen.findByLabelText(f.host)) as HTMLInputElement;
    expect(host.value).toBe("ghe.corp.example");
    expect(screen.getByRole("radio", { name: f.platformNames.github }).getAttribute("aria-checked")).toBe("true");
  });

  it("用令牌添加 Gitea：默认 codeberg.org，写明权限、给生成页，验证通过后收起表单", async () => {
    api.forgeAccounts.mockResolvedValue(empty);
    api.forgeAccountAdd.mockResolvedValue({ ...empty, accounts: [{ ...gitlab, platform: "gitea", host: "codeberg.org", login: "carol", name: null }] });
    render(<ForgeAccountList />);
    fireEvent.click(await screen.findByRole("button", { name: f.add }));
    fireEvent.click(screen.getByRole("radio", { name: f.platformNames.gitea }));
    expect((screen.getByLabelText(f.host) as HTMLInputElement).value).toBe("codeberg.org");
    expect(screen.getByText(f.scopes.gitea, { exact: false })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: f.createToken }));
    expect(api.openLink).toHaveBeenCalledWith("https://codeberg.org/user/settings/applications");
    // 没填令牌：不发请求。
    fireEvent.click(screen.getByTestId("forge-verify"));
    expect(api.forgeAccountAdd).not.toHaveBeenCalled();
    fireEvent.change(screen.getByTestId("forge-token"), { target: { value: " c0ffee " } });
    fireEvent.click(screen.getByTestId("forge-verify"));
    await waitFor(() => expect(api.forgeAccountAdd).toHaveBeenCalledWith("gitea", "codeberg.org", "c0ffee", null));
    expect(await screen.findByTestId("forge-account-gitea-codeberg.org")).toBeTruthy();
    expect(screen.queryByTestId("forge-add")).toBeNull();
  });

  it("Bitbucket：主机固定为 bitbucket.org，要 Atlassian 邮箱；令牌被拒时说清楚", async () => {
    api.forgeAccounts.mockResolvedValue(empty);
    api.forgeAccountAdd.mockRejectedValue("forge/token_rejected");
    render(<ForgeAccountList />);
    fireEvent.click(await screen.findByRole("button", { name: f.add }));
    fireEvent.click(screen.getByRole("radio", { name: f.platformNames.bitbucket }));
    const host = screen.getByLabelText(f.host) as HTMLInputElement;
    expect(host.disabled).toBe(true);
    expect(host.value).toBe("bitbucket.org");
    fireEvent.change(screen.getByLabelText(f.email), { target: { value: "me@example.com" } });
    fireEvent.change(screen.getByTestId("forge-token"), { target: { value: "ATATT3xFf=AB" } });
    fireEvent.click(screen.getByTestId("forge-verify"));
    await waitFor(() => expect(api.forgeAccountAdd).toHaveBeenCalledWith("bitbucket", "bitbucket.org", "ATATT3xFf=AB", "me@example.com"));
    expect(await screen.findByText("令牌不对或已失效")).toBeTruthy();
    expect(screen.getByTestId("forge-add")).toBeTruthy();
  });

  it("gitlab.com 配了应用时先给浏览器登录：显示代码、复制并打开、登录成功后列出账号", async () => {
    api.forgeAccounts.mockResolvedValue({ ...empty, gitlabDevice: true });
    const code = { platform: "gitlab", host: "gitlab.com", userCode: "0A44L90H", verificationUri: "https://gitlab.com/oauth/device", verificationUriComplete: "https://gitlab.com/oauth/device?user_code=0A44L90H", expiresAt: Date.now() + 5 * 60_000 };
    api.forgeLoginStart.mockResolvedValue(code);
    Object.assign(navigator, { clipboard: { writeText: vi.fn(() => Promise.resolve()) } });
    render(<ForgeAccountList />);
    fireEvent.click(await screen.findByRole("button", { name: f.add }));
    expect(screen.getByText(f.deviceDesc)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: f.deviceStart }));
    expect((await screen.findByTestId("forge-code")).textContent).toBe("0A44L90H");
    fireEvent.click(screen.getByRole("button", { name: f.copyAndOpen }));
    expect(api.openLink).toHaveBeenCalledWith(code.verificationUriComplete);
    api.forgeAccounts.mockResolvedValue({ ...empty, gitlabDevice: true, accounts: [{ ...gitlab, oauth: true }] });
    await act(async () => events.handlers.get("forge-login-finished")?.({ payload: { state: "ok", error: null } }));
    expect(await screen.findByTestId("forge-account-gitlab-gitlab.com")).toBeTruthy();
    expect(screen.queryByTestId("forge-code")).toBeNull();
  });

  it("gitlab.com 没配应用：只有令牌入口；自管实例也只有令牌", async () => {
    api.forgeAccounts.mockResolvedValue(empty);
    render(<ForgeAccountList />);
    fireEvent.click(await screen.findByRole("button", { name: f.add }));
    expect(screen.queryByText(f.deviceDesc)).toBeNull();
    expect(screen.getByTestId("forge-token")).toBeTruthy();
    expect(screen.getByText(f.scopes.gitlab, { exact: false })).toBeTruthy();
  });

  it("退出前确认，确认后删掉", async () => {
    api.forgeAccounts.mockResolvedValue({ ...empty, accounts: [gitlab] });
    api.forgeAccountRemove.mockResolvedValue(empty);
    confirm.appConfirm.mockResolvedValue(true);
    render(<ForgeAccountList />);
    fireEvent.click(within(await screen.findByTestId("forge-account-gitlab-gitlab.com")).getByRole("button", { name: f.signOut }));
    await waitFor(() => expect(confirm.appConfirm).toHaveBeenCalledWith(f.signOutConfirm("GitLab", "gitlab.com"), expect.anything()));
    await waitFor(() => expect(api.forgeAccountRemove).toHaveBeenCalledWith("gitlab", "gitlab.com"));
    expect(await screen.findByTestId("forge-empty")).toBeTruthy();
  });

  it("主机输入归一、生成令牌的网页", () => {
    expect(cleanHost(" https://Git.Example.com/group/ ")).toBe("git.example.com");
    expect(tokenPage("gitlab", "gitlab.com")).toBe("https://gitlab.com/-/user_settings/personal_access_tokens?name=Meowo&scopes=api");
    expect(tokenPage("github", "ghe.corp.example")).toBe("https://ghe.corp.example/settings/tokens/new?description=Meowo&scopes=repo");
    expect(tokenPage("bitbucket", "bitbucket.org")).toBe("https://id.atlassian.com/manage-profile/security/api-tokens");
    expect(tokenPage("gitea", "")).toBeNull();
  });
});
