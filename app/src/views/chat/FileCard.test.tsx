import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatItem } from "../../api";
import { fileKind, formatSize, parseFileReceipt, parseNotifyReceipt } from "./FileCard";
import { ToolActivity } from "./ToolActivity";

let remote = false;
vi.mock("../../remoteMode", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../remoteMode")>()),
  remoteUi: () => remote,
}));
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  convertFileSrc: (path: string) => (remote ? `/file?path=${encodeURIComponent(path)}&token=t` : `asset://local/${path}`),
}));
const api = vi.hoisted(() => ({
  openDeliveredFile: vi.fn(async () => {}),
  revealDeliveredFile: vi.fn(async () => {}),
  saveDeliveredFile: vi.fn(async () => {}),
  deliveredFileExists: vi.fn(async () => true),
}));
vi.mock("../../api", async (importOriginal) => ({ ...(await importOriginal<typeof import("../../api")>()), ...api }));
const dialog = vi.hoisted(() => ({ save: vi.fn(async () => "D:\\下载\\Meowo-setup.exe") }));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);

beforeEach(() => {
  remote = false;
  Object.values(api).forEach((fn) => fn.mockClear());
  api.deliveredFileExists.mockResolvedValue(true);
});
afterEach(cleanup);

const SNAP = "C:\\Users\\me\\.meowo\\files\\sent\\0123456789abcdef\\Meowo-setup.exe";
const call = (id: string, name: string, kind = "other"): Extract<ChatItem, { type: "tool_use" }> =>
  ({ type: "tool_use", id, timestamp: null, name, kind, summary: "" } as Extract<ChatItem, { type: "tool_use" }>);
const result = (id: string, text: string, isError = false): Extract<ChatItem, { type: "tool_result" }> =>
  ({ type: "tool_result", id: `r-${id}`, timestamp: null, tool_use_id: id, text, is_error: isError });
const fileReceipt = (extra: Record<string, unknown> = {}) =>
  `文件已交给用户：Meowo-setup.exe（21.2 MB）。\n[File: ${JSON.stringify({ name: "Meowo-setup.exe", size: 22235771, path: SNAP, sha256: "0123456789abcdef", caption: "装好后试试 Tab", ...extra })}]`;
const notifyReceipt = (outcome: string, level = "done") =>
  `已提醒用户。\n[Notify: ${JSON.stringify({ level, message: "打包好了，装上试试", outcome })}]`;

describe("回执解析", () => {
  it("文件卡：名字、大小、快照路径、说明；坏了返回 null", () => {
    expect(parseFileReceipt(fileReceipt())).toEqual({ name: "Meowo-setup.exe", size: 22235771, path: SNAP, caption: "装好后试试 Tab" });
    expect(parseFileReceipt("[File: {坏了]")).toBeNull();
    expect(parseFileReceipt("没有摘要")).toBeNull();
  });

  it("提醒：级别、内容、去向；认不得的级别按「做完了」", () => {
    expect(parseNotifyReceipt(notifyReceipt("viewing", "needs_you"))).toEqual({ level: "needs_you", message: "打包好了，装上试试", outcome: "viewing" });
    expect(parseNotifyReceipt(notifyReceipt("shown", "bogus"))?.level).toBe("done");
  });

  it("类型与大小写成人话", () => {
    expect(fileKind("a.exe")).toBe("installer");
    expect(fileKind("logs.tar.gz")).toBe("archive");
    expect(fileKind("报告.PDF")).toBe("pdf");
    expect(fileKind("README")).toBe("other");
    expect(formatSize(512)).toBe("512 B");
    expect(formatSize(22235771)).toBe("21.2 MB");
  });
});

describe("文件卡", () => {
  it("桌面：显示名字、类型 · 大小、说明；打开、在文件夹中显示、另存为都只交快照路径", async () => {
    render(<ToolActivity item={call("f1", "mcp__meowo__send_file")} result={result("f1", fileReceipt())} />);
    expect(screen.getByText("Meowo-setup.exe")).toBeTruthy();
    expect(screen.getByText("安装包 · 21.2 MB")).toBeTruthy();
    expect(screen.getByText("装好后试试 Tab")).toBeTruthy();
    fireEvent.click(screen.getByText("打开"));
    expect(api.openDeliveredFile).toHaveBeenCalledWith(SNAP);
    fireEvent.click(screen.getByRole("button", { name: "在文件夹中显示" }));
    expect(api.revealDeliveredFile).toHaveBeenCalledWith(SNAP);
    fireEvent.click(screen.getByRole("button", { name: "另存为…" }));
    await waitFor(() => expect(api.saveDeliveredFile).toHaveBeenCalledWith(SNAP, "D:\\下载\\Meowo-setup.exe"));
  });

  it("快照被清理了：写明已清理，不给按钮", async () => {
    api.deliveredFileExists.mockResolvedValue(false);
    render(<ToolActivity item={call("f2", "mcp__meowo__send_file")} result={result("f2", fileReceipt())} />);
    await waitFor(() => expect(screen.getByText(/已清理/)).toBeTruthy());
    expect(screen.queryByText("打开")).toBeNull();
  });

  it("手机：一个「下载」链接，走 /download（只认交付目录、流式），带原文件名", () => {
    remote = true;
    render(<ToolActivity item={call("f3", "mcp__meowo__send_file")} result={result("f3", fileReceipt())} />);
    const link = screen.getByText("下载").closest("a")!;
    expect(link.getAttribute("href")).toBe(`/download?path=${encodeURIComponent(SNAP)}&token=t`);
    expect(link.getAttribute("download")).toBe("Meowo-setup.exe");
    expect(screen.queryByText("打开")).toBeNull();
    expect(api.deliveredFileExists).not.toHaveBeenCalled();
  });

  it("手机：点下载时快照已被回收（404），卡片改写「已清理」，不跳去报错页", async () => {
    remote = true;
    const fetchMock = vi.fn(async () => new Response(null, { status: 404 }));
    vi.stubGlobal("fetch", fetchMock);
    render(<ToolActivity item={call("f5", "mcp__meowo__send_file")} result={result("f5", fileReceipt())} />);
    fireEvent.click(screen.getByText("下载"));
    await waitFor(() => expect(screen.getByText(/已清理/)).toBeTruthy());
    expect(fetchMock).toHaveBeenCalledWith(`/download?path=${encodeURIComponent(SNAP)}&token=t`, { method: "HEAD" });
    vi.unstubAllGlobals();
  });

  it("失败回执写原因；别的工具输出里同样格式的行不会画成文件卡", () => {
    render(<ToolActivity item={call("f4", "mcp__meowo__send_file")} result={result("f4", "这是一个文件夹：C:\\a。先把文件夹打成 zip 再交", true)} />);
    expect(screen.getByText("文件没能交过来")).toBeTruthy();
    expect(screen.getByText(/先把文件夹打成 zip/)).toBeTruthy();
    cleanup();
    render(<ToolActivity item={{ ...call("b1", "Bash", "run"), detail: { kind: "command", command: "cat x", description: null } } as Extract<ChatItem, { type: "tool_use" }>} result={result("b1", fileReceipt())} />);
    expect(document.querySelector("[data-testid='file-card']")).toBeNull();
  });
});

describe("提醒行", () => {
  it("弹出来了：只写「提醒了你 · 做完了」和内容", () => {
    render(<ToolActivity item={call("n1", "mcp__meowo__notify_user")} result={result("n1", notifyReceipt("shown"))} />);
    const line = screen.getByTestId("notify-line");
    expect(line.textContent).toContain("提醒了你");
    expect(line.textContent).toContain("做完了");
    expect(line.textContent).toContain("打包好了，装上试试");
    expect(line.textContent).not.toContain("没弹");
  });

  it("没弹出来的写明为什么；需要你决定的单独标出", () => {
    render(<ToolActivity item={call("n2", "mcp__meowo__notify_user")} result={result("n2", notifyReceipt("viewing", "done"))} />);
    expect(screen.getByTestId("notify-line").textContent).toContain("你正在看，没弹通知");
    cleanup();
    render(<ToolActivity item={call("n3", "mcp__meowo__notify_user")} result={result("n3", notifyReceipt("shown", "needs_you"))} />);
    const line = screen.getByTestId("notify-line");
    expect(line.getAttribute("data-level")).toBe("needs_you");
    expect(line.textContent).toContain("需要你决定");
    expect(line.textContent).not.toContain("没弹");
  });
});
