import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";

// 团队设置子页（设计稿 designer-1 C 版）：界面和个人层 config.json 是同一份数据；改动先留在页面上，
// 点「保存」才写；文件被外部改过不覆盖；底线锁死；写坏的文件只读。
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
  floorPrefixes: ["上生产", "花钱", "删除数据", "对外发消息", "deploy to prod"],
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

  it("换档先留在页面上，点保存才写文件，带上读到时的 token", async () => {
    render(<TeamSection onBack={() => {}} />);
    await screen.findByText("合并到主分支");
    const row = screen.getByText("合并到主分支").closest(".tpc-it")!;
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: p.move }));
    fireEvent.click(await screen.findByRole("menuitem", { name: p.moveTo(p.tierName.askFirst) }));
    await screen.findByText(p.dirty(1));
    expect(api.saveTeamPersonalConfig).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: p.save }));
    await waitFor(() => expect(api.saveTeamPersonalConfig).toHaveBeenCalled());
    const args = api.saveTeamPersonalConfig.mock.calls[0][0];
    expect(args.token).toBe("t1");
    expect(args.askAtEnd).toEqual([]);
    expect(args.askFirst).toEqual(["合并到主分支"]);
    await waitFor(() => expect(screen.queryByText(p.dirty(1))).toBeNull());
  });

  it("「放弃」把页面恢复成文件里的样子，不写文件", async () => {
    render(<TeamSection onBack={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: p.removeDir("acme-*") }));
    expect(screen.queryByText("acme-*")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: p.discard }));
    await screen.findByText("acme-*");
    expect(screen.queryByText(p.dirty(1))).toBeNull();
    expect(api.saveTeamPersonalConfig).not.toHaveBeenCalled();
  });

  it("新增条目只在按回车时加入，失焦不加入", async () => {
    render(<TeamSection onBack={() => {}} />);
    await screen.findByText("合并到主分支");
    fireEvent.click(screen.getAllByRole("button", { name: p.addItem })[0]);
    const input = screen.getByPlaceholderText(p.addItemPlaceholder);
    fireEvent.change(input, { target: { value: "半句" } });
    fireEvent.blur(input);
    expect(screen.queryByText(p.dirty(1))).toBeNull();
    fireEvent.change(input, { target: { value: "跑基准测试" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await screen.findByText(p.dirty(1));
    screen.getByText("跑基准测试");
  });

  it("加目录、删目录", async () => {
    render(<TeamSection onBack={() => {}} />);
    const input = await screen.findByPlaceholderText(p.dirPlaceholder);
    fireEvent.change(input, { target: { value: " client-x " } });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.click(await screen.findByRole("button", { name: p.removeDir("acme-*") }));
    await screen.findByText(p.dirty(2));
    fireEvent.click(screen.getByRole("button", { name: p.save }));
    await waitFor(() => expect(api.saveTeamPersonalConfig).toHaveBeenCalledTimes(1));
    expect(api.saveTeamPersonalConfig.mock.calls[0][0].protected).toEqual(["secret-repo", "client-x"]);
  });

  it("文件在打开后被改过：不覆盖，说清两边各改了什么；叠上我的改动 = 在新内容上重放这几处", async () => {
    api.saveTeamPersonalConfig.mockRejectedValueOnce("team/config_changed");
    render(<TeamSection onBack={() => {}} />);
    const input = await screen.findByPlaceholderText(p.dirPlaceholder);
    api.teamPersonalConfig.mockResolvedValue(dto({ token: "t9", protected: ["acme-*", "secret-repo", "from-editor"] }));
    fireEvent.change(input, { target: { value: "client-x" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.change(input, { target: { value: "client-y" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.click(await screen.findByRole("button", { name: p.save }));
    const bar = await screen.findByRole("alertdialog", { name: p.conflictTitle });
    within(bar).getByText(p.diffDirsAdded("from-editor"));
    within(bar).getByText(p.dirAddedDesc("client-x") + "，" + p.dirAddedDesc("client-y"));
    fireEvent.click(within(bar).getByRole("button", { name: p.reapply }));
    await waitFor(() => expect(api.saveTeamPersonalConfig).toHaveBeenCalledTimes(2));
    const retry = api.saveTeamPersonalConfig.mock.calls[1][0];
    expect(retry.token).toBe("t9");
    expect(retry.protected).toEqual(["acme-*", "secret-repo", "from-editor", "client-x", "client-y"]);
  });

  it("「用文件里的」丢掉页面上的改动，不再写文件", async () => {
    api.saveTeamPersonalConfig.mockRejectedValueOnce("team/config_changed");
    render(<TeamSection onBack={() => {}} />);
    const input = await screen.findByPlaceholderText(p.dirPlaceholder);
    api.teamPersonalConfig.mockResolvedValue(dto({ token: "t9", protected: [] }));
    fireEvent.change(input, { target: { value: "client-x" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.click(await screen.findByRole("button", { name: p.save }));
    const bar = await screen.findByRole("alertdialog");
    fireEvent.click(within(bar).getByRole("button", { name: p.useFile }));
    await screen.findByText(p.dirsEmpty);
    expect(api.saveTeamPersonalConfig).toHaveBeenCalledTimes(1);
    expect(screen.queryByText(p.dirty(1))).toBeNull();
  });

  it("写坏了：只读摆出行号和原文，缺逗号时连上一行一起标出，没有会写文件的按钮", async () => {
    api.teamPersonalConfig.mockResolvedValue(dto({ error: "不是合法的 JSON：expected `,`", errorLine: 4, errorFirstLine: 2, errorLines: ['  "protected": [', '    "a"', '    "b"', "  ]"] }));
    render(<TeamSection onBack={() => {}} />);
    await screen.findByText(p.brokenAt(4));
    const rows = Array.from(document.querySelectorAll(".tp-code > div"));
    expect(rows.map((r) => r.textContent)).toEqual(['2  "protected": [', '3    "a"', '4    "b"', "5  ]"]);
    expect(rows.map((r) => r.className)).toEqual(["", "is-bad", "is-bad", ""]);
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

  it("底线被手改进宽松档：那一条标「这里不生效」，照样能换档；「移回去」是移到动手前先问，不是删掉", async () => {
    api.teamPersonalConfig.mockResolvedValue(dto({ doAndReport: ["本地提交", "删除数据（清理测试库）"] }));
    render(<TeamSection onBack={() => {}} />);
    const row = (await screen.findAllByText("删除数据（清理测试库）")).map((n) => n.closest(".tpc-it")).find((n) => n?.classList.contains("is-void"))!;
    within(row as HTMLElement).getByText(p.voidTag);
    within(row as HTMLElement).getByRole("button", { name: p.move });
    fireEvent.click(screen.getByRole("button", { name: p.moveBack }));
    fireEvent.click(await screen.findByRole("button", { name: p.save }));
    await waitFor(() => expect(api.saveTeamPersonalConfig).toHaveBeenCalled());
    const args = api.saveTeamPersonalConfig.mock.calls[0][0];
    expect(args.doAndReport).toEqual(["本地提交"]);
    expect(args.askFirst).toEqual(["删除数据（清理测试库）"]);
  });

  it("冲突后叠上：删除按文字找条目，外部在前面插了一项也删的是同一条", async () => {
    api.teamPersonalConfig.mockResolvedValue(dto({ protected: ["a-repo", "b-repo", "c-repo"], doAndReport: ["本地提交", "写测试", "跑基准"] }));
    api.saveTeamPersonalConfig.mockRejectedValueOnce("team/config_changed");
    render(<TeamSection onBack={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: p.removeDir("c-repo") }));
    const row = screen.getByText("跑基准").closest(".tpc-it") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: p.move }));
    fireEvent.click(await screen.findByRole("menuitem", { name: p.remove }));
    api.teamPersonalConfig.mockResolvedValue(dto({ token: "t9", protected: ["from-editor", "a-repo", "b-repo", "c-repo"], doAndReport: ["编辑器里加的", "本地提交", "写测试", "跑基准"] }));
    fireEvent.click(screen.getByRole("button", { name: p.save }));
    const bar = await screen.findByRole("alertdialog");
    fireEvent.click(within(bar).getByRole("button", { name: p.reapply }));
    await waitFor(() => expect(api.saveTeamPersonalConfig).toHaveBeenCalledTimes(2));
    const retry = api.saveTeamPersonalConfig.mock.calls[1][0];
    expect(retry.protected).toEqual(["from-editor", "a-repo", "b-repo"]);
    expect(retry.doAndReport).toEqual(["编辑器里加的", "本地提交", "写测试"]);
  });

  it("「动手前先问」里写着底线原文：并到锁组下、只能删不能换档，不删就原样写回", async () => {
    api.teamPersonalConfig.mockResolvedValue(dto({ askFirst: ["上生产", "花钱：开机器、买资源", "给客户发消息前先给我看草稿"] }));
    render(<TeamSection onBack={() => {}} />);
    const say = (await screen.findByText(p.floorPlus(5, 1))).closest("p")!;
    expect(say).toBeTruthy();
    const bed = screen.getByText(p.floorHead).closest(".tpc-bed") as HTMLElement;
    expect(within(bed).getAllByText(p.echoTag)).toHaveLength(2);
    // 内置的五条没有按钮，重复的两条各有一个只含「删掉」的菜单
    expect(within(bed).queryAllByRole("button")).toHaveLength(2);
    // 能换档的只有用户自己的那条
    const rung = screen.getByText(p.tierName.askFirst).closest(".tpc-rung") as HTMLElement;
    expect(within(rung).getAllByRole("button", { name: p.move })).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: p.removeDir("acme-*") }));
    fireEvent.click(screen.getByRole("button", { name: p.save }));
    await waitFor(() => expect(api.saveTeamPersonalConfig).toHaveBeenCalled());
    expect(api.saveTeamPersonalConfig.mock.calls[0][0].askFirst).toEqual(["上生产", "花钱：开机器、买资源", "给客户发消息前先给我看草稿"]);
  });

  it("开头碰巧是底线说法的普通规则照常可编辑，不并进锁组、不标不生效", async () => {
    api.teamPersonalConfig.mockResolvedValue(dto({ doAndReport: ["删除数据库表前先导出备份"], askFirst: ["花钱超过 100 元先问", "上生产前先跑冒烟"] }));
    render(<TeamSection onBack={() => {}} />);
    for (const text of ["删除数据库表前先导出备份", "花钱超过 100 元先问", "上生产前先跑冒烟"]) {
      const row = (await screen.findByText(text)).closest(".tpc-it") as HTMLElement;
      expect(row.classList.contains("is-echo") || row.classList.contains("is-void")).toBe(false);
      within(row).getByRole("button", { name: p.move });
    }
    expect(screen.queryByText(p.echoTag)).toBeNull();
  });

  it("冲突重放找不到第几个同名条目就跳过，不退而删掉另一份", async () => {
    api.teamPersonalConfig.mockResolvedValue(dto({ protected: ["a-repo", "x", "a-repo"] }));
    api.saveTeamPersonalConfig.mockRejectedValueOnce("team/config_changed");
    render(<TeamSection onBack={() => {}} />);
    await screen.findByText("x");
    fireEvent.click(screen.getAllByRole("button", { name: p.removeDir("a-repo") })[1]);
    api.teamPersonalConfig.mockResolvedValue(dto({ token: "t9", protected: ["a-repo", "x"] }));
    fireEvent.click(screen.getByRole("button", { name: p.save }));
    fireEvent.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: p.reapply }));
    await waitFor(() => expect(api.saveTeamPersonalConfig).toHaveBeenCalledTimes(2));
    expect(api.saveTeamPersonalConfig.mock.calls[1][0].protected).toEqual(["a-repo", "x"]);
  });

  it("有没保存的改动时文件被写坏：写坏页说明改动没写进去，并给「放弃」", async () => {
    api.teamPersonalConfig.mockResolvedValue(dto({ taste: null, tasteExists: false }));
    api.createTeamTaste.mockRejectedValueOnce("team/config_changed");
    render(<TeamSection onBack={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: p.removeDir("acme-*") }));
    api.teamPersonalConfig.mockResolvedValue(dto({ error: "不是合法的 JSON", errorLine: 3, errorFirstLine: 1, errorLines: ["{", "  x", "}"] }));
    fireEvent.click(screen.getByRole("button", { name: p.newTaste }));
    await screen.findByText(p.brokenPending(1));
    fireEvent.click(screen.getByRole("button", { name: p.discard }));
    expect(screen.queryByText(p.brokenPending(1))).toBeNull();
  });

  it("新加进宽松档的底线说法当场标「这里不生效」，不用等保存", async () => {
    render(<TeamSection onBack={() => {}} />);
    await screen.findByText("合并到主分支");
    fireEvent.click(screen.getAllByRole("button", { name: p.addItem })[0]);
    const input = screen.getByPlaceholderText(p.addItemPlaceholder);
    fireEvent.change(input, { target: { value: "Deploy to prod 周五" } });
    fireEvent.keyDown(input, { key: "Enter" });
    const row = (await screen.findByText("Deploy to prod 周五")).closest(".tpc-it") as HTMLElement;
    expect(row.classList.contains("is-void")).toBe(true);
    within(row).getByText(p.voidTag);
  });

  it("新建品味清单：连点只调一次；遇到外部改动如实重读，没保存的改动还在", async () => {
    api.teamPersonalConfig.mockResolvedValue(dto({ taste: null, tasteExists: false }));
    let release: (v: unknown) => void = () => {};
    api.createTeamTaste.mockImplementationOnce(() => new Promise((_, reject) => { release = () => reject("team/config_changed"); }));
    render(<TeamSection onBack={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: p.removeDir("acme-*") }));
    const btn = screen.getByRole("button", { name: p.newTaste });
    fireEvent.click(btn);
    fireEvent.click(btn);
    expect(api.createTeamTaste).toHaveBeenCalledTimes(1);
    const reads = api.teamPersonalConfig.mock.calls.length;
    release(null);
    await screen.findByText(p.tasteRetry);
    expect(api.teamPersonalConfig.mock.calls.length).toBe(reads + 1);
    screen.getByText(p.dirty(1));
  });

  it("同名条目各删各的，不会一删删两条", async () => {
    api.teamPersonalConfig.mockResolvedValue(dto({ doAndReport: ["写测试", "写测试"] }));
    render(<TeamSection onBack={() => {}} />);
    const rows = (await screen.findAllByText("写测试")).map((n) => n.closest(".tpc-it") as HTMLElement);
    fireEvent.click(within(rows[1]).getByRole("button", { name: p.move }));
    fireEvent.click(await screen.findByRole("menuitem", { name: p.remove }));
    fireEvent.click(await screen.findByRole("button", { name: p.save }));
    await waitFor(() => expect(api.saveTeamPersonalConfig).toHaveBeenCalled());
    expect(api.saveTeamPersonalConfig.mock.calls[0][0].doAndReport).toEqual(["写测试"]);
  });
});
