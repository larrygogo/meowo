import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

// UI 调研 chat #9/#10/#11：审批卡短命令也挂「展开全部」且点了没变化；Bash 展开后前 3 行输出显示两遍
// 还写着「还有 N 行」；Read 失败的摘要写「读取了 1 行」。
import { ApprovalCard, ApprovalCommandDetail } from "../ApprovalCard";
import { BashTool } from "./BashTool";
import { ReadTool } from "./ReadTool";
import type { ToolResultItem, ToolUseItem } from "../shared";

const labels = { label: "完整参数", expandLabel: "展开全部", collapseLabel: "收起", copyLabel: "复制命令", copiedLabel: "已复制" };
const use = (name: string, detail: ToolUseItem["detail"]): ToolUseItem =>
  ({ type: "tool_use", id: "t1", timestamp: null, name, summary: "", detail } as ToolUseItem);
const res = (text: string, is_error = false): ToolResultItem =>
  ({ type: "tool_result", id: "r1", timestamp: null, tool_use_id: "t1", text, is_error } as ToolResultItem);

// jsdom 没有布局：按 pre 的文本行数伪造 scrollHeight，clientHeight 固定为限高 160。
function fakeLayout() {
  const sh = vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(function (this: HTMLElement) {
    return this.tagName === "PRE" ? (this.textContent ?? "").split("\n").length * 20 : 0;
  });
  const ch = vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(function (this: HTMLElement) {
    return this.tagName === "PRE" ? Math.min(160, (this.textContent ?? "").split("\n").length * 20) : 0;
  });
  return () => { sh.mockRestore(); ch.mockRestore(); };
}

afterEach(cleanup);

describe("审批卡命令详情", () => {
  it("短命令不溢出：只留复制，不挂点了没变化的展开全部", () => {
    const restore = fakeLayout();
    render(<ApprovalCard title="Agent 请求权限"><ApprovalCommandDetail command="git push origin main" {...labels} /></ApprovalCard>);
    expect(screen.queryByRole("button", { name: "展开全部" })).toBeNull();
    expect(screen.getByRole("button", { name: "复制命令" })).toBeTruthy();
    restore();
  });

  it("长命令溢出才给展开全部", () => {
    const restore = fakeLayout();
    const long = Array.from({ length: 30 }, (_, i) => `echo line ${i}`).join("\n");
    render(<ApprovalCard title="Agent 请求权限"><ApprovalCommandDetail command={long} {...labels} /></ApprovalCard>);
    expect(screen.getByRole("button", { name: "展开全部" })).toBeTruthy();
    restore();
  });
});

describe("工具行", () => {
  it("Bash 展开区不再重复前 3 行预览，也不写还有 N 行", () => {
    const out = Array.from({ length: 60 }, (_, i) => `row-${i}`).join("\n");
    const { container } = render(<BashTool item={use("Bash", { kind: "command", command: "ls", description: null } as ToolUseItem["detail"])} result={res(out)} />);
    const body = container.querySelector(".chat-tool-body")!;
    expect(body.textContent!.split("row-0").length - 1).toBe(1);
    expect(body.querySelector(".chat-tool-out-more")).toBeNull();
  });

  it("Read 失败的摘要写读取失败，不写读取了 N 行", () => {
    const { container } = render(<ReadTool item={use("Read", { kind: "read", path: "C:/x/missing.ts", offset: null, limit: null } as ToolUseItem["detail"])} result={res("ENOENT: no such file", true)} />);
    const count = container.querySelector("summary .chat-tool-count");
    expect(count?.textContent).toBe("读取失败");
  });
});
