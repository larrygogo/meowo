import { describe, expect, it } from "vitest";
import { en } from "../../i18n/en";
import { zh } from "../../i18n/zh";
import { cardLockHint, composerCardLock, type CardLockInput } from "./composerCardLock";

const NONE: CardLockInput = { terminalCard: null, brokerApproval: false, brokerQuestion: false, pendingReview: null, terminalMenuOpen: false };

describe("有卡片待处理时输入框的规则", () => {
  it("没有卡片：不拦发送、不停控件、没有说明", () => {
    const lock = composerCardLock(NONE);
    expect(lock).toEqual({ send: false, controls: false, reason: null });
    expect(cardLockHint(lock, zh)).toBe("");
  });

  /// 走查 输入 B2：此前屏幕通道（kimi、antigravity、opencode）锁输入框，broker 通道（claude、codex）不锁、
  /// 发送后才报错。现在五家的审批都先拦发送，说明是同一句。
  it.each([
    ["claude", { brokerApproval: true }],
    ["codex", { brokerApproval: true }],
    ["kimi", { terminalCard: { kind: "approval" as const, collapsed: false } }],
    ["antigravity", { terminalCard: { kind: "approval" as const, collapsed: false } }],
    ["opencode", { terminalCard: { kind: "approval" as const, collapsed: false } }],
  ])("%s 的审批待处理：发送先拦住，说明同一句", (_provider, input) => {
    const lock = composerCardLock({ ...NONE, ...input });
    expect(lock.send).toBe(true);
    expect(lock.reason).toBe("approval");
    expect(cardLockHint(lock, zh)).toBe("先处理上方的审批再发送");
  });

  it.each([
    ["claude 的 broker 提问", { brokerQuestion: true }],
    ["kimi 的 broker 提问", { brokerQuestion: true }],
    ["kimi 屏幕上的方括号表单", { terminalCard: { kind: "question" as const, collapsed: false } }],
    ["antigravity 屏幕上的提问表单", { terminalCard: { kind: "question" as const, collapsed: false } }],
    ["会话记录报了提问、题面还没认出来", { pendingReview: "question" as const }],
  ])("%s：发送先拦住，说明是「先回答」", (_name, input) => {
    const lock = composerCardLock({ ...NONE, ...input });
    expect(lock.send).toBe(true);
    expect(cardLockHint(lock, zh)).toBe("先回答上方的提问再发送");
  });

  it("其余提示（信任页、额度耗尽、启动步骤）与终端菜单各有一句", () => {
    expect(cardLockHint(composerCardLock({ ...NONE, terminalCard: { kind: "prompt", collapsed: false } }), zh)).toBe("先处理上方的卡片再发送");
    expect(cardLockHint(composerCardLock({ ...NONE, terminalMenuOpen: true }), zh)).toBe("先完成终端里的选择再发送");
  });

  it("会话记录报了审批或计划批准而卡还没出来：同样先拦，不放一条消息进未决的审批", () => {
    for (const pendingReview of ["approval", "plan"] as const) {
      const lock = composerCardLock({ ...NONE, pendingReview });
      expect(lock).toEqual({ send: true, controls: false, reason: "approval" });
    }
  });

  it("控件停用只看屏幕上的卡是否摆在眼前：broker 请求、收起的卡、终端菜单都不停控件", () => {
    expect(composerCardLock({ ...NONE, terminalCard: { kind: "approval", collapsed: false } }).controls).toBe(true);
    expect(composerCardLock({ ...NONE, terminalCard: { kind: "question", collapsed: false } }).controls).toBe(true);
    expect(composerCardLock({ ...NONE, terminalCard: { kind: "prompt", collapsed: false } }).controls).toBe(true);
    // 「仅收起」之后终端里的框还在等：发送照拦，控件不再停。
    expect(composerCardLock({ ...NONE, terminalCard: { kind: "approval", collapsed: true } })).toEqual({ send: true, controls: false, reason: "approval" });
    expect(composerCardLock({ ...NONE, brokerApproval: true }).controls).toBe(false);
    expect(composerCardLock({ ...NONE, brokerQuestion: true }).controls).toBe(false);
    expect(composerCardLock({ ...NONE, terminalMenuOpen: true }).controls).toBe(false);
  });

  it("同时有几样在等：屏幕上的卡优先（终端正等着按键），其次 broker 审批", () => {
    expect(composerCardLock({ ...NONE, terminalCard: { kind: "prompt", collapsed: false }, brokerApproval: true, brokerQuestion: true }).reason).toBe("prompt");
    expect(composerCardLock({ ...NONE, brokerApproval: true, brokerQuestion: true, pendingReview: "question" }).reason).toBe("approval");
    expect(composerCardLock({ ...NONE, terminalMenuOpen: true, brokerQuestion: true }).reason).toBe("menu");
  });

  it("说明文字中英文齐全，不超过 20 个字，不用破折号和分号", () => {
    const keys = ["cardLockApproval", "cardLockQuestion", "cardLockPrompt", "cardLockMenu"] as const;
    for (const key of keys) {
      expect(zh.chat[key].length).toBeLessThanOrEqual(20);
      expect(zh.chat[key]).not.toMatch(/[—；;]|--/);
      expect(en.chat[key]).toBeTruthy();
      expect(en.chat[key]).not.toMatch(/[—;]|--/);
    }
  });

  it("CLI 自报在等人（OSC 7501）：没认出卡也先拦，原因按报告分类", () => {
    expect(composerCardLock({ ...NONE, reportedBlocked: "permission" })).toEqual({ send: true, controls: false, reason: "approval" });
    expect(composerCardLock({ ...NONE, reportedBlocked: "question" }).reason).toBe("question");
    expect(composerCardLock({ ...NONE, reportedBlocked: "auth" }).reason).toBe("prompt");
    // 认不出的原因、没有原因：不锁（认屏幕的 blocked 没有原因，照旧由卡片决定）。
    expect(composerCardLock({ ...NONE, reportedBlocked: "coffee" }).send).toBe(false);
    expect(composerCardLock({ ...NONE, reportedBlocked: null }).send).toBe(false);
    // 更具体的来源优先：屏幕上的卡、broker、hook 报的待处理。
    expect(composerCardLock({ ...NONE, reportedBlocked: "permission", terminalCard: { kind: "question", collapsed: false } }).reason).toBe("question");
    expect(composerCardLock({ ...NONE, reportedBlocked: "permission", pendingReview: "question" }).reason).toBe("question");
  });
});
