import { afterEach, describe, expect, it } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ChatItem } from "../../api";
import { TurnChanges } from "./TurnChanges";
import { revealItem, useRememberedOpen } from "./foldMemory";
import { zh } from "../../i18n/zh";

afterEach(cleanup);

const edit = (id: string, path: string): ChatItem =>
  ({ type: "tool_use", id, timestamp: null, name: "Edit", kind: "edit", summary: path, detail: { kind: "edit", path, old: "a", new: "a\nb", replace_all: false, truncated: false } });
const ok = (id: string, text = "ok", isError = false): ChatItem =>
  ({ type: "tool_result", id: `r-${id}`, timestamp: null, tool_use_id: id, text, is_error: isError });
const run = (id: string, command: string): ChatItem =>
  ({ type: "tool_use", id, timestamp: null, name: "Bash", kind: "run", summary: command, detail: { kind: "command", command, description: null } });

describe("本轮结果卡的检查段", () => {
  it("列出检查与结果；检查之后又改了代码时标出来", () => {
    render(<TurnChanges items={[
      edit("e1", "src/General.tsx"), ok("e1"),
      run("c1", "bunx tsc --noEmit"), ok("c1"),
      run("c2", "bunx vitest run src/views/settings"), ok("c2", " Tests  162 passed (162)"),
      edit("e2", "src/Row.tsx"), ok("e2"),
      edit("e3", "README.md"), ok("e3"),
    ]} />);
    const rows = [...document.querySelectorAll(".chat-turn-check")];
    expect(rows.map((row) => [row.className, row.querySelector(".chat-turn-check-kind")?.textContent, row.querySelector(".chat-turn-check-result")?.textContent])).toEqual([
      ["chat-turn-check is-pass is-stale", "类型检查", "通过 · 改动前跑的"],
      ["chat-turn-check is-pass is-stale", "测试", "162 个通过 · 改动前跑的"],
    ]);
    // README 是文档，不算「检查之后又改了」。
    expect(screen.getByText(zh.chat.checkStale(["类型检查", "测试"], ["Row.tsx"]))).toBeTruthy();
  });

  it("类型检查之后改了文件、只重跑了测试：类型检查那一行标「改动前跑的」，测试不标", () => {
    render(<TurnChanges items={[
      edit("e1", "src/a.ts"), ok("e1"),
      run("c1", "bunx tsc --noEmit"), ok("c1"),
      edit("e2", "src/b.ts"), ok("e2"),
      run("c2", "bunx vitest run"), ok("c2", "Tests  5 passed (5)"),
    ]} />);
    const rows = [...document.querySelectorAll(".chat-turn-check")];
    expect(rows.map((row) => row.querySelector(".chat-turn-check-result")?.textContent)).toEqual(["通过 · 改动前跑的", "5 个通过"]);
    expect(screen.getByText(zh.chat.checkStale(["类型检查"], ["b.ts"]))).toBeTruthy();
  });

  it("提示写成人话：哪项检查之后改了哪个文件；命令显示真正跑检查的那段，不显示开头的变量赋值", () => {
    render(<TurnChanges items={[
      edit("e1", "src/a.ts"), ok("e1"),
      run("c1", 'SP="C:/Users/x/Temp/claude/abc"; cd app && FORCE_COLOR=0 bunx vitest run src/derive'), ok("c1", "Tests  1 passed (1)"),
      edit("e2", "src/derive.ts"), ok("e2"),
    ]} />);
    expect(document.querySelector(".chat-turn-check-cmd")?.textContent).toBe("bunx vitest run src/derive");
    expect(document.querySelector(".chat-turn-checks-note")?.textContent).toBe("测试跑完后又改了 derive.ts，这处改动还没验证过");
  });

  it("改了代码却没跑检查：如实写；只改文档：不提", () => {
    render(<TurnChanges items={[edit("e1", "src/a.ts"), ok("e1")]} />);
    expect(screen.getByText(zh.chat.checkNone)).toBeTruthy();
    cleanup();
    render(<TurnChanges items={[edit("e1", "docs/guide.md"), ok("e1")]} />);
    expect(screen.queryByText(zh.chat.checkNone)).toBeNull();
  });

  it("只跑了检查、没改文件：卡片只有检查段，失败写出数量", () => {
    render(<TurnChanges items={[run("c1", "cargo test"), ok("c1", "test result: FAILED. 40 passed; 3 failed;", true)]} />);
    expect(screen.queryByText(zh.chat.turnChangedFiles(0))).toBeNull();
    const row = document.querySelector(".chat-turn-check")!;
    expect(row.classList.contains("is-fail")).toBe(true);
    expect(row.querySelector(".chat-turn-check-result")?.textContent).toBe("3 个失败");
  });

  it("没有改动也没有检查：不出卡片", () => {
    render(<TurnChanges items={[run("c1", "git status"), ok("c1")]} />);
    expect(document.querySelector(".chat-turn-changes")).toBeNull();
  });
});

function Fold({ id, ids }: { id: string; ids: string[] }) {
  const [open] = useRememberedOpen(`fold:${id}`, false, (target) => ids.includes(target));
  return <div data-testid={id}>{open ? "open" : "closed"}</div>;
}

describe("跳到这条命令", () => {
  it("已挂载的折叠：装着目标的展开，别的不动；按键名点到的也展开", () => {
    render(<><Fold id="a" ids={["t1"]} /><Fold id="b" ids={["t2"]} /><Fold id="c" ids={[]} /></>);
    act(() => revealItem("t1", ["fold:c"]));
    expect(screen.getByTestId("a").textContent).toBe("open");
    expect(screen.getByTestId("b").textContent).toBe("closed");
    expect(screen.getByTestId("c").textContent).toBe("open");
  });

  it("随后才挂载的内层折叠：挂载时认出目标就直接展开", () => {
    act(() => revealItem("t9", []));
    render(<Fold id="inner" ids={["t9"]} />);
    expect(screen.getByTestId("inner").textContent).toBe("open");
  });
});
