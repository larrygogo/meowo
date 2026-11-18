// Lead 面板的键盘与读屏（调研 i11）：有内容的检查项能用键盘展开（button + aria-expanded），没内容的不是按钮（2026-11-19），
// 通过 / 未通过 / 豁免图标与入口上的「待拍板」点读屏读得到（role="img" 才让 aria-label 生效）。
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { LeadTaskDto } from "../../api";
import { LeadPanel, LeadPill } from "./LeadPanel";

afterEach(cleanup);

function task(over: Partial<LeadTaskDto> = {}): LeadTaskDto {
  return {
    slug: "s", phase: "implement", spec: "", decisions: "", progress: "", gate: null,
    checks: [
      { id: "1", check: "测试全过", passes: true, waived: null, leader: false, blocked: null },
      { id: "2", check: "手机能用", passes: false, waived: null, leader: false, blocked: null },
      { id: "3", check: "旧机型", passes: false, waived: "不再支持", leader: false, blocked: null },
    ],
    ...over,
  } as LeadTaskDto;
}

describe("Lead 检查项", () => {
  it("有证据、待实测或豁免说明的条目才是带 aria-expanded 的按钮，点按（键盘回车同此）展开后 aria-expanded=true", () => {
    const evidence = { command: "bunx vitest run", result: "12 passed", revision: "r1", reference: "progress.md", limitations: "无" };
    render(<LeadPanel task={task({ checks: [
      { id: "1", check: "测试全过", passes: false, waived: null, leader: false, blocked: null, evidence: [evidence] },
      { id: "2", check: "手机能用", passes: false, waived: null, leader: false, blocked: null },
      { id: "4", check: "真机实测", passes: false, waived: null, leader: false, blocked: "要真机" },
    ] })} onClose={() => {}} />);
    const row = screen.getByRole("button", { name: /测试全过/ });
    expect(row.getAttribute("aria-expanded")).toBe("false");
    expect(row.querySelector(".lead-check-caret")).toBeTruthy();
    fireEvent.click(row);
    expect(row.getAttribute("aria-expanded")).toBe("true");
    expect(row.className).toContain("is-open");
    expect(screen.getByText("bunx vitest run")).toBeTruthy();
    // 待实测说明也算可展开的内容（收起时说明只露一行）。
    expect(screen.getByRole("button", { name: /真机实测/ }).getAttribute("aria-expanded")).toBe("false");
  });

  it("没有可展开内容的条目不是按钮：不可聚焦、没有展开态和箭头、悬停不变色（修复前每行都是按钮，点开是空的）", () => {
    render(<LeadPanel task={task()} onClose={() => {}} />);
    expect(screen.queryByRole("button", { name: /手机能用/ })).toBeNull();
    const row = screen.getByText("手机能用").closest(".lead-check") as HTMLElement;
    expect(row.tagName).toBe("DIV");
    expect(row.className).toContain("is-static");
    expect(row.hasAttribute("tabindex")).toBe(false);
    expect(row.hasAttribute("aria-expanded")).toBe(false);
    expect(row.querySelector(".lead-check-caret")).toBeNull();
    fireEvent.click(row);
    expect(row.className).not.toContain("is-open");
    // 悬停底色、手型只挂在 button.lead-check 上：静态行在样式上就不像可点。
    const css = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../../styles.css"), "utf8");
    expect(css).toMatch(/^button\.lead-check:hover \{/m);
    expect(css).not.toMatch(/^\.lead-check:hover \{/m);
    expect(css).toMatch(/^\.lead-check \{[^}]*\}/m);
    expect(css.match(/^\.lead-check \{([^}]*)\}/m)?.[1]).not.toContain("cursor: pointer");
    expect(css.match(/^\.lead-check\.is-static \{([^}]*)\}/m)?.[1]).toContain("cursor: default");
  });

  it("状态图标有可读名：已通过、未通过、已豁免", () => {
    render(<LeadPanel task={task()} onClose={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "已通过与豁免 · 2" }));
    for (const name of ["已通过", "未通过", "已豁免"]) expect(screen.getByRole("img", { name })).toBeTruthy();
    // 可展开的行：按钮的可访问名里带上状态，读屏一次就知道这条过没过。
    expect(screen.getByRole("button", { name: /已豁免.*旧机型/ })).toBeTruthy();
    // 静态行：状态图标就在同一行里。
    expect(screen.getByText("测试全过").closest(".lead-check")?.querySelector('[role="img"]')?.getAttribute("aria-label")).toBe("已通过");
  });

  it("aria-label 只挂在有 role 的元素上（无 role 的 span 上 aria-label 不生效）", () => {
    const { container } = render(<><LeadPanel task={task()} onClose={() => {}} /><LeadPill task={task({ gate: { id: "g", kind: "A", title: "", summary: "", options: [], status: "pending", answer: null } as never })} active={false} onClick={() => {}} /></>);
    const bad = Array.from(container.querySelectorAll("span[aria-label], div[aria-label], li[aria-label]")).filter((el) => !el.getAttribute("role"));
    expect(bad.map((el) => el.className)).toEqual([]);
  });
});

describe("Lead 入口", () => {
  it("关卡等待时入口按钮的名字里有「待拍板」", () => {
    render(<LeadPill task={task({ gate: { id: "g", kind: "A", title: "", summary: "", options: [], status: "pending", answer: null } as never })} active={false} onClick={() => {}} />);
    expect(screen.getByRole("button", { name: /待拍板/ })).toBeTruthy();
  });
});
