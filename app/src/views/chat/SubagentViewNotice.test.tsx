import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SubagentViewNotice } from "./SubagentViewNotice";
import { zh } from "../../i18n/zh";
import { en } from "../../i18n/en";

afterEach(cleanup);

describe("SubagentViewNotice", () => {
  it("说清楚终端停在哪个子任务，点按钮切回", () => {
    const onBack = vi.fn();
    render(<SubagentViewNotice name="后端实现 Excel 附件解析" busy={false} onBack={onBack} />);
    expect(screen.getByRole("status").textContent).toBe("终端停在子任务「后端实现 Excel 附件解析」，发送会交给它");
    fireEvent.click(screen.getByRole("button", { name: "切回主会话" }));
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it("发送在途时按钮不可点，名字缺失有兜底", () => {
    const onBack = vi.fn();
    render(<SubagentViewNotice name={null} busy onBack={onBack} />);
    expect(screen.getByRole("status").textContent).toContain("「未命名」");
    // aria-disabled + 拦截（做空第一轮 #4）：真 disabled 会把焦点踢到 body。
    const back = screen.getByRole("button", { name: "切回主会话" }) as HTMLButtonElement;
    expect(back.getAttribute("aria-disabled")).toBe("true");
    fireEvent.click(back);
    expect(onBack).not.toHaveBeenCalled();
  });

  it("文案守规矩：不超 20 字、不用破折号和分号，中英文都有", () => {
    const template = zh.chat.subagentViewNotice("");
    expect([...template].length).toBeLessThanOrEqual(20);
    for (const text of [template, zh.chat.subagentViewBack, en.chat.subagentViewNotice(""), en.chat.subagentViewBack]) {
      expect(text).not.toMatch(/[—–;；]/);
    }
  });
});
