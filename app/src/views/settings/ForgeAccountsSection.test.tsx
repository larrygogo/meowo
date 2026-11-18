// 设置 › 代码托管：Git 组（git 程序、提交身份）与平台账号组（github.com 与其他平台同一个列表、同一个添加流程；
// github.com 可浏览器登录或令牌，其他平台只用令牌；没有「关闭」开关）。原 GithubSection 的测试迁到这里。
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor, act, within } from "@testing-library/react";

const api = vi.hoisted(() => ({
  forgeAccounts: vi.fn(),
  forgeAccountAdd: vi.fn(),
  forgeAccountRemove: vi.fn(),
  forgeLoginStart: vi.fn(),
  forgeLoginCancel: vi.fn(),
  gitToolStatus: vi.fn(),
  gitIdentitySet: vi.fn(),
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

import { CodeHostingSection, ForgeAccountList, cleanHost, deviceLogin, minutesLeft, tokenPage } from "./ForgeAccountsSection";
import { GitToolCard, GIT_DOWNLOAD_URL } from "./GitToolCard";
import { zh } from "../../i18n/zh";
import type { ForgeAccountsDto, GitToolDto } from "../../api";

const f = zh.forgeAccounts;
const g = zh.codeHosting;
const empty: ForgeAccountsDto = { accounts: [], credentialSupported: true, pending: null };
const github = { platform: "github", host: "github.com", login: "octocat", name: "The Octocat", needsRelogin: false, oauth: true };
const gitlab = { platform: "gitlab", host: "gitlab.com", login: "alice", name: "Alice", needsRelogin: false, oauth: false };
const ghe = { platform: "github", host: "ghe.corp.example", login: "bob", name: null, needsRelogin: true, oauth: false };
const git: GitToolDto = { found: true, path: "C:\\Program Files\\Git\\cmd\\git.exe", version: "2.47.1.windows.1", userName: "Larry", userEmail: "larry@example.com", error: null };

beforeEach(() => {
  Object.values(api).forEach((m) => m.mockReset());
  confirm.appConfirm.mockReset();
  events.handlers.clear();
  api.openLink.mockResolvedValue(undefined);
  api.forgeLoginCancel.mockResolvedValue(undefined);
  api.gitToolStatus.mockResolvedValue(git);
});
afterEach(() => cleanup());

describe("代码托管分区", () => {
  it("两组：Git 与平台账号；没有 GitHub 连接方式、也没有关闭开关", async () => {
    api.forgeAccounts.mockResolvedValue(empty);
    render(<CodeHostingSection />);
    expect(await screen.findByText(g.gitCaption)).toBeTruthy();
    expect(screen.getByText(g.accountsCaption)).toBeTruthy();
    expect(await screen.findByTestId("git-tool-path")).toBeTruthy();
    expect(await screen.findByTestId("forge-empty")).toBeTruthy();
    expect(screen.queryByRole("radio")).toBeNull();
    expect(screen.queryByRole("switch")).toBeNull();
    expect(screen.queryByText(/gh/)).toBeNull();
  });
});

describe("平台账号", () => {
  it("github.com 与其他平台列在同一个列表里：平台、主机、登录方式；令牌失效的标出来，给重新添加", async () => {
    api.forgeAccounts.mockResolvedValue({ ...empty, accounts: [github, gitlab, ghe] });
    render(<ForgeAccountList />);
    const gh = await screen.findByTestId("forge-account-github-github.com");
    expect(gh.textContent).toContain("The Octocat · octocat");
    expect(gh.textContent).toContain(`GitHub · github.com · ${f.viaDevice}`);
    const gl = screen.getByTestId("forge-account-gitlab-gitlab.com");
    expect(gl.textContent).toContain(`GitLab · gitlab.com · ${f.viaToken}`);
    const ent = screen.getByTestId("forge-account-github-ghe.corp.example");
    expect(ent.textContent).toContain(f.reloginDesc);
    fireEvent.click(within(ent).getByRole("button", { name: f.readd }));
    // 重新添加：表单按这个账号的平台与主机预填；企业版只有令牌。
    const host = (await screen.findByLabelText(f.host)) as HTMLInputElement;
    expect(host.value).toBe("ghe.corp.example");
    expect(screen.getByRole("radio", { name: f.platformNames.github }).getAttribute("aria-checked")).toBe("true");
    expect(screen.getByTestId("forge-token")).toBeTruthy();
    expect(screen.queryByTestId("forge-use-device")).toBeNull();
  });

  it("添加默认选 GitHub、主机 github.com，先给浏览器登录：显示代码、复制并打开、登录成功后列出账号", async () => {
    api.forgeAccounts.mockResolvedValue(empty);
    const code = { platform: "github", host: "github.com", userCode: "ABCD-1234", verificationUri: "https://github.com/login/device", verificationUriComplete: null, expiresAt: Date.now() + 15 * 60_000 };
    api.forgeLoginStart.mockResolvedValue(code);
    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
    render(<ForgeAccountList />);
    fireEvent.click(await screen.findByRole("button", { name: f.add }));
    expect(screen.getByRole("radio", { name: f.platformNames.github }).getAttribute("aria-checked")).toBe("true");
    expect((screen.getByLabelText(f.host) as HTMLInputElement).value).toBe("github.com");
    expect(screen.getByText(f.deviceDesc)).toBeTruthy();
    fireEvent.click(screen.getByTestId("forge-device-start"));
    expect((await screen.findByTestId("forge-code")).textContent).toBe("ABCD-1234");
    fireEvent.click(screen.getByRole("button", { name: f.copyAndOpen }));
    expect(writeText).toHaveBeenCalledWith("ABCD-1234");
    expect(api.openLink).toHaveBeenCalledWith("https://github.com/login/device");
    api.forgeAccounts.mockResolvedValue({ ...empty, accounts: [github] });
    await act(async () => events.handlers.get("forge-login-finished")?.({ payload: { state: "ok", error: null } }));
    expect(await screen.findByTestId("forge-account-github-github.com")).toBeTruthy();
    expect(screen.queryByTestId("forge-code")).toBeNull();
  });

  it("github.com 设备码：取消后收起代码；过期与拒绝说清楚", async () => {
    api.forgeAccounts.mockResolvedValue(empty);
    api.forgeLoginStart.mockResolvedValue({ platform: "github", host: "github.com", userCode: "WXYZ-9999", verificationUri: "https://github.com/login/device", verificationUriComplete: null, expiresAt: Date.now() + 60_000 });
    render(<ForgeAccountList />);
    fireEvent.click(await screen.findByRole("button", { name: f.add }));
    fireEvent.click(screen.getByTestId("forge-device-start"));
    await screen.findByTestId("forge-code");
    fireEvent.click(screen.getByRole("button", { name: f.cancel }));
    await waitFor(() => expect(api.forgeLoginCancel).toHaveBeenCalled());
    expect(screen.queryByTestId("forge-code")).toBeNull();
    await act(async () => events.handlers.get("forge-login-finished")?.({ payload: { state: "expired", error: null } }));
    await act(async () => events.handlers.get("forge-login-finished")?.({ payload: { state: "denied", error: null } }));
    expect(await screen.findByText(f.denied)).toBeTruthy();
  });

  it("github.com 也能改用访问令牌：写明权限、给生成页，按 github / github.com 添加", async () => {
    api.forgeAccounts.mockResolvedValue(empty);
    api.forgeAccountAdd.mockResolvedValue({ ...empty, accounts: [{ ...github, oauth: false }] });
    render(<ForgeAccountList />);
    fireEvent.click(await screen.findByRole("button", { name: f.add }));
    fireEvent.click(screen.getByTestId("forge-use-token"));
    expect(screen.getByText(f.scopes.github, { exact: false })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: f.createToken }));
    expect(api.openLink).toHaveBeenCalledWith("https://github.com/settings/tokens/new?description=Meowo&scopes=repo,read:org");
    fireEvent.change(screen.getByTestId("forge-token"), { target: { value: " ghp_abc " } });
    fireEvent.click(screen.getByTestId("forge-verify"));
    await waitFor(() => expect(api.forgeAccountAdd).toHaveBeenCalledWith("github", "github.com", "ghp_abc", null));
    expect((await screen.findByTestId("forge-account-github-github.com")).textContent).toContain(f.viaToken);
    // 令牌页里还能切回浏览器登录。
    expect(screen.queryByTestId("forge-add")).toBeNull();
  });

  it("GitHub 改成企业版域名：没有浏览器登录，只有令牌", async () => {
    api.forgeAccounts.mockResolvedValue(empty);
    render(<ForgeAccountList />);
    fireEvent.click(await screen.findByRole("button", { name: f.add }));
    fireEvent.change(screen.getByLabelText(f.host), { target: { value: "ghe.corp.example" } });
    expect(screen.queryByText(f.deviceDesc)).toBeNull();
    expect(screen.getByTestId("forge-token")).toBeTruthy();
    expect(screen.queryByTestId("forge-use-device")).toBeNull();
    expect(screen.getByText(f.hostFixed.github)).toBeTruthy();
  });

  it("gitlab.com 只用令牌（不再有浏览器登录）；自管实例也只有令牌", async () => {
    api.forgeAccounts.mockResolvedValue(empty);
    render(<ForgeAccountList />);
    fireEvent.click(await screen.findByRole("button", { name: f.add }));
    fireEvent.click(screen.getByRole("radio", { name: f.platformNames.gitlab }));
    expect((screen.getByLabelText(f.host) as HTMLInputElement).value).toBe("gitlab.com");
    expect(screen.queryByText(f.deviceDesc)).toBeNull();
    expect(screen.queryByTestId("forge-use-device")).toBeNull();
    expect(screen.getByText(f.scopes.gitlab, { exact: false })).toBeTruthy();
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

  it("Gitee：主机固定为 gitee.com，写明私人令牌的权限", async () => {
    api.forgeAccounts.mockResolvedValue(empty);
    api.forgeAccountAdd.mockResolvedValue(empty);
    render(<ForgeAccountList />);
    fireEvent.click(await screen.findByRole("button", { name: f.add }));
    fireEvent.click(screen.getByRole("radio", { name: f.platformNames.gitee }));
    const host = screen.getByLabelText(f.host) as HTMLInputElement;
    expect(host.disabled).toBe(true);
    expect(host.value).toBe("gitee.com");
    expect(screen.getByText(f.hostFixed.gitee)).toBeTruthy();
    expect(screen.getByText(f.scopes.gitee, { exact: false })).toBeTruthy();
    fireEvent.change(screen.getByTestId("forge-token"), { target: { value: "abc123" } });
    fireEvent.click(screen.getByTestId("forge-verify"));
    await waitFor(() => expect(api.forgeAccountAdd).toHaveBeenCalledWith("gitee", "gitee.com", "abc123", null));
  });

  it("退出前确认（浏览器登录的 github.com 写明去 GitHub 撤销授权），确认后删掉", async () => {
    api.forgeAccounts.mockResolvedValue({ ...empty, accounts: [github, gitlab] });
    api.forgeAccountRemove.mockResolvedValue({ ...empty, accounts: [gitlab] });
    confirm.appConfirm.mockResolvedValue(true);
    render(<ForgeAccountList />);
    fireEvent.click(within(await screen.findByTestId("forge-account-github-github.com")).getByRole("button", { name: f.signOut }));
    await waitFor(() => expect(confirm.appConfirm).toHaveBeenCalledWith(f.signOutConfirmDevice, expect.anything()));
    await waitFor(() => expect(api.forgeAccountRemove).toHaveBeenCalledWith("github", "github.com"));
    await waitFor(() => expect(screen.queryByTestId("forge-account-github-github.com")).toBeNull());
    api.forgeAccountRemove.mockResolvedValue(empty);
    fireEvent.click(within(screen.getByTestId("forge-account-gitlab-gitlab.com")).getByRole("button", { name: f.signOut }));
    await waitFor(() => expect(confirm.appConfirm).toHaveBeenCalledWith(f.signOutConfirm("GitLab", "gitlab.com"), expect.anything()));
    expect(await screen.findByTestId("forge-empty")).toBeTruthy();
  });

  it("主机输入归一、能不能浏览器登录、生成令牌的网页、代码剩余分钟", () => {
    expect(cleanHost(" https://Git.Example.com/group/ ")).toBe("git.example.com");
    expect(deviceLogin("github", "https://github.com/")).toBe(true);
    expect(deviceLogin("github", "ghe.corp.example")).toBe(false);
    expect(deviceLogin("gitlab", "gitlab.com")).toBe(false);
    expect(tokenPage("gitlab", "gitlab.com")).toBe("https://gitlab.com/-/user_settings/personal_access_tokens?name=Meowo&scopes=api");
    expect(tokenPage("github", "ghe.corp.example")).toBe("https://ghe.corp.example/settings/tokens/new?description=Meowo&scopes=repo,read:org");
    expect(tokenPage("bitbucket", "bitbucket.org")).toBe("https://id.atlassian.com/manage-profile/security/api-tokens");
    expect(tokenPage("gitea", "")).toBeNull();
    expect(tokenPage("gitee", "gitee.com")).toBe("https://gitee.com/profile/personal_access_tokens");
    expect(minutesLeft(60_000 * 14.2, 0)).toBe(15);
    expect(minutesLeft(0, 10)).toBe(1);
  });
});

describe("Git 组", () => {
  it("显示实际调用的 git 路径与版本、全局提交身份；改了才能保存，保存后回显", async () => {
    api.gitIdentitySet.mockResolvedValue({ ...git, userName: "Larry Go" });
    render(<GitToolCard />);
    const path = await screen.findByTestId("git-tool-path");
    expect(path.textContent).toContain("git 2.47.1.windows.1");
    expect(path.textContent).toContain("C:\\Program Files\\Git\\cmd\\git.exe");
    const name = screen.getByLabelText(g.userName) as HTMLInputElement;
    expect(name.value).toBe("Larry");
    expect((screen.getByLabelText(g.userEmail) as HTMLInputElement).value).toBe("larry@example.com");
    const save = screen.getByTestId("git-identity-save");
    expect(save.getAttribute("aria-disabled")).toBe("true");
    fireEvent.click(save);
    expect(api.gitIdentitySet).not.toHaveBeenCalled();
    fireEvent.change(name, { target: { value: "Larry Go" } });
    fireEvent.click(save);
    await waitFor(() => expect(api.gitIdentitySet).toHaveBeenCalledWith("Larry Go", "larry@example.com"));
    expect(await screen.findByText(g.saved)).toBeTruthy();
  });

  it("写失败给 git 的原话；校验不过说清楚", async () => {
    api.gitIdentitySet.mockRejectedValueOnce("git/config_failed: error: could not lock config file C:/Users/x/.gitconfig: File exists");
    render(<GitToolCard />);
    fireEvent.change(await screen.findByLabelText(g.userEmail), { target: { value: "new@example.com" } });
    fireEvent.click(screen.getByTestId("git-identity-save"));
    expect(await screen.findByText(/could not lock config file C:\/Users\/x\/\.gitconfig: File exists/)).toBeTruthy();
    api.gitIdentitySet.mockRejectedValueOnce("git/email_invalid");
    fireEvent.click(screen.getByTestId("git-identity-save"));
    expect(await screen.findByText("邮箱格式不对")).toBeTruthy();
  });

  it("找不到 git：说明影响、给下载链接与重新检测，不显示提交身份", async () => {
    api.gitToolStatus.mockResolvedValue({ found: false, path: null, version: null, userName: null, userEmail: null, error: "program not found" });
    render(<GitToolCard />);
    expect((await screen.findByTestId("git-tool-missing")).textContent).toBe(g.missing);
    expect(screen.getByText("program not found")).toBeTruthy();
    expect(screen.queryByTestId("git-identity")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: g.download }));
    expect(api.openLink).toHaveBeenCalledWith(GIT_DOWNLOAD_URL);
    api.gitToolStatus.mockResolvedValue(git);
    fireEvent.click(screen.getByRole("button", { name: g.recheck }));
    expect(await screen.findByTestId("git-identity")).toBeTruthy();
  });
});
