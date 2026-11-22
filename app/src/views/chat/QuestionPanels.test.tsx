import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { QuestionPanels } from "./QuestionPanels";
import type { StructuredQuestion } from "./askUserQuestion";

afterEach(cleanup);

const question = (multiSelect: boolean): StructuredQuestion => ({
  question: "选哪个？", header: null, multiSelect,
  options: [{ label: "甲", description: null }, { label: "乙", description: null }],
});

describe("QuestionPanels 作答面", () => {
  // 单选画圆点、多选画方框打勾：此前一律方框打勾，单选题看起来像能多选。
  it("单选题标 is-single 且选中项不打勾，多选题选中项打勾", () => {
    const answers = new Map([[0, { selected: ["甲"], custom: "" }]]);
    const noop = () => {};
    const single = render(<QuestionPanels mode="answer" items={[question(false)]} answers={answers} onSelect={noop} onCustom={noop} />);
    const singleList = single.container.querySelector(".chat-approval-options")!;
    expect(singleList.classList.contains("is-single")).toBe(true);
    expect(singleList.querySelector(".is-selected i")!.textContent).toBe("");
    cleanup();
    const multi = render(<QuestionPanels mode="answer" items={[question(true)]} answers={answers} onSelect={noop} onCustom={noop} />);
    const multiList = multi.container.querySelector(".chat-approval-options")!;
    expect(multiList.classList.contains("is-single")).toBe(false);
    expect(multiList.querySelector(".is-selected i")!.textContent).toBe("✓");
  });

  // 自定义答案：回车提交，但输入法选词的那一下回车不算（走公共守卫 imeComposing）。
  it("自定义答案输入框：输入法合成中的回车不提交，WebKit 只剩 keyCode 229 的那一下也不提交", () => {
    const onSubmit = vi.fn();
    const noop = () => {};
    const view = render(<QuestionPanels mode="answer" items={[question(false)]} answers={new Map([[0, { selected: [], custom: "zhong" }]])} onSelect={noop} onCustom={noop} onSubmit={onSubmit} />);
    const input = view.container.querySelector<HTMLInputElement>(".chat-approval-custom input")!;
    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    fireEvent.keyDown(input, { key: "Enter", keyCode: 229 });
    expect(onSubmit).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter", keyCode: 13 });
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });
});

describe("QuestionPanels 选项预览", () => {
  const withPreview: StructuredQuestion = {
    question: "设置页用哪种布局？", header: null, multiSelect: false,
    options: [
      { label: "卡片式", description: null, preview: "┌────┐\n│ 卡 │\n└────┘" },
      { label: "列表式", description: null, preview: "- 深色模式  [x]\n- 声音      [ ]" },
      { label: "随你", description: null },
    ],
  };
  const noop = () => {};

  it("默认显示第一个带预览的选项，ASCII 草图按原样保留换行", () => {
    const view = render(<QuestionPanels mode="answer" items={[withPreview]} answers={new Map()} onSelect={noop} onCustom={noop} />);
    const pre = view.container.querySelector(".chat-question-preview pre")!;
    expect(pre.textContent).toBe("┌────┐\n│ 卡 │\n└────┘");
  });

  it("悬停或聚焦到别的选项就换成它的预览；指到没预览的选项时保持上一张", () => {
    const view = render(<QuestionPanels mode="answer" items={[withPreview]} answers={new Map()} onSelect={noop} onCustom={noop} />);
    const buttons = view.container.querySelectorAll<HTMLButtonElement>(".chat-approval-options button");
    fireEvent.mouseEnter(buttons[1]);
    expect(view.container.querySelector(".chat-question-preview pre")!.textContent).toContain("深色模式");
    fireEvent.focus(buttons[2]);
    expect(view.container.querySelector(".chat-question-preview pre")!.textContent).toContain("┌");
  });

  it("已选的选项优先显示它的预览", () => {
    const view = render(<QuestionPanels mode="answer" items={[withPreview]} answers={new Map([[0, { selected: ["列表式"], custom: "" }]])} onSelect={noop} onCustom={noop} />);
    expect(view.container.querySelector(".chat-question-preview pre")!.textContent).toContain("深色模式");
  });

  it("只读题面也显示预览；全都没有预览时不渲染预览框", () => {
    const view = render(<QuestionPanels items={[withPreview]} interactive={false} queuedAnswers={new Map()} onToggle={noop} />);
    expect(view.container.querySelector(".chat-question-preview")).toBeTruthy();
    cleanup();
    const plain = render(<QuestionPanels mode="answer" items={[question(false)]} answers={new Map()} onSelect={noop} onCustom={noop} />);
    expect(plain.container.querySelector(".chat-question-preview")).toBeNull();
  });
});
