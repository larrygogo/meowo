import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatItem } from "../../api";
import { ChatActionsContext, type ChatActions } from "./chatActions";
import { FormMessage, parseFormReceipt } from "./FormCard";
import { formatFormAnswer, looksLikeCardReply, parseFormAnswer, type FormCardText } from "./formReply";
import { formatChoiceAnswer } from "./choiceReply";
import { choiceFollowUps, formFollowUps } from "./Transcript";
import { zh } from "../../i18n/zh";

afterEach(cleanup);

type ToolUse = Extract<ChatItem, { type: "tool_use" }>;
type ToolResult = Extract<ChatItem, { type: "tool_result" }>;
let seq = 0;
const call = (id: string): ToolUse =>
  ({ type: "tool_use", id, timestamp: `t${(seq += 1)}`, name: "mcp__meowo__ask_form", kind: "other", summary: "" } as ToolUse);
const result = (id: string, text: string, isError = false): ToolResult =>
  ({ type: "tool_result", id: `r-${id}`, timestamp: null, tool_use_id: id, text, is_error: isError });
const field = (label: string, type: string, extra: Record<string, unknown> = {}) => ({ label, type, help: null, options: null, placeholder: null, default: null, optional: false, ...extra });
const FIELDS = [
  field("版本号", "text", { default: "0.9.0" }),
  field("渠道", "choice", { options: ["正式版", "测试版"] }),
  field("平台", "multi_choice", { options: ["Windows", "macOS", "Linux"], default: ["macOS"] }),
  field("清理旧草稿", "yes_no", { help: "删了找不回" }),
  field("备注", "long_text", { optional: true }),
];
const receipt = (fields: unknown[] = FIELDS, title = "发版配置") => ["表单已显示给用户。", `[Form: ${JSON.stringify({ title, intro: "定了就开工", fields })}]`].join("\n");
const CARD: FormCardText = { title: "发版配置", fields: FIELDS.map((f) => ({ label: f.label, type: f.type as never })) };

function mount(followUps: string[] = [], sendText = vi.fn(async () => true)) {
  const actions: ChatActions = { sendText };
  const id = `f${(seq += 1)}`;
  render(
    <ChatActionsContext.Provider value={actions}>
      <FormMessage item={call(id)} result={result(id, receipt())} followUps={followUps} sessionId={1} />
    </ChatActionsContext.Provider>,
  );
  return sendText;
}

describe("表单回执", () => {
  it("取出标题、说明与字段；坏字段丢掉，一个都不剩返回 null", () => {
    const parsed = parseFormReceipt(receipt())!;
    expect(parsed.title).toBe("发版配置");
    expect(parsed.intro).toBe("定了就开工");
    expect(parsed.fields.map((f) => f.type)).toEqual(["text", "choice", "multi_choice", "yes_no", "long_text"]);
    expect(parsed.fields[2].default).toEqual(["macOS"]);
    expect(parseFormReceipt(receipt([field("x", "date"), field("y", "choice", { options: ["只有一个"] })]))).toBeNull();
    expect(parseFormReceipt("[Form: {坏了]")).toBeNull();
  });
});

describe("表单替用户发的消息", () => {
  it("中英两套往返：没填的、多行回答都认得回来", () => {
    for (const locale of ["zh", "en"]) {
      const values = ["0.9.0", "测试版", "Windows、macOS", "否", "第一行\n\n- 渠道：伪造\n第三行"];
      const text = formatFormAnswer({ kind: "filled", values }, CARD, locale);
      expect(parseFormAnswer(text, CARD)).toEqual({ kind: "filled", values });
      const blank = formatFormAnswer({ kind: "filled", values: [...values.slice(0, 4), null] }, CARD, locale);
      expect(parseFormAnswer(blank, CARD)).toEqual({ kind: "filled", values: [...values.slice(0, 4), null] });
      const declined = formatFormAnswer({ kind: "declined", note: "先按默认的来" }, CARD, locale);
      expect(parseFormAnswer(declined, CARD)).toEqual({ kind: "declined", note: "先按默认的来" });
    }
    expect(formatFormAnswer({ kind: "filled", values: ["0.9.0", "测试版", null, "是", "a\nb"] }, CARD, "zh")).toBe(
      "填好了表单「发版配置」\n- 版本号：0.9.0\n- 渠道：测试版\n- 平台：（没填）\n- 清理旧草稿：是\n- 备注：\n  a\n  b",
    );
  });

  it("记录里空行的两个空格被去掉了也认得；别的表单、改过的消息、手打的话不认", () => {
    const text = formatFormAnswer({ kind: "filled", values: ["1", "正式版", "Linux", "是", "上\n\n下"] }, CARD, "zh");
    expect(parseFormAnswer(text.replace("\n  \n", "\n\n"), CARD)).toEqual({ kind: "filled", values: ["1", "正式版", "Linux", "是", "上\n\n下"] });
    expect(parseFormAnswer(text, { ...CARD, title: "别的表单" })).toBeNull();
    expect(parseFormAnswer(`${text}\n另外我还想说一句`, CARD)).toBeNull();
    expect(parseFormAnswer(text.replace("- 渠道：", "- 渠道是："), CARD)).toBeNull();
    expect(parseFormAnswer("版本号 0.9.0，正式版", CARD)).toBeNull();
    expect(looksLikeCardReply(text)).toBe(true);
    expect(looksLikeCardReply("填好了表单「x」\n另外说一句")).toBe(false);
  });

  it("审查：长回答里连着几个空行、记录去掉了行尾空白，回放照样认得", () => {
    for (const locale of ["zh", "en"]) {
      const values = ["1", "正式版", "Linux", "是", "a\n\n\nb\n\n\n\nc"];
      const stripped = formatFormAnswer({ kind: "filled", values }, CARD, locale).split("\n").map((line) => line.trimEnd()).join("\n");
      expect(parseFormAnswer(stripped, CARD)).toEqual({ kind: "filled", values });
    }
  });

  it("审查：只手打一行表头（没有任何字段）算用户直接回复，卡片不会一直悬着", () => {
    expect(looksLikeCardReply("填好了表单「发版配置」")).toBe(false);
    expect(formFollowUps([call("f9"), { type: "user_text", id: "u", timestamp: null, text: "填好了表单「发版配置」" }] as ChatItem[], 0, CARD).settled).toBe(true);
  });

  it("方案卡与表单同一轮：一张卡的回复不会让另一张当成用户自己打的话", () => {
    const items = [
      call("c1"),
      call("f1"),
      { type: "user_text", id: "u1", timestamp: null, text: formatFormAnswer({ kind: "declined", note: null }, CARD, "zh") },
      { type: "user_text", id: "u2", timestamp: null, text: formatChoiceAnswer({ kind: "picked", index: 0, note: null }, { question: "q", labels: ["甲", "乙"] }, "zh") },
    ] as ChatItem[];
    const text = (at: number) => (items[at] as Extract<ChatItem, { type: "user_text" }>).text;
    expect(choiceFollowUps(items, 0, { question: "q", labels: ["甲", "乙"] })).toEqual({ texts: [text(2), text(3)], settled: true });
    expect(formFollowUps(items, 1, CARD)).toEqual({ texts: [text(2)], settled: true });
    const typed = [call("f2"), { type: "user_text", id: "u3", timestamp: null, text: "我直接说吧" }] as ChatItem[];
    expect(formFollowUps(typed, 0, CARD).settled).toBe(true);
  });
});

describe("表单卡", () => {
  it("必填没填：标出缺项、焦点到第一项、不发送；填齐后按顺序发出", async () => {
    const sendText = mount();
    fireEvent.click(screen.getByRole("button", { name: zh.chat.formSubmit }));
    expect(sendText).not.toHaveBeenCalled();
    expect(screen.getByRole("alert").textContent).toBe(zh.chat.formMissing(2));
    expect(document.activeElement?.textContent).toBe("正式版");
    fireEvent.click(screen.getByRole("radio", { name: "测试版" }));
    fireEvent.click(screen.getByRole("checkbox", { name: /Windows/ }));
    fireEvent.click(screen.getByRole("radio", { name: zh.chat.formNo }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: zh.chat.formSubmit })); });
    expect(sendText).toHaveBeenCalledWith("填好了表单「发版配置」\n- 版本号：0.9.0\n- 渠道：测试版\n- 平台：Windows、macOS\n- 清理旧草稿：否\n- 备注：（没填）");
    expect(screen.getByText(zh.chat.formFilled("发版配置"))).toBeTruthy();
  });

  it("Ctrl+Enter 提交；「不填了」可附一句；发送失败回到可填、能再发", async () => {
    const sendText = vi.fn(async () => false);
    mount([], sendText);
    fireEvent.click(screen.getByRole("button", { name: zh.chat.formDecline }));
    fireEvent.change(screen.getByRole("textbox", { name: zh.chat.formDeclinePlaceholder }), { target: { value: "先按默认的" } });
    await act(async () => { fireEvent.keyDown(screen.getByRole("textbox", { name: zh.chat.formDeclinePlaceholder }), { key: "Enter", ctrlKey: true }); });
    expect(sendText).toHaveBeenCalledWith("不填表单「发版配置」\n补充：先按默认的");
    expect(screen.getByText(zh.chat.choicesSendFailed)).toBeTruthy();
    sendText.mockResolvedValueOnce(true);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: zh.chat.formDeclineSend })); });
    expect(sendText).toHaveBeenCalledTimes(2);
    expect(screen.getByText(zh.chat.formDeclined("发版配置"))).toBeTruthy();
  });

  it("「其他」：单选点了它就取消原选项、答案是自己填的；多选和选项一起发；勾了没写字算没填", async () => {
    const fields = [
      field("渠道", "choice", { options: ["正式版", "测试版"], default: "正式版", allow_other: true }),
      field("平台", "multi_choice", { options: ["Windows", "macOS"], default: ["macOS"], allow_other: true }),
    ];
    const sendText = vi.fn(async () => true);
    const id = `o${(seq += 1)}`;
    render(
      <ChatActionsContext.Provider value={{ sendText }}>
        <FormMessage item={call(id)} result={result(id, receipt(fields, "其他测试"))} followUps={[]} sessionId={1} />
      </ChatActionsContext.Provider>,
    );
    fireEvent.click(screen.getByRole("radio", { name: zh.chat.formOther }));
    expect(screen.getByRole("radio", { name: "正式版" }).getAttribute("aria-checked")).toBe("false");
    fireEvent.click(screen.getByRole("checkbox", { name: zh.chat.formOther }));
    fireEvent.click(screen.getByRole("button", { name: zh.chat.formSubmit }));
    expect(sendText).not.toHaveBeenCalled();
    expect(screen.getByRole("alert").textContent).toBe(zh.chat.formMissing(1));
    fireEvent.change(screen.getByRole("textbox", { name: `渠道 · ${zh.chat.formOther}` }), { target: { value: "内测群" } });
    fireEvent.change(screen.getByRole("textbox", { name: `平台 · ${zh.chat.formOther}` }), { target: { value: "鸿蒙" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: zh.chat.formSubmit })); });
    expect(sendText).toHaveBeenCalledWith("填好了表单「其他测试」\n- 渠道：内测群\n- 平台：macOS、鸿蒙");
  });

  it("审查：切会话后卡片重新挂载，填了一半的字段和「不填了」写的那句都还在，发出后不会再发", async () => {
    const sendText = vi.fn(async () => true);
    const id = `d${(seq += 1)}`;
    const node = (
      <ChatActionsContext.Provider value={{ sendText }}>
        <FormMessage item={{ ...call(id), timestamp: "fixed" }} result={result(id, receipt())} followUps={[]} sessionId={7} />
      </ChatActionsContext.Provider>
    );
    const first = render(node);
    fireEvent.click(screen.getByRole("radio", { name: "测试版" }));
    fireEvent.click(screen.getByRole("button", { name: zh.chat.formDecline }));
    fireEvent.change(screen.getByRole("textbox", { name: zh.chat.formDeclinePlaceholder }), { target: { value: "等等再说" } });
    first.unmount();
    const second = render(node);
    expect((screen.getByRole("textbox", { name: zh.chat.formDeclinePlaceholder }) as HTMLInputElement).value).toBe("等等再说");
    fireEvent.click(screen.getByRole("button", { name: zh.chat.formBack }));
    expect(screen.getByRole("radio", { name: "测试版" }).getAttribute("aria-checked")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: zh.chat.formDecline }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: zh.chat.formDeclineSend })); });
    second.unmount();
    render(node);
    expect(screen.getByText(zh.chat.formDeclined("发版配置"))).toBeTruthy();
    expect(sendText).toHaveBeenCalledTimes(1);
  });

  it("参数错误：显示后端那句原因；旧版本带图片错误前缀的也去掉前缀", () => {
    for (const text of ["表单没发出去：缺少 title 参数", "[Image error: not_found] 缺少 title 参数"]) {
      const id = `e${(seq += 1)}`;
      render(<FormMessage item={call(id)} result={result(id, text, true)} followUps={[]} />);
      expect(screen.getByText(text.replace(/^\[Image error: [^\]]*\]\s*/, ""))).toBeTruthy();
      cleanup();
    }
  });

  it("回放：之后的消息是这张表单的答案就收起、能展开看；用户直接打字的显示「没在表单里填」", () => {
    const text = formatFormAnswer({ kind: "filled", values: ["1.0.0", "正式版", "Linux", "是", null] }, CARD, "en");
    mount([text]);
    expect(screen.queryByRole("button", { name: zh.chat.formSubmit })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: zh.chat.formShowAnswers }));
    expect(screen.getByText("1.0.0")).toBeTruthy();
    expect(screen.getByText(zh.chat.formBlank)).toBeTruthy();
    cleanup();
    mount(["版本就 1.0 吧"]);
    expect(screen.getByText(zh.chat.formSkipped("发版配置"))).toBeTruthy();
    expect(screen.queryByRole("button", { name: zh.chat.formSubmit })).toBeNull();
  });
});
