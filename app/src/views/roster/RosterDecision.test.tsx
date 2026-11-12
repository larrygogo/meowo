import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// 行内决定单：拒绝、说明并拒绝、危险命令标红、卡片出现不抢正在打字的焦点、回答 Lead 关卡。
// 卡片本体是对话窗那一套（BrokerApprovalCard / LeadGateCard），这里钉的是「在名单行里用它」
// 时发出去的决定和行内特有的规矩。批准与 Esc 只收起见 RosterShell.test。
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})), emit: vi.fn(() => Promise.resolve()) }));

import { makeSession } from "../../demo/data";
import type { Item } from "../sticker/types";
import { RosterDecision } from "./RosterDecision";

let approval: unknown = null;
let gate: unknown = null;
let question: unknown = null;
const pending = (patch: (s: Item) => void = () => {}) => {
  const s = makeSession({ title: "乙", project: "p" });
  s.pending_review = "approval";
  patch(s);
  return s;
};
const ask = (id: number, input: string) => ({ sessionId: id, requestId: "req-1", provider: "claude", toolName: "Bash", description: null, input, permissionSuggestions: [], answerable: false });
const decisions = () => invoke.mock.calls.filter(([cmd]) => cmd === "resolve_pending_approval").map(([, args]) => args);

describe("名单行内的决定单", () => {
  afterEach(cleanup);
  beforeEach(() => {
    approval = null;
    gate = null;
    question = null;
    invoke.mockReset();
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "pending_interaction") return Promise.resolve({ approval, question });
      if (cmd === "lead_task_snapshot") return Promise.resolve(gate ? { gate } : null);
      return Promise.resolve(undefined);
    });
  });

  it("拒绝：按这一行的会话号和请求号发出 deny", async () => {
    const item = pending();
    approval = ask(item.session.id, JSON.stringify({ command: "git push" }));
    render(<RosterDecision item={item} fallback={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "拒绝" }));
    expect(decisions()).toEqual([{ sessionId: item.session.id, requestId: "req-1", choice: "deny" }]);
  });

  it("说明并拒绝：理由随拒绝一起发给它", async () => {
    const item = pending();
    approval = ask(item.session.id, JSON.stringify({ command: "git push" }));
    const view = render(<RosterDecision item={item} fallback={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "说明并拒绝" }));
    const reason = view.container.querySelector<HTMLInputElement>(".chat-approval-reason input")!;
    fireEvent.change(reason, { target: { value: "先别推，等我看完" } });
    fireEvent.submit(reason.form!);
    expect(decisions()).toEqual([{ sessionId: item.session.id, requestId: "req-1", choice: "deny:先别推，等我看完" }]);
  });

  it("危险命令的允许钮标红，普通命令不标", async () => {
    const item = pending();
    approval = ask(item.session.id, JSON.stringify({ command: "rm -rf /" }));
    const view = render(<RosterDecision item={item} fallback={null} />);
    expect((await screen.findByRole("button", { name: "允许一次" })).classList.contains("is-danger")).toBe(true);
    view.unmount();
    approval = ask(item.session.id, JSON.stringify({ command: "ls" }));
    render(<RosterDecision item={item} fallback={null} />);
    expect((await screen.findByRole("button", { name: "允许一次" })).classList.contains("is-danger")).toBe(false);
  });

  it("人正在别处打字时卡片出现：焦点不动", async () => {
    const item = pending();
    render(<><textarea aria-label="输入框" /><RosterDecision item={item} fallback={null} /></>);
    const box = screen.getByLabelText("输入框");
    box.focus();
    // 审批这时才到（行已经展开着）。
    approval = ask(item.session.id, JSON.stringify({ command: "git push" }));
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 1100)); });
    await screen.findByRole("button", { name: "允许一次" });
    expect(document.activeElement).toBe(box);
  });

  it("Lead 关卡：选一项提交，答复按这一行的目录和关卡号发出", async () => {
    const item = pending((s) => { s.pending_review = "gate"; s.cwd = "C:/repo"; });
    gate = { v: 1, id: "A-1", kind: "A", title: "确认规格", summary: "要不要开工", options: [], status: "pending", answer: null };
    render(<RosterDecision item={item} fallback={null} />);
    fireEvent.click(await screen.findByText("确认，开工"));
    fireEvent.click(screen.getByRole("button", { name: "提交" }));
    expect(invoke).toHaveBeenCalledWith("answer_lead_gate", { cwd: "C:/repo", gateId: "A-1", choice: "confirm", note: "" });
  });

  const ask2 = (id: number, answerable: boolean) => ({
    sessionId: id, requestId: "q-1", provider: "claude", toolName: "AskUserQuestion", description: null, permissionSuggestions: [], answerable,
    input: JSON.stringify({ questions: [{ question: "先做哪个？", header: "顺序", multiSelect: false, options: [{ label: "登录", description: "先修登录" }, { label: "支付", description: "先修支付" }] }] }),
  });

  it("Claude 的提问：行内点一个选项提交，答案按题面原文回给它", async () => {
    const item = pending((s) => { s.pending_review = "question"; s.pty_managed = true; });
    question = ask2(item.session.id, true);
    render(<RosterDecision item={item} fallback={null} />);
    await screen.findByText("顺序 · 先做哪个？");
    const submit = screen.getByRole("button", { name: "提交选择" }) as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    fireEvent.click(screen.getByText("支付"));
    fireEvent.click(submit);
    expect(decisions()).toEqual([{ sessionId: item.session.id, requestId: "q-1", choice: `answers:${JSON.stringify({ "先做哪个？": "支付" })}` }]);
    await screen.findByText("已处理");
  });

  it("答不了的提问（broker 没挂着等代答）：不摆作答卡，也不摆终端按键，说进现场作答", async () => {
    const item = pending((s) => { s.pending_review = "question"; s.pty_managed = true; });
    question = ask2(item.session.id, false);
    render(<RosterDecision item={item} fallback={null} />);
    await screen.findByText("它停在一个选择上，进入现场作答");
    expect(screen.queryByRole("button", { name: "提交选择" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Esc" })).toBeNull();
  });
});
