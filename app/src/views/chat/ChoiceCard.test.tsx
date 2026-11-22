import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatItem } from "../../api";
import { ChatActionsContext, type ChatActions } from "./chatActions";
import { ChoiceMessage, parseChoicesReceipt } from "./ChoiceCard";
import { formatChoiceAnswer, parseChoiceAnswer } from "./choiceReply";
import { ToolActivity } from "./ToolActivity";
import { TurnLiveContext } from "./toolState";
import { Transcript } from "./Transcript";

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  convertFileSrc: (path: string) => `asset://local/${path}`,
}));

afterEach(cleanup);

const DIR = "C:\\Users\\me\\.meowo\\images\\sent\\";
type ToolUse = Extract<ChatItem, { type: "tool_use" }>;
type ToolResult = Extract<ChatItem, { type: "tool_result" }>;
const call = (id: string): ToolUse =>
  ({ type: "tool_use", id, timestamp: null, name: "mcp__meowo__offer_choices", kind: "other", summary: "" } as ToolUse);
const result = (id: string, text: string, isError = false): ToolResult =>
  ({ type: "tool_result", id: `r-${id}`, timestamp: null, tool_use_id: id, text, is_error: isError });
const OPTIONS = [
  { label: "卡片式", description: "留白多", image: 0 },
  { label: "列表式", description: "像系统设置] 密度高", image: 1 },
  { label: "分组式", description: null, image: null },
];
const receipt = (options: unknown[] = OPTIONS, refs = [`${DIR}a.png`, `${DIR}b.png`]) => [
  "方案卡已显示给用户。",
  ...refs.map((path) => `[Image: source: ${path}]`),
  `[Choices: ${JSON.stringify({ question: "设置页用哪种布局？", options })}]`,
].join("\n");

function withActions(node: React.ReactNode, sendText = vi.fn(async () => true)) {
  const actions: ChatActions = { sendText };
  return { sendText, view: render(<ChatActionsContext.Provider value={actions}>{node}</ChatActionsContext.Provider>) };
}

describe("方案卡回执", () => {
  it("取出题面与方案；截图按下标对上引用行，说明里有 ] 也不截断", () => {
    const parsed = parseChoicesReceipt(receipt())!;
    expect(parsed.question).toBe("设置页用哪种布局？");
    expect(parsed.options).toEqual([
      { label: "卡片式", description: "留白多", image: `${DIR}a.png` },
      { label: "列表式", description: "像系统设置] 密度高", image: `${DIR}b.png` },
      { label: "分组式", description: null, image: null },
    ]);
  });

  it("引用行被改写成普通文字（不可信目录）时按没图显示；不足两个方案或摘要坏了返回 null", () => {
    const parsed = parseChoicesReceipt(receipt(OPTIONS, []))!;
    expect(parsed.options.map((option) => option.image)).toEqual([null, null, null]);
    expect(parseChoicesReceipt(receipt([{ label: "只有一个" }]))).toBeNull();
    expect(parseChoicesReceipt("[Choices: {坏了]")).toBeNull();
  });
});

describe("方案卡替用户发的消息", () => {
  const card = { question: "设置页用哪种布局？", labels: ["卡片式", "列表式"] };

  it("中英两种写法都认得；带补充", () => {
    const zh = formatChoiceAnswer({ kind: "picked", index: 1, note: "按钮用 1 的颜色" }, card, "zh-CN");
    expect(zh).toBe("选了方案 2：列表式\n补充：按钮用 1 的颜色");
    expect(parseChoiceAnswer(zh, card)).toEqual({ kind: "picked", index: 1, note: "按钮用 1 的颜色" });
    const en = formatChoiceAnswer({ kind: "none", note: null }, card, "en-US");
    expect(en).toBe("None of these options: 设置页用哪种布局？");
    expect(parseChoiceAnswer(en, card)).toEqual({ kind: "none", note: null });
  });

  it("只认这张卡发的：方案名、题面要对上；用户手打的相似句子不算", () => {
    // 同一轮另一张配色卡的消息：序号 2 在范围内，但方案名不是这张卡的。
    expect(parseChoiceAnswer("选了方案 2：浅色", card)).toBeNull();
    expect(parseChoiceAnswer("方案都不要：主色用哪个？", card)).toBeNull();
    expect(parseChoiceAnswer("选了方案 2：其实我想说别的", card)).toBeNull();
    expect(parseChoiceAnswer("选了方案 2：列表式\n然后顺便把首页也改了", card)).toBeNull();
    expect(parseChoiceAnswer("选了方案 3：不存在", card)).toBeNull();
    expect(parseChoiceAnswer("我觉得第二个好", card)).toBeNull();
  });
});

describe("方案卡", () => {
  it("并排显示截图与方案名；点一个再点「选 N 发送」，以用户身份发出，卡片随即收成一行", async () => {
    const { sendText } = withActions(<ChoiceMessage item={call("c1")} result={result("c1", receipt())} />);
    const imgs = [...document.querySelectorAll(".chat-choice-shot img")].map((img) => img.getAttribute("src"));
    expect(imgs).toEqual([`asset://local/${DIR}a.png`, `asset://local/${DIR}b.png`]);
    const send = document.querySelector<HTMLButtonElement>(".chat-choices-send")!;
    expect(send.disabled).toBe(true);
    fireEvent.click(screen.getAllByRole("radio")[1]);
    expect(screen.getAllByRole("radio")[1].getAttribute("aria-checked")).toBe("true");
    fireEvent.change(document.querySelector(".chat-choices-note")!, { target: { value: "按钮用 1 的颜色" } });
    await act(async () => { fireEvent.click(send); });
    expect(sendText).toHaveBeenCalledWith("选了方案 2：列表式\n补充：按钮用 1 的颜色");
    expect(document.querySelector(".chat-choices.is-settled")!.textContent).toContain("你选了 2 · 列表式");
    expect(document.querySelector(".chat-choices-foot")).toBeNull();
  });

  it("数字键选方案；输入框里按数字不抢；回车发送", async () => {
    const { sendText } = withActions(<ChoiceMessage item={call("c2")} result={result("c2", receipt())} />);
    const card = document.querySelector(".chat-choices")!;
    fireEvent.keyDown(card, { key: "3" });
    expect(screen.getAllByRole("radio")[2].getAttribute("aria-checked")).toBe("true");
    const note = document.querySelector<HTMLInputElement>(".chat-choices-note")!;
    fireEvent.keyDown(note, { key: "1" });
    expect(screen.getAllByRole("radio")[2].getAttribute("aria-checked")).toBe("true");
    await act(async () => { fireEvent.keyDown(note, { key: "Enter", keyCode: 13 }); });
    expect(sendText).toHaveBeenCalledWith("选了方案 3：分组式");
  });

  it("「都不要」也发一条消息；发送失败留在卡上并提示重试", async () => {
    const { sendText } = withActions(<ChoiceMessage item={call("c3")} result={result("c3", receipt())} />, vi.fn(async () => false));
    await act(async () => { fireEvent.click(document.querySelector(".chat-choices-none")!); });
    expect(sendText).toHaveBeenCalledWith("方案都不要：设置页用哪种布局？");
    expect(screen.getByRole("alert").textContent).toContain("没发出去");
    expect(document.querySelector(".chat-choices-foot")).toBeTruthy();
  });

  it("点放大进大图相册，只收有截图的方案；大图里「选方案 N」选中并关闭", () => {
    withActions(<ChoiceMessage item={call("c4")} result={result("c4", receipt())} />);
    // 有截图的方案图片读到了才给放大按钮。
    document.querySelectorAll(".chat-choice-shot img").forEach((img) => fireEvent.load(img));
    const zooms = document.querySelectorAll<HTMLButtonElement>(".chat-choice-zoom");
    expect(zooms.length).toBe(2);
    fireEvent.click(zooms[1]);
    expect(document.querySelector(".chat-lightbox-count")!.textContent).toBe("2 / 2");
    fireEvent.click(screen.getByRole("button", { name: "选方案 2" }));
    expect(document.querySelector(".chat-lightbox-count")).toBeNull();
    expect(screen.getAllByRole("radio")[1].getAttribute("aria-checked")).toBe("true");
  });

  it("回放旧会话：之后那条消息是卡片发的，显示当时的选择；用户直接打字回复了，卡片不再接受点选", () => {
    withActions(<ChoiceMessage item={call("c5")} result={result("c5", receipt())} followUp={"选了方案 1：卡片式\n补充：再紧凑点"} />);
    const settled = document.querySelector(".chat-choices.is-settled")!;
    expect(settled.textContent).toContain("你选了 1 · 卡片式");
    expect(settled.textContent).toContain("补充：再紧凑点");
    expect(screen.queryAllByRole("radio")).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "看全部方案" }));
    expect(document.querySelectorAll(".chat-choice").length).toBe(3);
    expect(document.querySelector(".chat-choice.is-selected .chat-choice-text b")!.textContent).toBe("卡片式");
    cleanup();
    withActions(<ChoiceMessage item={call("c6")} result={result("c6", receipt())} followUp="我想再看一版" />);
    expect(document.querySelector(".chat-choices.is-settled")!.textContent).toContain("没有在卡上选");
  });

  it("对话窗之外（子任务记录）拿不到发消息的动作：只展示，不出按钮", () => {
    render(<ToolActivity item={call("c7")} result={result("c7", receipt())} />);
    expect(document.querySelector(".chat-choices")).toBeTruthy();
    expect(screen.queryAllByRole("radio")).toHaveLength(0);
    expect(document.querySelector(".chat-choices-foot")).toBeNull();
  });

  it("失败回执把原因原样显示（参数不对是给 agent 的一句话）；没有回执时是正在准备", () => {
    render(<ChoiceMessage item={call("c8")} result={result("c8", "[Image error: not_found] options 要有 2~4 个方案，现在是 1 个", true)} />);
    expect(document.querySelector(".chat-sent-failed")!.textContent).toContain("options 要有 2~4 个方案");
    cleanup();
    render(<TurnLiveContext.Provider value="running"><ChoiceMessage item={call("c9")} /></TurnLiveContext.Provider>);
    expect(screen.getByRole("status").textContent).toContain("正在准备方案");
    cleanup();
    // 回合已经停了还没回执：中断，不再转圈。
    render(<ChoiceMessage item={call("c10")} />);
    expect(document.querySelector(".chat-sent-failed")).toBeTruthy();
  });

  it("发出后卡片重新挂载（切走会话再回来）：仍是已选，不会再发一次", async () => {
    const { sendText, view } = withActions(<ChoiceMessage item={call("m1")} result={result("m1", receipt())} />);
    fireEvent.click(screen.getAllByRole("radio")[0]);
    await act(async () => { fireEvent.click(document.querySelector(".chat-choices-send")!); });
    view.unmount();
    withActions(<ChoiceMessage item={call("m1")} result={result("m1", receipt())} />, sendText);
    expect(document.querySelector(".chat-choices.is-settled")!.textContent).toContain("你选了 1 · 卡片式");
    expect(sendText).toHaveBeenCalledTimes(1);
  });

  it("同一轮两张卡：之后那条消息只记到它对应的那张上", () => {
    const colors = receipt([{ label: "深色", description: null, image: null }, { label: "浅色", description: null, image: null }], [])
      .replace("设置页用哪种布局？", "主色用哪个？");
    const items: ChatItem[] = [
      { type: "user_text", id: "u", timestamp: null, text: "做两版" },
      call("x1"), result("x1", receipt()),
      call("x2"), result("x2", colors),
      { type: "assistant_text", id: "a", timestamp: null, text: "两张卡你都看看" },
      { type: "user_text", id: "u2", timestamp: null, text: "选了方案 2：列表式" },
    ];
    withActions(<Transcript sessionId={1} items={items} />);
    const cards = [...document.querySelectorAll(".chat-choices")];
    expect(cards[0].textContent).toContain("你选了 2 · 列表式");
    expect(cards[1].textContent).toContain("没有在卡上选");
    expect(cards[1].textContent).not.toContain("你选了");
  });

  it("排在正文里不收进过程块；之后来了用户消息，缓存的旧段也会重算成已选", () => {
    const base: ChatItem[] = [
      { type: "user_text", id: "u", timestamp: null, text: "把设置页做好看点" },
      { type: "tool_use", id: "b1", timestamp: null, name: "Bash", kind: "run", summary: "npm run dev" } as ChatItem,
      result("b1", "ok"),
      call("k1"), result("k1", receipt()),
      { type: "assistant_text", id: "a", timestamp: null, text: "你挑一个" },
    ];
    const view = withActions(<Transcript sessionId={1} items={base} />).view;
    const card = document.querySelector(".chat-choices")!;
    expect(card.closest(".turn-fold-body, details")).toBeNull();
    expect(screen.getAllByRole("radio")).toHaveLength(3);
    const next: ChatItem[] = [...base, { type: "user_text", id: "u2", timestamp: null, text: "选了方案 2：列表式" }];
    view.rerender(<ChatActionsContext.Provider value={{ sendText: async () => true }}><Transcript sessionId={1} items={next} /></ChatActionsContext.Provider>);
    expect(document.querySelector(".chat-choices.is-settled")!.textContent).toContain("你选了 2 · 列表式");
  });
});
