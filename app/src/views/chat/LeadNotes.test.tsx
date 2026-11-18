// Lead 面板「决策」「进度」页签（2026-11-19 面板打磨）：样例是 .lead 下真实任务目录的 decisions.md 与
// progress.md（progress 按后端 meowo_reporter::lead::tail 取最后 6000 字，与面板实际拿到的一致），
// 见 __fixtures__/lead/。修复前：决定按「标题：正文」正则拆，`## D1 …` 格式拆不出结构；进度把各行平铺成一条时间线，
// 没有先后、看不出哪条最新，截断的半截节照样显示。
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { visibleText } from "../../test/visibleText";
import { LeadDecisions, LeadProgress } from "./LeadNotes";
import { LEAD_TAIL_LIMIT, leadTailTruncated, parseLeadDecisions, parseLeadProgress, type LeadDecision } from "./leadNotesParse";

afterEach(cleanup);

const here = dirname(fileURLToPath(import.meta.url));
const sample = (name: string) => readFileSync(join(here, "__fixtures__/lead", name), "utf8");
const cards = (text: string) => parseLeadDecisions(text).entries.filter((e): e is Extract<LeadDecision, { kind: "card" }> => e.kind === "card");

describe("决定：解析真实 decisions.md", () => {
  it("`## D<n> 标题（日期，leader：「原话」）`：编号、标题、日期、原话分开，最新的在最前", () => {
    const list = cards(sample("multi-forge.decisions.md"));
    expect(list.map((c) => c.id)).toEqual(["D6", "D5", "D4", "D3", "D2", "D1"]);
    const d3 = list.find((c) => c.id === "D3")!;
    expect(d3).toMatchObject({ title: "取消本机 gh", date: "2026-11-18", quotes: ["取消gh"], meta: null });
    // 要点是 agent 归纳的，原样保留；原话不混进标题。
    expect(d3.body).toContain("- Meowo 不再调用本机 gh 命令行");
    expect(d3.title).not.toContain("「");
    // 括号写成「leader 2026-11-18：「…」」也认。
    expect(list.find((c) => c.id === "D1")).toMatchObject({ title: "范围与做法", date: "2026-11-18", quotes: ["按你的建议"] });
    // 没有原话的说明留作一行小字。
    expect(list[0].meta).toContain("关卡 C-20261118-multi-forge-1");
  });

  it("英文任务的原话写成 \"…\" / “…” 也认", () => {
    const list = cards("## D2 Drop the local gh (2026-11-18, leader: \"drop gh\")\n\n- No more gh.\n\n## D3 Hosts (leader: “one list”)\n");
    expect(list.map((c) => [c.id, c.title, c.date, c.quotes, c.meta])).toEqual([
      ["D3", "Hosts", null, ["one list"], null],
      ["D2", "Drop the local gh", "2026-11-18", ["drop gh"], null],
    ]);
  });

  it("没有日期、agent 自己定的、正文是段落而非列表的决定也成卡", () => {
    const list = cards(sample("github-connection.decisions.md"));
    expect(list.find((c) => c.id === "D2")).toMatchObject({ title: "在 GhExec 这一层替换", date: null, quotes: [], meta: "agent 决定" });
    expect(list.find((c) => c.id === "D3")).toMatchObject({ title: "默认「自动」", date: null });
    expect(list.find((c) => c.id === "D3")!.body).toContain("老用户已装好 gh 的不受影响");
    // 「leader：「原话」，leader 自己……」：开头的身份词由原话块交代，说明里不再重复。
    const d4 = list.find((c) => c.id === "D4")!;
    expect(d4.quotes).toEqual(["你来帮我注册"]);
    expect(d4.meta).toBe("leader 自己在面板浏览器登录 GitHub 后由 agent 填表提交");
  });

  it("旧格式：`- D0：正文`、`- D4（leader，关卡 …）：正文`、`- 标题：正文` 单行条目", () => {
    const list = cards(sample("kimi-parity.decisions.md"));
    expect(list.map((c) => c.id)).toEqual(["D7", "D6", "D5", "D4", "D3", "D2", "D1", "D0", "D0"]);
    expect(list[0]).toMatchObject({ meta: "leader，关卡 C-20261116-browser-agent-final-2" });
    expect(list[0].body).toMatch(/^accept；A5 真实链路补齐后接受本轮交付/);
    const trust = cards(sample("lead-trust.decisions.md"));
    expect(trust.find((c) => c.title === "用户追加")?.body).toMatch(/^进一步优化 Lead 面板/);
    expect(trust.find((c) => c.title === "实施方案")).toMatchObject({ date: "2026-11-16" });
    // 「2026-11-16，leader：正文」：冒号前只有日期和身份，不当标题。
    expect(trust.at(-1)).toMatchObject({ title: "", date: "2026-11-16", meta: "leader" });
  });

  it("旧格式的决定表格每行一张卡：标题取决策列，日期、谁定的归位，其余列成要点", () => {
    const list = cards(sample("lead-panel-ux.decisions.md"));
    expect(list).toHaveLength(5);
    expect(list[0].title).toMatch(/^关卡 C-20261112-lead-panel-ux/);
    expect(list.at(-1)).toMatchObject({ title: "不挂关卡 A，直接开工", date: "2026-11-12", meta: "agent" });
    expect(list.at(-1)!.body).toContain("- **理由**：没有单向门");
    expect(list.find((c) => c.title.startsWith("固定选项关卡"))!.body).toContain("- **推翻条件**：有人误点「通过」");
    // 「—」的空格子不出要点。
    expect(list[0].body).not.toContain("理由");
    // 理由里写的「leader 原话「…」」取出来做原话块，原文里不重复。
    const table = "| 日期 | 决策 | 理由 | 谁定的 |\n|---|---|---|---|\n| 2026-11-13 | 保留 TUI | leader 原话「我觉得还是得保留TUI」；推翻托管会话 | leader |";
    expect(cards(table)[0]).toMatchObject({ title: "保留 TUI", quotes: ["我觉得还是得保留TUI"], meta: "leader", body: "- **理由**：推翻托管会话" });
  });

  it("认不出的段落、没有决策列的表格原样保留，不丢内容", () => {
    const text = "# 决定\n\n先说明一下背景。\n\n- D1：做 A\n\n| 名称 | 值 |\n|---|---|\n| x | 1 |\n\n尾注：以 progress.md 为准";
    const entries = parseLeadDecisions(text).entries;
    expect(entries.map((e) => e.kind)).toEqual(["raw", "card", "raw"]);
    expect(entries[0]).toMatchObject({ body: "| 名称 | 值 |\n|---|---|\n| x | 1 |\n\n尾注：以 progress.md 为准" });
    expect(entries[2]).toMatchObject({ body: "先说明一下背景。" });
    // 总标题「# 决定」与页签名重复，不进卡片。
    expect(entries.some((e) => e.body.includes("# 决定"))).toBe(false);
  });

  it("渲染：原话在引文块里、带「leader 原话」标签，与要点分开；Markdown 交给 ChatMarkdown", () => {
    render(<LeadDecisions text={sample("multi-forge.decisions.md")} />);
    const d3 = screen.getByRole("heading", { name: "取消本机 gh" }).closest(".lead-decision")!;
    const quote = d3.querySelector("blockquote.lead-decision-quote")!;
    expect(quote.textContent).toBe("leader 原话「取消gh」");
    expect(d3.querySelector(".lead-decision-date")?.textContent).toBe("2026-11-18");
    // 要点是列表（ChatMarkdown 渲染），不在引文块里。
    const points = d3.querySelector(".lead-note-body ul")!;
    expect(points.querySelectorAll("li").length).toBe(4);
    expect(quote.contains(points)).toBe(false);
    // 最新的 D6 在最上面。
    expect(document.querySelector(".lead-decision .lead-decision-id")?.textContent).toBe("D6");
  });
});

describe("进度：解析真实 progress.md", () => {
  it("按节倒序：最新一节在最前，节标题拆成日期与小标题，节内条目保持原顺序", () => {
    const log = parseLeadProgress(sample("multi-forge.progress.tail.md"));
    expect(log.sections.map((s) => s.title)).toEqual([
      "验证（lead）", "复核修正（取消 gh 之后）", "取消 gh（D3）、gitlab.com 只用令牌（D4）、设置「代码托管」分区（D5）",
      "复核第二轮修复（feat/multi-forge，基于 f9585035，已 push 到 bfaab13f）",
    ]);
    expect(log.sections.every((s) => s.date === "2026-11-18")).toBe(true);
    const latest = log.sections[0];
    expect(latest.items).toBe(6);
    expect(latest.body.indexOf("独立复核三轮")).toBeLessThan(latest.body.indexOf("leader 接受 C-20261118-multi-forge-1"));
  });

  it("尾部截断在节中间：半截的节整节不显示，末尾说明更早的记录在 progress.md", () => {
    const text = sample("multi-forge.progress.tail.md");
    expect([...text].length).toBe(LEAD_TAIL_LIMIT);
    const log = parseLeadProgress(text);
    expect(log.truncated).toBe(true);
    // 被截的那节（独立审查问题修复 + Gitee）连同它的 ### 子标题都不出现。
    expect(log.sections.some((s) => s.body.includes("### Gitee（gitee.com）"))).toBe(false);
    expect(log.sections.at(-1)?.body.startsWith("- A 8abcee2c")).toBe(true);
    // 截在表格中间（ui-polish）：半张表不显示。
    const ui = parseLeadProgress(sample("ui-polish.progress.tail.md"));
    expect(ui.truncated).toBe(true);
    expect(ui.sections.map((s) => s.title)).toEqual(["复核返工（agent）", "验证（主会话）"]);
    expect(ui.sections.some((s) => s.body.includes("| 手机 |"))).toBe(false);
    render(<LeadProgress text={text} />);
    expect(screen.getByText("更早的记录在 progress.md 里")).toBeTruthy();
    expect(screen.queryByText(/独立审查问题修复/)).toBeNull();
  });

  it("截断判断：不满 6000 字、或开头是 phase 行 / 总标题的都不算截断", () => {
    expect(leadTailTruncated("phase: done\n- a")).toBe(false);
    const full = "phase: done\n" + "- 记录\n".repeat(1500);
    expect([...full].length).toBeGreaterThanOrEqual(LEAD_TAIL_LIMIT);
    expect(leadTailTruncated(full)).toBe(false);
    expect(leadTailTruncated("录\n" + "- 记录\n".repeat(1500))).toBe(true);
    // 整份都在一节里、截断后找不到节开头：不显示半截内容，只留「更早的记录」说明。
    const log = parseLeadProgress("录\n" + "x".repeat(LEAD_TAIL_LIMIT));
    expect(log).toEqual({ sections: [], truncated: true });
  });

  it("旧格式（没有节标题、条目带日期）：按日期分组，不写日期的条目跟着上一组", () => {
    const rows = parseLeadProgress(sample("chat-record-rows.progress.md"));
    expect(rows.truncated).toBe(false);
    expect(rows.sections).toHaveLength(1);
    expect(rows.sections[0]).toMatchObject({ date: "2026-11-09", title: null, items: 11 });
    // 条目里的日期已由节头写明，正文里去掉；时间（21:36）保留。
    expect(rows.sections[0].body).toMatch(/^- 定位根因：/);
    expect(rows.sections[0].body).toContain("- 21:36 leader 追加");
    const mixed = parseLeadProgress("phase: verify\n- 2026-11-10 第一天\n- 补充\n- 2026-11-11 第二天\n  - 子条目");
    expect(mixed.sections.map((s) => [s.date, s.items])).toEqual([["2026-11-11", 1], ["2026-11-10", 2]]);
    expect(mixed.sections[0].body).toBe("- 第二天\n  - 子条目");
  });

  it("没有日期的进度、只有 phase 行、空文件：各有确定的显示", () => {
    const smoke = parseLeadProgress(sample("workflow-smoke.progress.md"));
    expect(smoke.sections).toHaveLength(1);
    expect(smoke.sections[0]).toMatchObject({ date: null, title: null, items: 12 });
    // 总标题「# 进度」和 phase 行不进时间线。
    expect(smoke.sections[0].body).not.toMatch(/# 进度|phase:/);
    render(<LeadProgress text={sample("workflow-smoke.progress.md")} />);
    expect(document.querySelector(".lead-entry-date")?.textContent).toBe("未注明日期");
    cleanup();
    render(<LeadProgress text="phase: implement" />);
    expect(screen.getByText("暂无内容")).toBeTruthy();
    expect(document.body.textContent).not.toContain("phase");
    cleanup();
    render(<LeadProgress text="" />);
    expect(screen.getByText("暂无内容")).toBeTruthy();
    // 只有节标题、没有正文的节：显示日期与标题，但不是按钮。
    cleanup();
    render(<LeadProgress text={"phase: verify\n## 2026-11-19 建任务"} />);
    expect(screen.getByText("建任务")).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("渲染：最新一节标「最新」并默认展开；旧节收起只显示条数，点开才看正文", () => {
    render(<LeadProgress text={sample("multi-forge.progress.tail.md")} />);
    const heads = screen.getAllByRole("button");
    expect(heads).toHaveLength(4);
    expect(heads[0].getAttribute("aria-expanded")).toBe("true");
    expect(heads[0].textContent).toContain("最新");
    expect(heads[0].textContent).toContain("验证（lead）");
    expect(heads[0].textContent).not.toContain("条");
    expect(screen.getByText(visibleText(/^独立复核三轮/))).toBeTruthy();
    expect(heads[1].getAttribute("aria-expanded")).toBe("false");
    expect(heads[1].textContent).toContain("4 条");
    expect(heads[1].textContent).not.toContain("最新");
    expect(screen.queryByText(visibleText(/^提交 504db0f3/))).toBeNull();
    fireEvent.click(heads[1]);
    expect(heads[1].getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText(visibleText(/^提交 504db0f3/))).toBeTruthy();
    // 最新一节也能收起。
    fireEvent.click(heads[0]);
    expect(heads[0].getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByText(visibleText(/^独立复核三轮/))).toBeNull();
  });

  it("子标题、表格、代码块、链接经 ChatMarkdown 正常渲染", () => {
    const text = "phase: verify\n\n## 2026-11-19 验证\n\n### 截图\n\n| 场景 | 结果 |\n|---|---|\n| 手机 | 通过 |\n\n```bash\nbunx vitest run src\n```\n\n见 [PR](https://example.com/pr/1)";
    render(<LeadProgress text={text} />);
    expect(screen.getByRole("heading", { name: "截图" })).toBeTruthy();
    expect(screen.getByRole("table")).toBeTruthy();
    expect(screen.getByRole("columnheader", { name: "场景" })).toBeTruthy();
    expect(document.querySelector(".lead-note-body pre code")?.textContent).toContain("bunx vitest run src");
    expect(screen.getByRole("link", { name: "PR" })).toBeTruthy();
    // 代码块里的 `##`、`- ` 不是节或条目。
    const fenced = parseLeadProgress("## 2026-11-19 a\n```\n## 不是节\n- 不是条目\n```");
    expect(fenced.sections).toHaveLength(1);
    expect(fenced.sections[0].items).toBe(0);
  });
});
