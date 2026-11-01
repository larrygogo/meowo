import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ToolActivity, ActivityGroup, diffText, lineDelta } from "./ToolActivity";
import type { ToolResultItem, ToolUseItem } from "./shared";

afterEach(cleanup);

const write = (content: string, lines: number, truncated = false): ToolUseItem => ({
  type: "tool_use", id: "t1", timestamp: null, name: "Write", summary: "src/a.ts",
  detail: { kind: "write", path: "src/a.ts", content, lines, truncated },
});
const done: ToolResultItem = { type: "tool_result", id: "r1", timestamp: null, tool_use_id: "t1", text: "File created", is_error: false };

/** 写入/编辑在摘要行显示增删规模，展开卡片带文件头与带行号的正文预览。 */
describe("ToolActivity 详情", () => {
  it("Write：标题行写增行数，展开是带行号预览，长正文折成「还有 N 行」", () => {
    const body = Array.from({ length: 30 }, (_, i) => `const v${i} = ${i};`).join("\n");
    render(<ToolActivity item={write(body, 30)} result={done} />);
    expect(document.querySelector(".chat-tool-delta")?.textContent).toBe("+30");
    // 有回执的行挂载时不自动展开；内容仍在 DOM 里（details 只是收起）。
    expect(document.querySelector("details.chat-tool")?.hasAttribute("open")).toBe(false);
    expect(document.querySelectorAll(".chat-tool-line").length).toBe(12);
    expect(screen.getByText("v0", { exact: false })).toBeTruthy();
    const more = screen.getByRole("button", { name: "还有 18 行 · 展开全部" });
    fireEvent.click(more);
    expect(document.querySelectorAll(".chat-tool-line").length).toBe(30);
    expect(screen.getByRole("button", { name: "收起预览" })).toBeTruthy();
  });

  it("Write：后端截断的正文展开到底时提示只保留了开头", () => {
    const body = Array.from({ length: 20 }, (_, i) => `line ${i}`).join("\n");
    render(<ToolActivity item={write(body, 2000, true)} result={done} />);
    expect(document.querySelector(".chat-tool-delta")?.textContent).toBe("+2000");
    expect(screen.queryByText("内容过长，只保留了开头")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /展开全部/ }));
    expect(screen.getByText("内容过长，只保留了开头")).toBeTruthy();
  });

  it("Edit：原有行保留为上下文，只把新增的两行标绿且不重复结果文案", () => {
    const item: ToolUseItem = {
      type: "tool_use", id: "t2", timestamp: null, name: "Edit", summary: "x.rs",
      detail: { kind: "edit", path: "x.rs", old: "let a = 1;", new: "let a = 1;\nlet b = 2;\nlet c = 3;", replace_all: true, truncated: false },
    };
    render(<ToolActivity item={item} result={{ ...done, tool_use_id: "t2" }} />);
    expect(document.querySelector(".chat-edit-card-head")?.textContent).toContain("+2");
    expect(document.querySelector(".chat-edit-card-head")?.textContent).not.toContain("-1");
    expect(document.querySelector(".chat-tool-result")).toBeNull();
    expect(document.querySelectorAll(".chat-tool-udiff .chat-tool-line.is-del").length).toBe(0);
    expect(document.querySelectorAll(".chat-tool-udiff .chat-tool-line.is-add").length).toBe(2);
    expect(screen.getByText("全部替换")).toBeTruthy();
  });

  it("Read：Rust 文件内容按语法高亮", () => {
    const item: ToolUseItem = {
      type: "tool_use", id: "read-rs", timestamp: null, name: "Read", summary: "src/auth.rs",
      detail: { kind: "read", path: "src/auth.rs", offset: null, limit: null },
    };
    render(<ToolActivity item={item} result={{ ...done, tool_use_id: "read-rs", text: "let answer = 42;" }} />);
    expect(document.querySelector(".chat-tool-output .hljs-keyword")?.textContent).toBe("let");
    expect(document.querySelector(".chat-tool-state")).toBeNull();
  });

  it("Read：TypeScript 文件内容按语法高亮", () => {
    const item: ToolUseItem = {
      type: "tool_use", id: "read-ts", timestamp: null, name: "Read", summary: "src/a.ts",
      detail: { kind: "read", path: "src/a.ts", offset: null, limit: null },
    };
    render(<ToolActivity item={item} result={{ ...done, tool_use_id: "read-ts", text: "const answer: number = 42;" }} />);
    expect(document.querySelector(".chat-tool-output .hljs-keyword")?.textContent).toBe("const");
    expect(document.querySelector(".chat-tool-output .hljs-built_in")?.textContent).toBe("number");
  });

  it("其他工具：JSON 回执按键名与数字高亮", () => {
    const item: ToolUseItem = { type: "tool_use", id: "json", timestamp: null, name: "CustomTool", summary: "{}" };
    render(<ToolActivity item={item} result={{ ...done, tool_use_id: "json", text: '{"ok":true,"count":2}' }} />);
    expect(document.querySelector(".chat-tool-output .hljs-attr")?.textContent).toBe('"ok"');
    expect(document.querySelector(".chat-tool-output .hljs-number")?.textContent).toBe("2");
  });

  it("Bash：默认一行收起，展开后命令带 $ 记号，描述进悬停提示", () => {
    const item: ToolUseItem = {
      type: "tool_use", id: "t3", timestamp: null, name: "Bash", summary: "cargo test",
      detail: { kind: "command", command: "cargo test", description: "跑测试" },
    };
    render(<ToolActivity item={item} />);
    expect(document.querySelector(".chat-tool-state")).toBeTruthy();
    expect(document.querySelector("details.chat-tool")?.hasAttribute("open")).toBe(false);
    fireEvent.click(document.querySelector(".chat-tool > summary")!);
    expect(document.querySelector("details.chat-tool")?.hasAttribute("open")).toBe(true);
    expect(document.querySelector(".chat-tool-summary")?.getAttribute("data-tip")).toBe("跑测试");
    expect(document.querySelector(".chat-tool-ln")?.textContent).toBe("$");
    expect(screen.getByText("正在执行，结果尚未返回…")).toBeTruthy();
  });

  it("没有详情的工具行保持原样：只有摘要与回执", () => {
    const item: ToolUseItem = { type: "tool_use", id: "t4", timestamp: null, name: "TaskCreate", summary: "x" };
    render(<ToolActivity item={item} result={{ ...done, tool_use_id: "t4" }} />);
    expect(document.querySelector(".chat-tool-meta")).toBeNull();
    expect(document.querySelector(".chat-tool-code")).toBeNull();
    expect(document.querySelector("details.chat-tool")?.hasAttribute("open")).toBe(false);
  });

  it("活动组：挂载时按 defaultOpen 决定开合，此后同 id 沿用记忆", () => {
    const { unmount } = render(<ActivityGroup id="g1" className="chat-activity-group" defaultOpen><summary>a</summary></ActivityGroup>);
    expect(document.querySelector("details")?.hasAttribute("open")).toBe(true);
    unmount();
    render(<ActivityGroup id="g2" className="chat-activity-group" defaultOpen={false}><summary>b</summary></ActivityGroup>);
    expect(document.querySelector("details")?.hasAttribute("open")).toBe(false);
  });

  it("活动组：同 id 的重挂载沿用记忆，不被新的 defaultOpen 覆盖", () => {
    render(<ActivityGroup id="g3" className="chat-activity-group" defaultOpen><summary>a</summary></ActivityGroup>);
    expect(document.querySelector("details")?.hasAttribute("open")).toBe(true);
    // 流式追加会重建块：同 id 再挂一次，读到的仍是记忆里那份（此处 true），而不是本次
    // 传进来的 defaultOpen=false。
    render(<ActivityGroup id="g3" className="chat-activity-group" defaultOpen={false}><summary>a</summary></ActivityGroup>);
    const all = document.querySelectorAll("details.chat-activity-group");
    expect(all[all.length - 1]?.hasAttribute("open")).toBe(true);
  });

  it("lineDelta：Edit 按真实增删行数、Write 只算新增，其余工具不计", () => {
    expect(lineDelta({ kind: "edit", path: "a.rs", old: "x\ny\nz\n", new: "x\n", replace_all: false, truncated: false })).toEqual({ added: 0, removed: 2 });
    expect(lineDelta({ kind: "write", path: "a.rs", content: "", lines: 225, truncated: false })).toEqual({ added: 225, removed: 0 });
    expect(lineDelta({ kind: "command", command: "ls", description: null })).toBeNull();
    expect(lineDelta(null)).toBeNull();
  });

  it("diffText：三行中只改一行时是 -1 +1，另外两行保留为上下文", () => {
    const diff = diffText(
      "timestamp,\nname,\n// comment",
      "timestamp,\nname: name.clone(),\n// comment",
    );
    expect(diff).toEqual({
      added: 1,
      removed: 1,
      hunks: [{
        old_start: 1,
        new_start: 1,
        lines: [" timestamp,", "-name,", "+name: name.clone(),", " // comment"],
      }],
    });
  });

  it("diffText：相隔较远的两处修改分成两个区块", () => {
    const diff = diffText("a\nb\nc\nd\ne\nf\ng\nh\ni", "a\nB\nc\nd\ne\nf\ng\nh\nI");
    expect(diff.added).toBe(2);
    expect(diff.removed).toBe(2);
    expect(diff.hunks).toHaveLength(2);
  });
});
