import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ChatItem } from "../../api";
import { ChatCwdContext, relativize, ToolActivity, toolKind } from "./ToolActivity";
import { Transcript } from "./Transcript";
import { WelcomePanel, type WelcomeInfo } from "./WelcomePanel";
import { stripAnsi } from "./shared";
import { CommandLines, dedent, OutputBlock } from "./terminalText";

afterEach(cleanup);

const agent = { provider: "claude", name: "Claude Code", model: "Opus 5.5" };
const user = (id: string, text = "问一句"): ChatItem => ({ type: "user_text", id, timestamp: null, text });
const reply = (id: string, text = "答一句"): ChatItem => ({ type: "assistant_text", id, timestamp: null, text });
const result = (id: string, text: string, extra: Partial<Extract<ChatItem, { type: "tool_result" }>> = {}) =>
  ({ type: "tool_result" as const, id: `r-${id}`, timestamp: null, tool_use_id: id, text, is_error: false, ...extra });
const ESC = "\u001b";

/** 工具行照搬 claude 终端：首行「动词(参数)」，次行「⎿ 结果摘要」，状态点读 data-state。 */
describe("终端式工具行", () => {
  it("Bash(命令)；没回执是 running 且次行在计时，失败回执是 error；中文名留给读屏", () => {
    const item = { type: "tool_use" as const, id: "t1", timestamp: null, name: "Bash", summary: "ls" };
    const { rerender } = render(<ToolActivity item={item} />);
    const verb = document.querySelector(".chat-tool-verb")!;
    expect(verb.textContent).toBe("Bash");
    expect(verb.getAttribute("data-kind")).toBe("run");
    expect(document.querySelector(".chat-tool-call")?.textContent).toBe("Bash运行终端(ls)");
    expect(document.querySelector(".chat-tool")?.getAttribute("data-state")).toBe("running");
    expect(document.querySelector(".chat-tool-out.is-running")?.textContent).toMatch(/^运行中… \(\d+s\)$/);
    expect(screen.getByText("运行终端").className).toContain("chat-sronly");
    rerender(<ToolActivity item={item} result={result("t1", ["boom", "more"].join("\n"), { is_error: true })} />);
    expect(document.querySelector(".chat-tool")?.getAttribute("data-state")).toBe("error");
    expect(document.querySelector(".chat-tool-out.is-error")?.textContent).toBe("boom");
  });

  it("命令描述不占行（进悬停提示）；输出露头 3 行，余下折成「还有 N 行」；着色码剥掉", () => {
    expect(stripAnsi(`${ESC}[2m Tests ${ESC}[22m ${ESC}[1m${ESC}[32m316 passed${ESC}[39m`)).toBe(" Tests  316 passed");
    expect(stripAnsi(`${ESC}]8;;https://x\u0007link${ESC}]8;;\u0007`)).toBe("link");
    expect(stripAnsi("纯文本")).toBe("纯文本");
    const item = {
      type: "tool_use" as const, id: "t3", timestamp: null, name: "Bash", summary: "vitest",
      detail: { kind: "command" as const, command: "vitest", description: "Run tests" },
    };
    render(<ToolActivity item={item} result={result("t3", [`${ESC}[32ma${ESC}[39m`, "b", "c", "d", "e"].join("\n"))} />);
    expect(document.querySelector(".chat-tool-meta")).toBeNull();
    expect(document.querySelector(".chat-tool-call")?.getAttribute("data-tip")).toBe("Run tests");
    expect(document.querySelector(".chat-tool-out")?.textContent).toBe(["a", "b", "c", "… 还有 2 行"].join("\n"));
    expect(document.querySelector(".chat-tool-body pre")?.textContent).toBe(["a", "b", "c", "d", "e"].join("\n"));
  });

  it("Edit 叫 Update，带 structuredPatch 时按真实行号渲染统一 diff，默认展开", () => {
    const item = {
      type: "tool_use" as const, id: "t4", timestamp: null, name: "Edit", summary: "C:/repo/src/a.ts",
      detail: { kind: "edit" as const, path: "C:/repo/src/a.ts", old: "old", new: "new", replace_all: false, truncated: false },
    };
    render(
      <ChatCwdContext.Provider value={"C:\\repo"}>
        <ToolActivity item={item} result={result("t4", "ok", { patch: [{ old_start: 26, new_start: 26, lines: [" keep", "-old", "+new", " tail"] }] })} />
      </ChatCwdContext.Provider>,
    );
    expect(document.querySelector(".chat-tool-verb")?.textContent).toBe("Update");
    // 会话目录下的路径显示成相对路径。
    expect(document.querySelector(".chat-tool-summary")?.textContent).toBe("src/a.ts");
    expect(document.querySelector(".chat-tool-out")?.textContent).toBe("新增 1 行，删除 1 行");
    expect(document.querySelector("details.chat-tool")?.hasAttribute("open")).toBe(true);
    const rows = [...document.querySelectorAll(".chat-tool-udiff .chat-tool-line")].map((row) =>
      `${row.querySelector(".chat-tool-ln")?.textContent}${row.querySelector(".chat-tool-mark")?.textContent}`);
    expect(rows).toEqual(["26 ", "27-", "27+", "28 "]);
    // 成功回执只是一句「已更新」，diff 已说明一切，不再重复。
    expect(document.querySelector(".chat-tool-body pre")).toBeNull();
  });

  it("Read 报读了几行，Search 报找到几条", () => {
    render(<>
      <ToolActivity item={{ type: "tool_use", id: "t5", timestamp: null, name: "Read", summary: "a.ts" }} result={result("t5", ["1\ta", "2\tb"].join("\n"))} />
      <ToolActivity item={{ type: "tool_use", id: "t6", timestamp: null, name: "Grep", summary: "foo" }} result={result("t6", ["x.ts", "y.ts", "z.ts"].join("\n"))} />
    </>);
    const outs = [...document.querySelectorAll(".chat-tool-out")].map((node) => node.textContent);
    expect(outs).toEqual(["读取了 2 行", "找到 3 条结果"]);
    expect(document.querySelectorAll(".chat-tool-verb")[1].textContent).toBe("Search");
  });

  it("relativize：大小写/斜杠不敏感地剥 cwd 前缀与 cd 前缀，目录外的路径原样", () => {
    expect(relativize("c:/Repo/src/a.ts", "C:\\repo")).toBe("src/a.ts");
    expect(relativize('cd "C:/repo" && bun test', "C:\\repo\\")).toBe("bun test");
    expect(relativize("D:/other/a.ts", "C:\\repo")).toBe("D:/other/a.ts");
    expect(relativize("C:/repository/a.ts", "C:/repo")).toBe("C:/repository/a.ts");
  });

  it("未识别的工具原样用工具 id 作动词", () => {
    expect(toolKind("mcp__x__do")).toBe("other");
    render(<ToolActivity item={{ type: "tool_use", id: "t2", timestamp: null, name: "mcp__x__do", summary: "" }} />);
    expect(document.querySelector(".chat-tool-verb")?.textContent).toBe("mcp__x__do");
  });
});

describe("命令与输出着色", () => {
  it("命令按命令名/参数/字符串/操作符/环境变量分色；heredoc 正文不按 shell 拆", () => {
    render(<CommandLines command={["A=1 cargo clippy -D warnings 2>&1 | grep -E 'x|y' && echo $HOME", "python - <<'X'", "print('hi')", "X"].join("\n")} />);
    const kinds = (kind: string) => [...document.querySelectorAll(`.chat-cmd-${kind}`)].map((node) => node.textContent);
    expect(kinds("env")).toEqual(["A=1"]);
    expect(kinds("name")).toEqual(["cargo", "grep", "echo", "python"]);
    expect(kinds("flag")).toEqual(["-D", "-E"]);
    expect(kinds("str")).toEqual(["'x|y'", "'X'"]);
    expect(kinds("op")).toEqual(["2>&1", "|", "&&", "<<"]);
    expect(kinds("var")).toEqual(["$HOME"]);
    expect(kinds("heredoc")).toEqual(["print('hi')", "X"]);
  });

  it("输出像 diff 时 +/- 行染色，error/warning 行染色；文本原样不改", () => {
    const text = ["Diff in a.rs:21:", "-    old", "+    new", "warning: unused", "error[E0063]: missing"].join("\n");
    render(<OutputBlock text={text} />);
    const tones = ["hunk", "del", "add", "warn", "err"].map((tone) => document.querySelector(`.chat-tool-output .is-${tone}`)?.textContent?.trim());
    expect(tones).toEqual(["Diff in a.rs:21:", "-    old", "+    new", "warning: unused", "error[E0063]: missing"]);
    expect(document.querySelector(".chat-tool-output")?.textContent).toBe(text);
  });

  it("不像 diff 的输出里 +/- 开头的行不染色", () => {
    render(<OutputBlock text={["- item", "+ more"].join("\n")} />);
    expect(document.querySelector(".chat-tool-output span")).toBeNull();
  });

  it("dedent 去掉公共前导空白", () => {
    expect(dedent(["      Tests  1", "        more", ""])).toEqual(["Tests  1", "  more", ""]);
  });
});

describe("回合头", () => {
  it("每轮回复前挂一次署名；工具组与正文同属一轮不重复挂", () => {
    const items: ChatItem[] = [
      user("u1"), reply("a1"),
      user("u2"),
      { type: "tool_use", id: "t1", timestamp: null, name: "Read", summary: "a.ts" },
      { type: "tool_result", id: "r1", timestamp: null, tool_use_id: "t1", text: "ok", is_error: false },
      reply("a2"),
    ];
    render(<Transcript sessionId={1} items={items} agent={agent} />);
    expect(document.querySelectorAll(".chat-turn-head")).toHaveLength(2);
    expect(screen.getAllByText("Claude Code")).toHaveLength(2);
    expect(screen.getAllByText("Opus 5.5")).toHaveLength(2);
  });

  it("不知道 agent 时不画回合头", () => {
    render(<Transcript sessionId={1} items={[user("u1"), reply("a1")]} />);
    expect(document.querySelector(".chat-turn-head")).toBeNull();
  });

  it("跨 provider 接续：前序段是别家的回复，只给当前段署名", () => {
    const items: ChatItem[] = [
      user("u0"), reply("a0"),
      { type: "meta", id: "handoff-current", timestamp: null, kind: "handoff:Claude Code" },
      user("u1"), reply("a1"),
    ];
    render(<Transcript sessionId={1} items={items} agent={agent} />);
    expect(document.querySelectorAll(".chat-turn-head")).toHaveLength(1);
  });

  it("过程组整组跑完才在摘要上报耗时", () => {
    const items: ChatItem[] = [
      user("u1"),
      { type: "tool_use", id: "t1", timestamp: "2026-10-19T10:00:00Z", name: "Bash", summary: "ls" },
      { type: "tool_result", id: "r1", timestamp: "2026-10-19T10:00:38Z", tool_use_id: "t1", text: "ok", is_error: false },
    ];
    render(<Transcript sessionId={1} items={items} agent={agent} />);
    expect(document.querySelector(".chat-activity-time")?.textContent).toBe("38s");
  });
});

describe("空会话欢迎面板", () => {
  const info: WelcomeInfo = { provider: "claude", name: "Claude Code", version: "2.1.303", model: "Opus 5.5", branch: "main", changes: 5 };

  it("事实条列 agent/model/branch；起手命令只取当前 agent 支持的；点了是预填不是发送", () => {
    const onPick = vi.fn();
    render(<WelcomePanel info={info} commands={["/model", "/compact"]} mention onPick={onPick} />);
    expect(screen.getByText("claude 2.1.303")).toBeTruthy();
    expect(screen.getByText("5 处改动")).toBeTruthy();
    expect(screen.queryByText("/resume")).toBeNull();
    fireEvent.click(screen.getByText("/model"));
    expect(onPick).toHaveBeenLastCalledWith("/model ");
    fireEvent.click(screen.getByText("审查未提交改动"));
    expect(onPick).toHaveBeenLastCalledWith("帮我审查当前未提交的改动");
    fireEvent.click(screen.getByText("引用文件"));
    expect(onPick).toHaveBeenLastCalledWith("@");
  });

  it("没有改动时不给审查入口，非仓库不列 branch", () => {
    render(<WelcomePanel info={{ ...info, branch: null, changes: 0 }} commands={[]} mention={false} onPick={() => {}} />);
    expect(screen.queryByText("审查未提交改动")).toBeNull();
    expect(screen.queryByText("branch")).toBeNull();
    expect(document.querySelector(".chat-welcome-chips")).toBeNull();
  });
});
