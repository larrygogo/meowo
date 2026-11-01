import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
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
});
