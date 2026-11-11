import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { Message } from "./Message";
import { Transcript } from "./Transcript";
import { resetFoldMemory } from "./foldMemory";
import { EchoActionsContext, EchoPhaseContext, type EchoActions, type OutgoingPhase } from "./outgoingPrompts";
import type { ChatItem } from "../../api";

afterEach(() => { cleanup(); resetFoldMemory(); });

/// 用户消息页脚上的两条事实标注：中途插入（后端结构标记）与回显未确认（前端对账结果）。
describe("Message 用户消息标注", () => {
  it("回合中插进来的消息标「回合中插入」，普通消息不标", () => {
    render(<Message item={{ type: "user_text", id: "u1", timestamp: null, text: "顺便加个测试", interjected: true }} />);
    expect(screen.getByText("回合中插入")).toBeTruthy();
    cleanup();
    render(<Message item={{ type: "user_text", id: "u2", timestamp: null, text: "开始吧" }} />);
    expect(screen.queryByText("回合中插入")).toBeNull();
  });

  it("回显超时未确认时照实标出，正常回显不标", () => {
    const item: ChatItem = { type: "user_text", id: "echo-1", timestamp: null, text: "在吗" };
    const renderWith = (phase: OutgoingPhase) => render(
      <EchoPhaseContext.Provider value={new Map([["echo-1", phase]])}>
        <Message item={item} />
      </EchoPhaseContext.Provider>,
    );
    renderWith("pending");
    expect(screen.queryByText("未确认送达")).toBeNull();
    cleanup();
    renderWith("uncertain");
    expect(screen.getByText("未确认送达")).toBeTruthy();
  });

  // 未确认送达要给出口，手机上不能只剩「去终端页看看」：还停在输入框里给「提交」，否则重发 / 放回。
  it("未确认送达：卡在输入框给提交，否则给重发与放回输入框", () => {
    const item: ChatItem = { type: "user_text", id: "echo-7", timestamp: null, text: "在吗" };
    const calls: string[] = [];
    const renderWith = (stuck: boolean) => {
      const actions: EchoActions = {
        retry: (id) => calls.push(`retry:${id}`),
        restore: (id) => calls.push(`restore:${id}`),
        submit: (id) => calls.push(`submit:${id}`),
        stuck: new Set(stuck ? [7] : []),
      };
      return render(
        <EchoActionsContext.Provider value={actions}>
          <EchoPhaseContext.Provider value={new Map([["echo-7", "uncertain" as OutgoingPhase]])}>
            <Message item={item} />
          </EchoPhaseContext.Provider>
        </EchoActionsContext.Provider>,
      );
    };
    renderWith(false);
    fireEvent.click(screen.getByRole("button", { name: "重发" }));
    fireEvent.click(screen.getByRole("button", { name: "放回输入框" }));
    expect(screen.queryByRole("button", { name: "提交" })).toBeNull();
    cleanup();
    renderWith(true);
    expect(screen.getByText("停在终端输入框里")).toBeTruthy();
    expect(screen.queryByText("未确认送达")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "提交" }));
    expect(calls).toEqual(["retry:7", "restore:7", "submit:7"]);
  });
});

/// 中断标记（后端把 CLI 的 `[Request interrupted by user]` 转成 meta）：分隔条写「已手动终止」，
/// 不再冒充用户气泡，也不能落进兜底的「上下文已压缩」。
describe("Message 分隔条", () => {
  it("interrupted 显示已手动终止，compact 照旧", () => {
    render(<Message item={{ type: "meta", id: "m1", timestamp: null, kind: "interrupted" }} />);
    expect(screen.getByText("已手动终止")).toBeTruthy();
    expect(screen.queryByText("上下文已压缩")).toBeNull();
    cleanup();
    render(<Message item={{ type: "meta", id: "m2", timestamp: null, kind: "compact" }} />);
    expect(screen.getByText("上下文已压缩")).toBeTruthy();
  });
});

/// 长用户消息收到 10 行（对照 Kimi）：只有实测超高才给「展开全部」，点开后可再收起。
describe("Message 长用户消息", () => {
  it("超高才出展开钮，展开后去掉收起限制", () => {
    const proto = HTMLElement.prototype;
    const sh = Object.getOwnPropertyDescriptor(proto, "scrollHeight");
    const ch = Object.getOwnPropertyDescriptor(proto, "clientHeight");
    let tall = true;
    Object.defineProperty(proto, "scrollHeight", { configurable: true, get() { return tall && this.classList.contains("chat-user-clip") ? 900 : 0; } });
    Object.defineProperty(proto, "clientHeight", { configurable: true, get() { return this.classList.contains("chat-user-clip") ? 300 : 0; } });
    try {
      render(<Message item={{ type: "user_text", id: "long", timestamp: null, text: "很长的日志" }} />);
      const toggle = screen.getByRole("button", { name: "展开全部" });
      expect(document.querySelector(".chat-user-text.is-clamped")).toBeTruthy();
      fireEvent.click(toggle);
      expect(screen.getByRole("button", { name: "收起" })).toBeTruthy();
      expect(document.querySelector(".chat-user-clip.is-clampable")).toBeNull();
      cleanup();
      tall = false;
      render(<Message item={{ type: "user_text", id: "short", timestamp: null, text: "短消息" }} />);
      expect(screen.queryByRole("button", { name: "展开全部" })).toBeNull();
    } finally {
      if (sh) Object.defineProperty(proto, "scrollHeight", sh); else delete (proto as { scrollHeight?: number }).scrollHeight;
      if (ch) Object.defineProperty(proto, "clientHeight", ch); else delete (proto as { clientHeight?: number }).clientHeight;
    }
  });
});

/// 压缩分隔条（对照 Kimi）：自动压缩说清是自动、带压缩前规模；紧随的摘要挂到分隔条的
/// 「查看摘要」里，默认收起，不再以单独一行或用户气泡出现。
describe("压缩分隔条", () => {
  it("自动压缩显示规模，摘要收在查看摘要里", () => {
    render(<Transcript sessionId={1} items={[
      { type: "meta", id: "b1", timestamp: null, kind: "compact", compaction: { trigger: "auto", pre_tokens: 592064 } },
      { type: "meta", id: "s1", timestamp: null, kind: "compact_summary", compaction: { summary: "保留的要点：重构对话窗" } },
    ]} />);
    expect(screen.getByText("已自动压缩上下文")).toBeTruthy();
    expect(screen.getByText("压缩前 592K tokens")).toBeTruthy();
    expect(screen.queryByText("保留的要点：重构对话窗")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "查看摘要" }));
    expect(screen.getByText("保留的要点：重构对话窗")).toBeTruthy();
    expect(screen.getByRole("button", { name: "收起摘要" })).toBeTruthy();
  });

  it("没有细节时照旧一句上下文已压缩，没有摘要钮", () => {
    render(<Transcript sessionId={1} items={[{ type: "meta", id: "b2", timestamp: null, kind: "compact" }]} />);
    expect(screen.getByText("上下文已压缩")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "查看摘要" })).toBeNull();
  });
});
