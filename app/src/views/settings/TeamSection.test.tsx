import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";

// 团队设置子页（设计稿 designer-1 C 版）：界面和个人层 config.json 是同一份数据，每改一处立即写；
// 文件被外部改过不覆盖；底线锁死；写坏的文件只读。
const api = vi.hoisted(() => ({
  teamPersonalConfig: vi.fn(),
  saveTeamPersonalConfig: vi.fn(),
  createTeamTaste: vi.fn(),
  openTeamPersonal: vi.fn(),
}));
vi.mock("../../api", async (o) => ({ ...(await o<typeof import("../../api")>()), ...api }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { TeamSection } from "./TeamSection";
import { zh } from "../../i18n/zh";
import type { TeamPersonalConfigDto } from "../../api";

const p = zh.settings.teamPage;
const dto = (over: Partial<TeamPersonalConfigDto> = {}): TeamPersonalConfigDto => ({
  personalDir: "/home/u/.meowo/team",
  configPath: "/home/u/.meowo/team/config.json",
  exists: true,
  error: null,
  errorLine: null,
  errorFirstLine: 0,
  errorLines: [],
  authorityDefaulted: false,
  token: "t1",
  taste: "/home/u/notes/taste.md",
  tasteExists: true,
  protected: ["acme-*", "secret-repo"],
  doAndReport: ["本地提交", "写测试"],
  askAtEnd: ["合并到主分支"],
  askFirst: [],
  floorMisplaced: [],
  blockedCommandCount: 1,
  ...over,
});

beforeEach(() => {
  api.teamPersonalConfig.mockResolvedValue(dto());
  api.saveTeamPersonalConfig.mockImplementation((args) => Promise.resolve(dto({ token: "t2", doAndReport: args.doAndReport, askAtEnd: args.askAtEnd, askFirst: args.askFirst, protected: args.protected })));
});
afterEach(() => {
  cleanup();
  Object.values(api).forEach((m) => m.mockReset());
});

describe("团队设置子页", () => {
  it("第一屏一句话讲清现在生效的设置，底线锁死没有换档按钮", async () => {
    render(<TeamSection onBack={() => {}} />);
    const say = (await screen.findByText(p.things(2))).closest("p")!;
    expect(say.textContent).toContain(p.things(1));
    expect(say.textContent).toContain(p.floorN(5));
    expect(say.textContent).toContain("taste.md");
    expect(say.textContent).toContain(p.dirsN(2));
    const bed = screen.getByText(p.floorHead).closest(".tpc-bed")!;
    for (const item of p.floorItems) within(bed as HTMLElement).getByText(item);
    expect(within(bed as HTMLElement).queryAllByRole("button")).toHaveLength(0);
    // 普通条目各有一个换档按钮
    expect(screen.getAllByRole("button", { name: p.move })).toHaveLength(3);
  });

  it("换档立即写文件，带上读到时的 token", async () => {
    render(<TeamSection onBack={() => {}} />);
    await screen.findByText("合并到主分支");
    const row = screen.getByText("合并到主分支").closest(".tpc-it")!;
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: p.move }));
    fireEvent.click(await screen.findByRole("menuitem", { name: p.moveTo(p.tierName.askFirst) }));
    await waitFor(() => expect(api.saveTeamPersonalConfig).toHaveBeenCalled());
    const args = api.saveTeamPersonalConfig.mock.calls[0][0];
    expect(args.token).toBe("t1");
    expect(args.askAtEnd).toEqual([]);
    expect(args.askFirst).toEqual(["合并到主分支"]);
  });

  it("加目录、删目录", async () => {
    render(<TeamSection onBack={() => {}} />);
    const input = await screen.findByPlaceholderText(p.dirPlaceholder);
    fireEvent.change(input, { target: { value: " client-x " } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(api.saveTeamPersonalConfig).toHaveBeenCalledTimes(1));
    expect(api.saveTeamPersonalConfig.mock.calls[0][0].protected).toEqual(["acme-*", "secret-repo", "client-x"]);
    fireEvent.click(screen.getByRole("button", { name: p.removeDir("acme-*") }));
    await waitFor(() => expect(api.saveTeamPersonalConfig).toHaveBeenCalledTimes(2));
  });

  it("文件在打开后被改过：不覆盖，说清两边各改了什么；叠上我的改动 = 在新内容上重放这一步", async () => {
    api.saveTeamPersonalConfig.mockRejectedValueOnce("team/config_changed");
    render(<TeamSection onBack={() => {}} />);
    const input = await screen.findByPlaceholderText(p.dirPlaceholder);
    api.teamPersonalConfig.mockResolvedValue(dto({ token: "t9", protected: ["acme-*", "secret-repo", "from-editor"] }));
    fireEvent.change(input, { target: { value: "client-x" } });
    fireEvent.keyDown(input, { key: "Enter" });
    const bar = await screen.findByRole("alertdialog", { name: p.conflictTitle });
    within(bar).getByText(p.diffDirsAdded("from-editor"));
    within(bar).getByText(p.dirAddedDesc("client-x"));
    fireEvent.click(within(bar).getByRole("button", { name: p.reapply }));
    await waitFor(() => expect(api.saveTeamPersonalConfig).toHaveBeenCalledTimes(2));
    const retry = api.saveTeamPersonalConfig.mock.calls[1][0];
    expect(retry.token).toBe("t9");
    expect(retry.protected).toEqual(["acme-*", "secret-repo", "from-editor", "client-x"]);
  });

  it("「用文件里的」丢掉这一步，不再写文件", async () => {
    api.saveTeamPersonalConfig.mockRejectedValueOnce("team/config_changed");
    render(<TeamSection onBack={() => {}} />);
    const input = await screen.findByPlaceholderText(p.dirPlaceholder);
    api.teamPersonalConfig.mockResolvedValue(dto({ token: "t9", protected: [] }));
    fireEvent.change(input, { target: { value: "client-x" } });
    fireEvent.keyDown(input, { key: "Enter" });
    const bar = await screen.findByRole("alertdialog");
    fireEvent.click(within(bar).getByRole("button", { name: p.useFile }));
    await screen.findByText(p.dirsEmpty);
    expect(api.saveTeamPersonalConfig).toHaveBeenCalledTimes(1);
  });

  it("写坏了：只读摆出行号和原文，没有会写文件的按钮", async () => {
    api.teamPersonalConfig.mockResolvedValue(dto({ error: "不是合法的 JSON：expected `,`", errorLine: 4, errorFirstLine: 2, errorLines: ['  "protected": [', '    "a"', '    "b"', "  ]"] }));
    render(<TeamSection onBack={() => {}} />);
    await screen.findByText(p.brokenAt(4));
    const rows = Array.from(document.querySelectorAll(".tp-code > div"));
    expect(rows.map((r) => r.textContent)).toEqual(['2  "protected": [', '3    "a"', '4    "b"', "5  ]"]);
    expect(rows[2].className).toBe("is-bad");
    expect(screen.queryByPlaceholderText(p.dirPlaceholder)).toBeNull();
    expect(screen.queryAllByRole("button", { name: p.move })).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: p.openToFix }));
    expect(api.openTeamPersonal).toHaveBeenCalledWith("config");
  });

  it("文件不存在：句子按默认值说，给「新建」；没有品味清单、没有目录时换说法", async () => {
    api.teamPersonalConfig.mockResolvedValue(dto({ exists: false, token: "", taste: null, tasteExists: false, protected: [], authorityDefaulted: true }));
    render(<TeamSection onBack={() => {}} />);
    const say = (await screen.findByText(p.noTaste, { selector: "button" })).closest("p")!;
    expect(say.textContent).toContain(p.anyDir);
    fireEvent.click(screen.getByRole("button", { name: p.create }));
    await waitFor(() => expect(api.saveTeamPersonalConfig).toHaveBeenCalledTimes(1));
    expect(api.saveTeamPersonalConfig.mock.calls[0][0].token).toBe("");
  });

  it("底线被手改进宽松档：那一条标「这里不生效」、没有换档按钮，可以一键移回去", async () => {
    api.teamPersonalConfig.mockResolvedValue(dto({ doAndReport: ["本地提交", "删除数据"], floorMisplaced: ["删除数据"] }));
    render(<TeamSection onBack={() => {}} />);
    const row = (await screen.findAllByText("删除数据")).map((n) => n.closest(".tpc-it")).find((n) => n?.classList.contains("is-void"))!;
    within(row as HTMLElement).getByText(p.voidTag);
    expect(within(row as HTMLElement).queryByRole("button")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: p.moveBack }));
    await waitFor(() => expect(api.saveTeamPersonalConfig).toHaveBeenCalled());
    expect(api.saveTeamPersonalConfig.mock.calls[0][0].doAndReport).toEqual(["本地提交"]);
  });
});
