import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ChatItem } from "../../api";
import { collectFileChanges, TurnChanges } from "./TurnChanges";
import { zh } from "../../i18n/zh";

afterEach(cleanup);

function edit(id: string, path: string, oldText: string, newText: string): ChatItem {
  return { type: "tool_use", id, timestamp: null, name: "Edit", kind: "edit", summary: path, detail: { kind: "edit", path, old: oldText, new: newText, replace_all: false, truncated: false } };
}

function done(id: string, isError = false): ChatItem {
  return { type: "tool_result", id: `r-${id}`, timestamp: null, tool_use_id: id, text: "ok", is_error: isError };
}

describe("TurnChanges", () => {
  it("同一文件多次编辑合并累加，失败的编辑不计，路径相对 cwd", () => {
    const items: ChatItem[] = [
      edit("e1", "C:/repo/src/a.ts", "x", "x\ny"), done("e1"),
      edit("e2", "C:\\repo\\src\\a.ts", "p\nq", "p"), done("e2"),
      edit("e3", "C:/repo/b.ts", "1", "2"), done("e3", true),
    ];
    expect(collectFileChanges(items, "C:/repo")).toMatchObject([{ path: "src/a.ts", added: 1, removed: 1 }]);
    expect(collectFileChanges(items, "C:/repo")[0].details.map((detail) => detail.id)).toEqual(["e1", "e2"]);
  });

  it("超过 3 个文件时折叠，点开看全部", () => {
    const items: ChatItem[] = ["a", "b", "c", "d", "e"].flatMap((name) => [edit(name, `src/${name}.ts`, "", "1"), done(name)]);
    render(<TurnChanges items={items} />);
    expect(screen.getByText(zh.chat.turnChangedFiles(5))).toBeTruthy();
    expect(document.querySelectorAll(".chat-turn-changes-row")).toHaveLength(3);
    fireEvent.click(screen.getByText(zh.chat.turnChangedMore(2)));
    expect(document.querySelectorAll(".chat-turn-changes-row")).toHaveLength(5);
  });

  it("文件行展开本回合多次 diff，收起后卸载详情", () => {
    render(<TurnChanges items={[edit("a", "src/a.ts", "before", "after"), done("a"), edit("b", "src/a.ts", "after", "final"), done("b")]} />);
    const row = screen.getByRole("button", { name: /src\/a.ts/ });
    expect(row.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(row);
    expect(screen.getByRole("region", { name: "src/a.ts" })).toBeTruthy();
    expect(screen.getByText("第 1/2 次修改")).toBeTruthy();
    expect(screen.getByText("第 2/2 次修改")).toBeTruthy();
    expect(document.querySelectorAll(".chat-edit-card-code")).toHaveLength(2);
    fireEvent.click(row);
    expect(screen.queryByRole("region")).toBeNull();
  });

  it("写入内容和截断状态可查看，未返回的调用不冒充已确认", () => {
    render(<TurnChanges items={[{ type: "tool_use", id: "w", timestamp: null, name: "Write", kind: "write", summary: "new.txt", detail: { kind: "write", path: "new.txt", content: "hello detail", lines: 1, truncated: true } }]} />);
    fireEvent.click(screen.getByRole("button", { name: /new.txt/ }));
    expect(screen.getByText("hello detail")).toBeTruthy();
    expect(screen.getByText(zh.chat.toolTruncated)).toBeTruthy();
    expect(screen.getByText(zh.chat.turnChangePending)).toBeTruthy();
  });

  it("多文件补丁按文件展开，缺详情和截断不伪造正文", () => {
    const patch: ChatItem = { type: "tool_use", id: "patch", timestamp: null, name: "apply_patch", kind: "edit", summary: "2 files", detail: { kind: "patch", files: [
      { path: "old.ts", move_to: "moved.ts", op: "update", hunks: [{ old_start: 10, new_start: 10, lines: ["-old", "+new"] }], added: 1, removed: 1, truncated: true },
      { path: "deleted.ts", op: "delete", hunks: [], added: 0, removed: 20, truncated: false },
    ] } };
    render(<TurnChanges items={[patch, done("patch")]} />);
    fireEvent.click(screen.getByRole("button", { name: /moved.ts/ }));
    expect(screen.getByRole("region", { name: "moved.ts" })).toBeTruthy();
    expect(screen.getByText(zh.chat.toolTruncated)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /deleted.ts/ }));
    expect(screen.queryByRole("region", { name: "moved.ts" })).toBeNull();
    expect(screen.getByText(zh.chat.turnChangeMissing)).toBeTruthy();
    expect(screen.queryByText(zh.chat.turnChangePending)).toBeNull();
  });

  it("优先展示回执中的实际 diff，打断调用不纳入文件汇总", () => {
    const result = { ...done("a"), patch: [{ old_start: 42, new_start: 42, lines: ["-actual before", "+actual after"] }] } as ChatItem;
    const changes = collectFileChanges([edit("a", "a.ts", "requested before", "requested after"), result,
      edit("b", "b.ts", "x", "y"), { ...done("b"), interrupted: true } as ChatItem], null);
    expect(changes).toHaveLength(1);
    expect(changes[0].details[0].file.patch).toEqual([{ old_start: 42, new_start: 42, lines: ["-actual before", "+actual after"] }]);
  });

  it("没有改动文件时不渲染", () => {
    render(<TurnChanges items={[{ type: "tool_use", id: "t", timestamp: null, name: "Read", kind: "read", summary: "a.ts" }]} />);
    expect(document.querySelector(".chat-turn-changes")).toBeNull();
  });
});
