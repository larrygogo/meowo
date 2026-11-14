import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";
import type { ChatItem } from "../../api";
import { reduceChatEvents } from "../../chat/reducer";
import { ChatCwdContext } from "./ToolActivity";
import { Transcript } from "./Transcript";
import { resetFoldMemory } from "./foldMemory";

/// 五家回放测试（前端半程）。
///
/// 输入是后端回放测试产出的快照（`src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/<家>.items.json`，
/// 由各家**本机真实会话样本**经完整解析得到，见 `meowo-agent/src/replay_tests.rs`）。这里接着走
/// 归并（reduceChatEvents）与渲染（Transcript），把用户看到的时间线摘成一份文字大纲做快照，
/// 并对关键结果逐条断言——文案、是否标红、落到哪种块。
///
/// 接一个新 CLI 版本、改一处解析或渲染，五家的结果都会在这里被比对。
/// 样本缺哪类块（本机没有对应的真实数据）写在后端那份测试的期望表里。

const FIXTURES = join(__dirname, "../../../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay");
const CWD = "C:\\Users\\user\\Desktop\\workspace\\meowo";

function load(agent: string): ChatItem[] {
  const raw = JSON.parse(readFileSync(join(FIXTURES, `${agent}.items.json`), "utf8")) as ChatItem[];
  return reduceChatEvents([], raw, true);
}

const flat = (text: string | null | undefined, max = 72) => {
  const one = (text ?? "").replace(/\s+/g, " ").trim();
  return one.length > max ? `${one.slice(0, max)}…` : one;
};

/// 把渲染出来的时间线摘成一行一块的大纲：用户看得见的类型、动作词、对象、状态与数字。
function outline(root: HTMLElement): string[] {
  const selector = [
    ".chat-message.is-user", ".chat-message.is-assistant", ".chat-message.is-turn-error", "details.chat-reasoning",
    "details.chat-tool", "details.chat-subagent", ".chat-meta", ".turn-fold-head", ".chat-turn-changes",
  ].join(", ");
  return [...root.querySelectorAll<HTMLElement>(selector)].map((el) => {
    const text = (query: string) => flat(el.querySelector(query)?.textContent);
    if (el.matches(".chat-message.is-user")) return `用户｜${flat(el.querySelector(".chat-text")?.textContent ?? el.textContent)}`;
    if (el.matches(".chat-message.is-assistant")) return `助手｜${flat(el.textContent)}`;
    if (el.matches(".chat-message.is-turn-error")) return `出错卡｜${text(".chat-turn-error-label")}｜${text(".chat-text")}`;
    if (el.matches("details.chat-reasoning")) return `思考｜${flat(el.querySelector("summary")?.textContent)}`;
    if (el.matches("details.chat-subagent")) return `子任务｜${text(".chat-subagent-desc")}｜${text(".chat-subagent-status")}`;
    if (el.matches(".chat-meta")) return `分隔｜${flat(el.textContent)}`;
    if (el.matches(".turn-fold-head")) return `过程块｜${text(".turn-fold-summary")}`;
    if (el.matches(".chat-turn-changes")) {
      const rows = [...el.querySelectorAll(".chat-turn-changes-row")].map((row) => flat(row.textContent, 60));
      return `改动卡｜${text(".chat-turn-changes-head")}｜${rows.join("；")}`;
    }
    const parts = [
      `工具[${el.getAttribute("data-state")}]`,
      text(".chat-tool-verb"),
      text(".chat-tool-summary"),
      [text(".chat-tool-count"), text(".chat-tool-delta")].filter(Boolean).join(" "),
      text(".chat-tool-out"),
    ];
    return parts.join("｜");
  });
}

function replay(agent: string) {
  const view = render(
    <ChatCwdContext.Provider value={CWD}>
      <Transcript sessionId={900 + agent.length} items={load(agent)} />
    </ChatCwdContext.Provider>,
  );
  // 历史回合的过程块默认收起（只露一行摘要）：像用户那样逐个点开，才看得到里面的工具行。
  for (let guard = 0; guard < 20; guard += 1) {
    const closed = view.container.querySelector<HTMLElement>(".turn-fold:not(.is-open) > .turn-fold-head");
    if (!closed) break;
    fireEvent.click(closed);
  }
  const lines = outline(view.container);
  const tools = [...view.container.querySelectorAll<HTMLElement>("details.chat-tool")];
  /** 摘要里含某段文字的工具行。 */
  const toolsWith = (needle: string) => tools.filter((tool) => tool.querySelector(".chat-tool-summary")?.textContent?.includes(needle));
  return { ...view, lines, tools, toolsWith };
}

afterEach(() => {
  cleanup();
  resetFoldMemory();
});

describe("五家回放：真实会话样本 → 解析 → 归并 → 渲染", () => {
  it.each(["claude", "kimi", "codex", "gemini", "opencode"])("%s 的时间线大纲与快照一致", (agent) => {
    expect(replay(agent).lines.join("\n")).toMatchSnapshot();
  });

  it("claude：读取行数与搜索条数取真实数字，失败命令标红带退出码，子任务、出错卡、中断分隔俱全", () => {
    const { lines, tools, container } = replay("claude");
    // 回执正文被截到几百字，行数仍是 CLI 记的真实值。
    expect(lines.some((line) => line.startsWith("工具[done]｜读取｜") && line.includes("读取了 62 行"))).toBe(true);
    expect(lines.some((line) => line.startsWith("工具[done]｜搜索｜") && line.includes("找到 5 条结果"))).toBe(true);
    const failed = tools.filter((tool) => tool.getAttribute("data-state") === "error");
    expect(failed).toHaveLength(1);
    expect(failed[0].classList.contains("is-error")).toBe(true);
    expect(failed[0].querySelector(".chat-tool-out.is-error")?.textContent).toMatch(/^退出码 128/);
    // 同步委派：回执不带结局统计，子任务块在、标题是任务描述。
    expect(container.querySelector("details.chat-subagent .chat-subagent-desc")?.textContent).toBe("查 /model 不显示模型列表的原因");
    expect(container.querySelector(".chat-turn-error-label")?.textContent).toBe("需要重新登录");
    expect(lines).toContain("分隔｜已中断");
    expect(lines.some((line) => line.startsWith("改动卡｜2 个文件已修改"))).toBe(true);
  });

  it("codex：脚本解开成真实命令，非零退出标红，一次文件修改只显示一遍，补丁有 diff 与增删行数", () => {
    const { lines, tools, container } = replay("codex");
    // 工具行是命令本身，不是包着它的 JS。
    expect(tools.length).toBeGreaterThan(8);
    for (const tool of tools) {
      const summary = tool.querySelector(".chat-tool-summary")?.textContent ?? "";
      expect(summary).not.toMatch(/tools\.|await |text\(/);
    }
    expect(lines.some((line) => line.startsWith("工具[done]｜运行｜Get-Content app/src-tauri/CLAUDE.md"))).toBe(true);
    // 命令结果里 exit_code 非零 → 失败态、红色、带退出码。
    const failed = tools.filter((tool) => tool.getAttribute("data-state") === "error");
    expect(failed.some((tool) => /^退出码 1/.test(tool.querySelector(".chat-tool-out.is-error")?.textContent ?? ""))).toBe(true);
    // 补丁校验失败（Script failed）同样标红。
    expect(failed.some((tool) => tool.querySelector(".chat-tool-verb")?.textContent === "编辑")).toBe(true);
    // 样本里 FileChange 记录与脚本里的 apply_patch 说的是同一次修改：只出一行。
    const progress = ".lead/20261112-lead-providers/progress.md";
    const patches = tools.filter((tool) => tool.querySelector(".chat-tool-summary")?.textContent === progress);
    expect(patches).toHaveLength(1);
    const patch = patches[0];
    expect(patch.querySelector(".chat-tool-verb")?.textContent).toBe("编辑");
    expect(patch.querySelector(".chat-tool-delta")?.textContent).toMatch(/^\+\d+/);
    expect(patch.querySelectorAll(".chat-tool-udiff .chat-tool-line.is-add").length).toBeGreaterThan(0);
    // 只新建文件的补丁是「写入」。
    expect(lines.some((line) => line.startsWith("工具[done]｜写入｜"))).toBe(true);
    // 一段脚本里先打补丁再跑命令：两行，各有各的回执。
    expect(lines.some((line) => line.startsWith("工具[done]｜编辑｜app/src-tauri/tests/probe_lead_codex.rs"))).toBe(true);
    expect(lines.some((line) => line.startsWith("工具[done]｜运行｜rustfmt --edition 2021 --config max_width=120"))).toBe(true);
    // 协作类工具不是「子任务」：只有 spawn_agent 是。
    const agents = lines.filter((line) => line.includes("｜子任务｜"));
    expect(agents).toHaveLength(1);
    // 摘要是任务名，不是加密的 message。
    expect(agents[0].startsWith("工具[done]｜子任务｜gemini_e2e｜")).toBe(true);
    expect(lines.some((line) => line.includes("｜等待子任务｜"))).toBe(true);
    // 被用户打断的命令是「已中断」，不是失败；打断处有分隔。
    const aborted = tools.filter((tool) => tool.getAttribute("data-state") === "interrupted");
    expect(aborted).toHaveLength(1);
    expect(aborted[0].classList.contains("is-error")).toBe(false);
    expect(aborted[0].querySelector(".chat-tool-out")?.textContent).toBe("已中断");
    expect(lines).toContain("分隔｜已中断");
    expect(container.querySelector(".chat-turn-changes-head")?.textContent).toMatch(/^\d+ 个文件已修改$/);
  });

  it("gemini：读写改归类正确，摘要是路径，条数与行数是真实数字，有 diff 和改动卡，首行系统记录不上屏", () => {
    const { lines, tools, toolsWith, container } = replay("gemini");
    expect(lines).toContain("工具[done]｜读取｜app/src/views/ NewSessionPanel.tsx:1-200｜读取了 200 行｜读取了 200 行");
    expect(lines.some((line) => line.startsWith("工具[done]｜搜索｜ns-window｜找到 2 条结果"))).toBe(true);
    // 没有命中也是真实数字：glob 一个都没找到。
    expect(lines).toContain("工具[done]｜搜索｜app/src/views/*Settings*.tsx｜找到 0 条结果｜找到 0 条结果");
    const written = toolsWith("improve-new-session-design.md");
    expect(written.map((tool) => tool.querySelector(".chat-tool-verb")?.textContent)).toEqual(["写入"]);
    const edits = toolsWith("styles.css");
    expect(edits.map((tool) => tool.querySelector(".chat-tool-verb")?.textContent)).toEqual(["编辑", "编辑"]);
    expect(edits[0].getAttribute("data-state")).toBe("done");
    // 增删行数按回执的 diff 数（样本里这段 diff 截短过，所以是 3 增 3 删）。
    expect(edits[0].querySelector(".chat-tool-delta")?.textContent).toBe("+3-3");
    // 回执带的 diff 有真实行号。
    expect(edits[0].querySelector(".chat-tool-udiff .chat-tool-ln")?.textContent).toBe("2348");
    expect(edits[1].getAttribute("data-state")).toBe("error");
    // 摘要不是半截 JSON。
    for (const tool of tools) expect(tool.querySelector(".chat-tool-summary")?.textContent ?? "").not.toMatch(/^\{/);
    expect(container.textContent).not.toContain("系统记录");
    expect(container.textContent).not.toContain("session_context");
    expect(lines.some((line) => line.startsWith("改动卡｜2 个文件已修改"))).toBe(true);
    expect(lines).toContain("分隔｜已中断");
    expect(container.querySelector(".chat-turn-error-label")?.textContent).toBe("API 请求出错");
  });

  it("kimi：失败的命令标红（isError 在 result 里），出错卡与中断分隔俱全", () => {
    const { lines, tools, container } = replay("kimi");
    const failed = tools.filter((tool) => tool.getAttribute("data-state") === "error");
    expect(failed).toHaveLength(1);
    expect(failed[0].querySelector(".chat-tool-verb")?.textContent).toBe("运行");
    expect(container.querySelector("details.chat-subagent .chat-subagent-desc")?.textContent).toBe("独立审查 RuntimeRequest");
    expect(container.querySelector(".chat-turn-error-label")?.textContent).toBe("API 请求出错");
    expect(lines).toContain("分隔｜已中断");
    expect(lines.some((line) => line.startsWith("改动卡｜2 个文件已修改"))).toBe(true);
  });

  it("opencode：被用户打断的命令是已中断而不是失败，被拒的读取标红，出错卡与中断分隔俱全", () => {
    const { lines, tools, container } = replay("opencode");
    const aborted = tools.filter((tool) => tool.getAttribute("data-state") === "interrupted");
    expect(aborted).toHaveLength(1);
    expect(aborted[0].querySelector(".chat-tool-summary")?.textContent).toBe("ping -n 30 127.0.0.1");
    expect(aborted[0].querySelector(".chat-tool-out")?.textContent).toBe("已中断");
    expect(aborted[0].classList.contains("is-error")).toBe(false);
    const failed = tools.filter((tool) => tool.getAttribute("data-state") === "error");
    expect(failed.map((tool) => tool.querySelector(".chat-tool-verb")?.textContent)).toEqual(["读取"]);
    expect(container.querySelector(".chat-turn-error-label")?.textContent).toBe("回合出错");
    expect(lines).toContain("分隔｜已中断");
  });

  it("五家同类工具落到同一个展示组件：动作词与类别标记一致", () => {
    const kinds = new Map<string, Set<string>>();
    for (const agent of ["claude", "kimi", "codex", "gemini", "opencode"]) {
      const { container } = replay(agent);
      for (const verb of container.querySelectorAll<HTMLElement>(".chat-tool-verb")) {
        const kind = verb.getAttribute("data-kind") ?? "";
        if (!kinds.has(kind)) kinds.set(kind, new Set());
        kinds.get(kind)!.add(verb.textContent ?? "");
      }
      cleanup();
    }
    // 每个类别在五家那里只有一个动作词。
    for (const kind of ["run", "read", "write", "edit", "search", "web"]) {
      expect([...(kinds.get(kind) ?? [])], kind).toHaveLength(1);
    }
    expect([...kinds.get("run")!]).toEqual(["运行"]);
    expect([...kinds.get("edit")!]).toEqual(["编辑"]);
  });
});
