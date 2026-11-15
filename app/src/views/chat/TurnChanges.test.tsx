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
    expect(collectFileChanges(items, "C:/repo")).toEqual([{ path: "src/a.ts", added: 1, removed: 1 }]);
  });

  it("超过 3 个文件时折叠，点开看全部", () => {
    const items: ChatItem[] = ["a", "b", "c", "d", "e"].flatMap((name) => [edit(name, `src/${name}.ts`, "", "1"), done(name)]);
    render(<TurnChanges items={items} />);
    expect(screen.getByText(zh.chat.turnChangedFiles(5))).toBeTruthy();
    expect(document.querySelectorAll(".chat-turn-changes-row")).toHaveLength(3);
    fireEvent.click(screen.getByText(zh.chat.turnChangedMore(2)));
    expect(document.querySelectorAll(".chat-turn-changes-row")).toHaveLength(5);
  });

  it("没有改动文件时不渲染", () => {
    render(<TurnChanges items={[{ type: "tool_use", id: "t", timestamp: null, name: "Read", kind: "read", summary: "a.ts" }]} />);
    expect(document.querySelector(".chat-turn-changes")).toBeNull();
  });
});
