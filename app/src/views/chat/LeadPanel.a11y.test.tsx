// Lead 面板的键盘与读屏（调研 i11）：检查项能用键盘展开（button + aria-expanded），
// 通过 / 未通过 / 豁免图标与入口上的「待拍板」点读屏读得到（role="img" 才让 aria-label 生效）。
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { LeadTaskDto } from "../../api";
import { LeadPanel, LeadPill } from "./LeadPanel";

afterEach(cleanup);

function task(over: Partial<LeadTaskDto> = {}): LeadTaskDto {
  return {
    slug: "s", phase: "implement", spec: "", decisions: "", progress: "", gate: null,
    checks: [
      { id: "1", check: "测试全过", passes: true, waived: null, leader: false },
      { id: "2", check: "手机能用", passes: false, waived: null, leader: false },
      { id: "3", check: "旧机型", passes: false, waived: "不再支持", leader: false },
    ],
    ...over,
  } as LeadTaskDto;
}

describe("Lead 检查项", () => {
  it("每条是带 aria-expanded 的按钮，点按（键盘回车同此）展开后 aria-expanded=true", () => {
    render(<LeadPanel task={task()} onClose={() => {}} />);
    const row = screen.getByRole("button", { name: /手机能用/ });
    expect(row.tagName).toBe("BUTTON");
    expect(row.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(row);
    expect(row.getAttribute("aria-expanded")).toBe("true");
    expect(row.className).toContain("is-open");
  });

  it("状态图标有可读名：已通过、未通过、已豁免", () => {
    render(<LeadPanel task={task()} onClose={() => {}} />);
    for (const name of ["已通过", "未通过", "已豁免"]) expect(screen.getByRole("img", { name })).toBeTruthy();
    // 按钮的可访问名里带上状态，读屏一次就知道这条过没过。
    expect(screen.getByRole("button", { name: /已通过.*测试全过/ })).toBeTruthy();
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
