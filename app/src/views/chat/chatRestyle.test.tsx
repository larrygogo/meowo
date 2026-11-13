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
    expect(verb.textContent).toBe("运行");
    expect(verb.getAttribute("data-kind")).toBe("run");
    expect(document.querySelector(".chat-tool-summary")?.textContent).toBe("ls");
    expect(document.querySelector(".chat-tool")?.getAttribute("data-state")).toBe("running");
    expect(document.querySelector(".chat-tool-out.is-running")?.textContent).toMatch(/^运行中… \(\d+s\)$/);
    expect(screen.getByText("运行终端").className).toContain("chat-sronly");
    rerender(<ToolActivity item={item} result={result("t1", ["boom", "more"].join("\n"), { is_error: true })} />);
    expect(document.querySelector(".chat-tool")?.getAttribute("data-state")).toBe("error");
    expect(document.querySelector(".chat-tool-out.is-error")?.textContent).toBe("boom");
  });

  it("命令描述不占行（进悬停提示）；成功输出只给全文、不再重复露头 3 行（chat #10）；着色码剥掉", () => {
    expect(stripAnsi(`${ESC}[2m Tests ${ESC}[22m ${ESC}[1m${ESC}[32m316 passed${ESC}[39m`)).toBe(" Tests  316 passed");
    expect(stripAnsi(`${ESC}]8;;https://x\u0007link${ESC}]8;;\u0007`)).toBe("link");
    expect(stripAnsi("纯文本")).toBe("纯文本");
    const item = {
      type: "tool_use" as const, id: "t3", timestamp: null, name: "Bash", summary: "vitest",
      detail: { kind: "command" as const, command: "vitest", description: "Run tests" },
    };
    render(<ToolActivity item={item} result={result("t3", [`${ESC}[32ma${ESC}[39m`, "b", "c", "d", "e"].join("\n"))} />);
    expect(document.querySelector(".chat-tool-meta")).toBeNull();
    expect(document.querySelector(".chat-tool-summary")?.getAttribute("data-tip")).toBe("Run tests");
    expect(document.querySelector(".chat-tool-out")).toBeNull();
    expect(document.querySelector(".chat-tool-body pre")?.textContent).toBe(["a", "b", "c", "d", "e"].join("\n"));
  });

  it("Edit 默认一行收起，展开后按真实行号渲染统一 diff", () => {
    const item = {
      type: "tool_use" as const, id: "t4", timestamp: null, name: "Edit", summary: "C:/repo/src/a.ts",
      detail: { kind: "edit" as const, path: "C:/repo/src/a.ts", old: "old", new: "new", replace_all: false, truncated: false },
    };
    render(
      <ChatCwdContext.Provider value={"C:\\repo"}>
        <ToolActivity item={item} result={result("t4", "ok", { patch: [{ old_start: 26, new_start: 26, lines: [" keep", "-old", "+new", " tail"] }] })} />
      </ChatCwdContext.Provider>,
    );
    expect(document.querySelector(".chat-tool-verb")?.textContent).toBe("编辑");
    // 会话目录下的路径显示成相对路径。
    expect(document.querySelector(".chat-tool-summary")?.textContent).toBe("src/ a.ts");
    expect(document.querySelector(".chat-edit-card-head")?.textContent).toContain("+1");
    expect(document.querySelector(".chat-edit-card-head")?.textContent).toContain("-1");
    expect(document.querySelector(".chat-tool-result")).toBeNull();
    expect(document.querySelector("details.chat-tool")?.hasAttribute("open")).toBe(false);
    fireEvent.click(document.querySelector(".chat-tool > summary")!);
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
    expect(document.querySelectorAll(".chat-tool-verb")[1].textContent).toBe("搜索");
  });

  it("relativize：大小写/斜杠不敏感地剥 cwd 前缀与 cd 前缀，目录外的路径原样", () => {
    expect(relativize("c:/Repo/src/a.ts", "C:\\repo")).toBe("src/a.ts");
    expect(relativize('cd "C:/repo" && bun test', "C:\\repo\\")).toBe("bun test");
    expect(relativize("D:/other/a.ts", "C:\\repo")).toBe("D:/other/a.ts");
    expect(relativize("C:/repository/a.ts", "C:/repo")).toBe("C:/repository/a.ts");
  });

  it("未识别的工具原样用工具 id 作动词", () => {
    expect(toolKind("SomeNewTool")).toBe("other");
    render(<ToolActivity item={{ type: "tool_use", id: "t2", timestamp: null, name: "SomeNewTool", summary: "" }} />);
    expect(document.querySelector(".chat-tool-verb")?.textContent).toBe("SomeNewTool");
  });

  it("MCP 工具的动词去掉 mcp__ 前缀，显示成「服务 · 工具」", () => {
    expect(toolKind("mcp__x__do")).toBe("other");
    render(<ToolActivity item={{ type: "tool_use", id: "t3", timestamp: null, name: "mcp__x__do", summary: "" }} />);
    expect(document.querySelector(".chat-tool-verb")?.textContent).toBe("x · do");
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

describe("回合结构", () => {
  it("不显示助手署名头；工具组与正文同属一轮，页脚不重复", () => {
    const items: ChatItem[] = [
      user("u1"), reply("a1"),
      user("u2"),
      { type: "tool_use", id: "t1", timestamp: null, name: "Read", summary: "a.ts" },
      { type: "tool_result", id: "r1", timestamp: null, tool_use_id: "t1", text: "ok", is_error: false },
      reply("a2"),
    ];
    render(<Transcript sessionId={1} items={items} agent={agent} />);
    expect(document.querySelector(".chat-turn-head")).toBeNull();
    expect(document.querySelectorAll(".chat-turn-foot")).toHaveLength(2);
  });

  it("工具调用把回复切成多节时，页脚只在整段末尾出现一次并复制全部正文", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    const items: ChatItem[] = [
      user("u1"),
      { type: "assistant_text", id: "a1", timestamp: null, text: "第一节" },
      { type: "tool_use", id: "t1", timestamp: null, name: "Read", summary: "a.ts" },
      { type: "tool_result", id: "r1", timestamp: null, tool_use_id: "t1", text: "ok", is_error: false },
      { type: "assistant_text", id: "a2", timestamp: null, text: "第二节" },
      user("u2"),
    ];
    render(<Transcript sessionId={1} items={items} agent={agent} />);
    expect(document.querySelectorAll(".chat-turn-foot")).toHaveLength(1);
    fireEvent.click(document.querySelector(".chat-turn-foot .chat-turn-copy")!);
    expect(await screen.findByRole("button", { name: "已复制消息" })).toBeTruthy();
    expect(writeText).toHaveBeenCalledWith("第一节\n\n第二节");
  });

  it("回合末尾只给一组时间与复制，复制这一整段助手正文", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    const timestamp = "2026-10-19T10:00:38Z";
    const items: ChatItem[] = [
      user("u1"),
      { type: "assistant_text", id: "a1", timestamp, text: "完整回复" },
    ];
    render(<Transcript sessionId={1} items={items} agent={agent} />);
    expect(document.querySelectorAll(".chat-turn-foot")).toHaveLength(1);
    expect(document.querySelector(".chat-turn-head")).toBeNull();
    expect(document.querySelector(".chat-turn-foot time")?.textContent).toBe(
      new Date(timestamp).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false }),
    );
    expect(document.querySelector(".chat-message.is-assistant .chat-message-copy")).toBeNull();
    fireEvent.click(document.querySelector(".chat-turn-foot .chat-turn-copy")!);
    expect(await screen.findByRole("button", { name: "已复制消息" })).toBeTruthy();
    expect(writeText).toHaveBeenCalledWith("完整回复");
  });

  it("用户消息的时间与复制在气泡外的右下页脚", () => {
    const timestamp = "2026-10-19T09:31:00Z";
    render(<Transcript sessionId={1} items={[{ type: "user_text", id: "u1", timestamp, text: "提问" }]} />);
    expect(document.querySelector(".chat-user-foot time")?.textContent).toBe(
      new Date(timestamp).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false }),
    );
    expect(document.querySelector(".chat-user-foot .chat-turn-copy")).toBeTruthy();
    expect(document.querySelector(".chat-message.is-user > .chat-message-copy")).toBeNull();
  });

  it("单条工具原样平铺，≥2 条才收成汇总块；跑完的汇总块收起并写完成态摘要", () => {
    const items: ChatItem[] = [
      user("u0"),
      { type: "tool_use", id: "t0", timestamp: null, name: "Bash", summary: "ls" },
      { type: "tool_result", id: "r0", timestamp: null, tool_use_id: "t0", text: "ok", is_error: false },
      reply("a0"),
      user("u1"),
      { type: "tool_use", id: "t1", timestamp: null, name: "Bash", summary: "pwd" },
      { type: "tool_result", id: "r1", timestamp: null, tool_use_id: "t1", text: "ok", is_error: false },
      { type: "tool_use", id: "t2", timestamp: null, name: "Read", summary: "a.ts" },
      { type: "tool_result", id: "r2", timestamp: null, tool_use_id: "t2", text: "boom", is_error: true },
      reply("a1"),
    ];
    render(<Transcript sessionId={1} items={items} agent={agent} />);
    const folds = document.querySelectorAll(".turn-fold");
    expect(folds).toHaveLength(1);
    expect(folds[0].classList.contains("is-open")).toBe(false);
    expect(folds[0].textContent).toContain("已运行 1 条命令 · 读取了 1 个文件（1 失败）");
  });

  it("运行中的汇总块展开并写「正在做什么」，跑完（正文到达）自动收起", () => {
    // 折叠记忆按块 id 存在模块里，id 不能和上一条用例撞。
    const running: ChatItem[] = [
      user("u1"),
      { type: "tool_use", id: "run-t1", timestamp: null, name: "Bash", summary: "pwd" },
      { type: "tool_result", id: "r1", timestamp: null, tool_use_id: "run-t1", text: "ok", is_error: false },
      { type: "tool_use", id: "run-t2", timestamp: null, name: "Read", summary: "a.ts", detail: { kind: "read", path: "src/a.ts", offset: null, limit: null } },
    ];
    const { rerender } = render(<Transcript sessionId={2} items={running} agent={agent} working />);
    const fold = () => document.querySelector(".turn-fold")!;
    expect(fold().classList.contains("is-open")).toBe(true);
    expect(fold().querySelector(".turn-fold-lead")?.textContent).toBe("正在读取 src/a.ts");
    expect(fold().textContent).toContain("已运行 1 条命令");
    rerender(<Transcript sessionId={2} items={[
      ...running,
      { type: "tool_result", id: "r2", timestamp: null, tool_use_id: "run-t2", text: "ok", is_error: false },
      reply("a1"),
    ]} agent={agent} working />);
    expect(fold().classList.contains("is-open")).toBe(false);
    expect(fold().querySelector(".turn-fold-lead")).toBeNull();
  });

  it("连续工具渲染成扁平行，不包分组卡片也不逐行放复制钮", () => {
    const items: ChatItem[] = [
      user("u1"),
      { type: "tool_use", id: "t1", timestamp: "2026-10-19T10:00:00Z", name: "Bash", summary: "ls" },
      { type: "tool_result", id: "r1", timestamp: "2026-10-19T10:00:38Z", tool_use_id: "t1", text: "ok", is_error: false },
    ];
    render(<Transcript sessionId={1} items={items} agent={agent} />);
    expect(document.querySelector(".chat-activity-group")).toBeNull();
    expect(document.querySelectorAll(".chat-flat-steps > .chat-tool")).toHaveLength(1);
    expect(document.querySelector(".chat-tool-verb")?.textContent).toBe("运行");
    expect(document.querySelector(".chat-tool-copy")).toBeNull();
  });
});

/** 设置「消息自动折叠」：已结束的回复只露最终回答，过程（含中途的话）收成「工作了 …」一行。 */
describe("回合级折叠", () => {
  // 一段回复被中途的话切成两段：工具 → 「先看看」→ 工具 → 最终回答。
  const turn = (prefix: string): ChatItem[] => [
    { type: "user_text", id: `${prefix}-u`, timestamp: "2026-11-02T10:00:00Z", text: "改一下" },
    { type: "tool_use", id: `${prefix}-t1`, timestamp: "2026-11-02T10:00:05Z", name: "Bash", summary: "ls" },
    { type: "tool_result", id: `${prefix}-r1`, timestamp: "2026-11-02T10:00:06Z", tool_use_id: `${prefix}-t1`, text: "ok", is_error: false },
    { type: "assistant_text", id: `${prefix}-a1`, timestamp: "2026-11-02T10:00:10Z", text: "先看看" },
    { type: "tool_use", id: `${prefix}-t2`, timestamp: "2026-11-02T10:01:00Z", name: "Read", summary: "a.ts" },
    { type: "tool_result", id: `${prefix}-r2`, timestamp: "2026-11-02T10:01:01Z", tool_use_id: `${prefix}-t2`, text: "ok", is_error: false },
    { type: "assistant_text", id: `${prefix}-a2`, timestamp: "2026-11-02T10:02:13Z", text: "改好了" },
  ];
  const replyFold = () => document.querySelector(".turn-fold.is-reply");

  it("关着（默认）不折：过程与中途的话照常平铺", () => {
    render(<Transcript sessionId={31} items={turn("off")} agent={agent} />);
    expect(replyFold()).toBeNull();
    expect(screen.getByText("先看看")).toBeTruthy();
  });

  it("开着：中途的话和工具收进一行「工作了 2m13s」，默认收起，只露最终回答；点开能看到过程", () => {
    render(<Transcript sessionId={32} items={turn("on")} agent={agent} foldTurns />);
    expect(replyFold()?.querySelector(".turn-fold-summary")?.textContent).toBe("工作了 2m13s");
    expect(replyFold()?.classList.contains("is-open")).toBe(false);
    expect(screen.queryByText("先看看")).toBeNull();
    expect(screen.getByText("改好了")).toBeTruthy();
    expect(document.querySelectorAll(".turn-fold")).toHaveLength(1);
    fireEvent.click(replyFold()!.querySelector(".turn-fold-head")!);
    expect(screen.getByText("先看看")).toBeTruthy();
    // 里面原有的工具汇总行（ActivityRun 那一层）原样嵌在回合折叠里，同 Kimi。
    const inner = replyFold()!.querySelector(".turn-fold-body .turn-fold");
    expect(inner?.textContent).toContain("已运行 1 条命令 · 读取了 1 个文件");
  });

  it("运行中不折，回合结束（working 落下）才收起", () => {
    const items = turn("live");
    const { rerender } = render(<Transcript sessionId={33} items={items} agent={agent} working foldTurns />);
    expect(replyFold()).toBeNull();
    expect(screen.getByText("先看看")).toBeTruthy();
    rerender(<Transcript sessionId={33} items={items} agent={agent} working={false} foldTurns />);
    expect(replyFold()).not.toBeNull();
    expect(screen.queryByText("先看看")).toBeNull();
  });

  it("回合中插话不算回合结束：运行中插了一句，前面的进度照样平铺；回合结束后才收起", () => {
    // 2026-11-12 实拍：运行中插了一句话，前面 16 分钟的进度被收成「工作了 16m21s」，看着像消息没反馈出来。
    const items: ChatItem[] = [
      ...turn("mid"),
      { type: "user_text", id: "mid-i", timestamp: "2026-11-02T10:03:00Z", text: "怎么样", interjected: true },
      { type: "tool_use", id: "mid-t3", timestamp: "2026-11-02T10:03:05Z", name: "Bash", summary: "ls" },
    ];
    const { rerender } = render(<Transcript sessionId={35} items={items} agent={agent} working foldTurns />);
    expect(replyFold()).toBeNull();
    expect(screen.getByText("先看看")).toBeTruthy();
    expect(screen.getByText("怎么样")).toBeTruthy();
    const done = [...items, { type: "assistant_text" as const, id: "mid-a3", timestamp: "2026-11-02T10:04:00Z", text: "好了" }];
    rerender(<Transcript sessionId={35} items={done} agent={agent} working={false} foldTurns />);
    expect(replyFold()).not.toBeNull();
    expect(screen.queryByText("先看看")).toBeNull();
    expect(screen.getByText("怎么样")).toBeTruthy();
  });

  it("正常发的新消息才是回合边界：上一回合在新消息之前，运行中照样收起", () => {
    const items: ChatItem[] = [
      ...turn("prev"),
      { type: "user_text", id: "next-u", timestamp: "2026-11-02T10:05:00Z", text: "下一个" },
      { type: "tool_use", id: "next-t", timestamp: "2026-11-02T10:05:05Z", name: "Bash", summary: "ls" },
    ];
    render(<Transcript sessionId={36} items={items} agent={agent} working foldTurns />);
    expect(replyFold()).not.toBeNull();
    expect(screen.queryByText("先看看")).toBeNull();
  });

  it("只有一句回答、前面什么过程都没有的回复不套折叠", () => {
    render(<Transcript sessionId={34} items={[user("plain-u"), reply("plain-a", "直接答")]} agent={agent} foldTurns />);
    expect(replyFold()).toBeNull();
    expect(screen.getByText("直接答")).toBeTruthy();
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
