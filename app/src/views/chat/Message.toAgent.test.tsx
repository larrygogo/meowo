import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { Message } from "./Message";
import { resetFoldMemory } from "./foldMemory";
import { anchorFrom, reconcileOutgoing, type OutgoingPrompt } from "./outgoingPrompts";
import type { ChatItem } from "../../api";

afterEach(() => { cleanup(); resetFoldMemory(); });

/// 用户实拍 11-11：在 Claude Code 里切到子代理视图（输入框「Message @general-purpose…」）发的话，
/// 正文只记在子代理侧车里。后端取回来挂到主时间线、标上 to_agent；前端要标明去向，
/// 回显据此结算，不再挂「未确认送达」。
describe("发给子任务的用户消息", () => {
  const toAgent: ChatItem = {
    type: "user_text", id: "side-u1", timestamp: "2026-11-11T04:38:44.141Z",
    text: "两边都已经通过了", to_agent: "拆分导入任务",
  };

  it("页脚标出发给了哪个子任务，普通消息不标", () => {
    render(<Message item={toAgent} />);
    expect(screen.getByText("发给子任务「拆分导入任务」")).toBeTruthy();
    cleanup();
    render(<Message item={{ type: "user_text", id: "u2", timestamp: null, text: "开始吧" }} />);
    expect(screen.queryByText(/发给子任务/)).toBeNull();
  });

  it("回显见到这条就算送达，不判未确认", () => {
    const before: ChatItem[] = [{ type: "assistant_text", id: "a1", timestamp: "2026-11-11T04:30:00.000Z", text: "收工" }];
    const echo: OutgoingPrompt = {
      id: 1, kind: "echo", text: "两边都已经通过了", attachmentOnly: false,
      at: Date.parse("2026-11-11T04:38:44.000Z"), phase: "pending", settledAt: null,
      anchor: anchorFrom(before, null),
    };
    const now = Date.parse("2026-11-11T04:39:30.000Z");
    const settled = reconcileOutgoing([echo], { items: [...before, toAgent], lastUserText: null, ended: false, now });
    expect(settled).toEqual([]);
  });
});
