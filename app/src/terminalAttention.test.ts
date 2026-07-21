import { describe, expect, it } from "vitest";
import { terminalAttention, terminalNeedsAttention } from "./terminalAttention";

describe("terminalAttention", () => {
  it("returns the provider-declared trust screen for the GUI", () => {
    const prompt = "\x1b[2JDo you trust the contents of this directory?\r\n❯ 1. Yes, I trust this folder\r\n  2. No, exit";
    const attention = terminalAttention(prompt, ["do you trust the contents of this directory"]);
    expect(attention?.id).toContain("provider:");
    expect(attention?.options?.map((option) => option.label)).toEqual(["Yes, I trust this folder", "No, exit"]);
    expect(terminalNeedsAttention(prompt, ["do you trust the contents of this directory"])).toBe(true);
  });

  it("detects a credential/token step even when the provider did not declare its wording", () => {
    const prompt = "\x1b[2JOAuth token has been revoked\r\nRun /login to sign in\r\nPress Enter to continue · Esc to cancel";
    const attention = terminalAttention(prompt, []);
    expect(attention?.id).toContain("generic:");
    expect(attention?.text).toContain("OAuth token has been revoked");
  });

  it("selects the newest step after trust instead of keeping the old prompt forever", () => {
    const output = [
      "\x1b[2JDo you trust the files in this folder?\r\n  Yes\r\n  No",
      "\x1b[2JRestore authentication token for this session?\r\n  Restore\r\n  Continue without it",
    ].join("");
    const attention = terminalAttention(output, ["do you trust the files in this folder"]);
    expect(attention?.id).toContain("generic:");
    expect(attention?.text).not.toContain("Do you trust");
    expect(attention?.text).toContain("Restore authentication token");
  });

  it("does not mistake an ordinary TUI keyboard hint for a startup prompt", () => {
    const prompt = "\x1b[2JMigrate this session state?\r\n  Keep current\r\n  Recover previous\r\nEnter to select";
    expect(terminalAttention(prompt, [])).toBeNull();
  });

  it("surfaces a numbered TUI selector only when the session is waiting for an answer", () => {
    const prompt = [
      "\x1b[2J剩下这几个点要不要继续做？",
      "> 1. [ ] #1 首屏尾读",
      "  2. [ ] #2 连接复用",
      "  3. [ ] 都先不做",
      "  4. [ ] Type something",
      "Submit",
      "Enter to select · ↑/↓ to navigate · Esc to cancel",
    ].join("\r\n");
    expect(terminalAttention(prompt, [])).toBeNull();
    const attention = terminalAttention(prompt, [], true);
    expect(attention?.id).toBe("interactive:numbered-selector");
    expect(attention?.text).toContain("剩下这几个点要不要继续做？");
    expect(attention?.options?.map((option) => option.label)).toEqual([
      "#1 首屏尾读", "#2 连接复用", "都先不做", "Type something", "Submit",
    ]);
    expect(attention?.options?.[0].selected).toBe(false);
    expect(attention?.options?.[0].focused).toBe(true);
    expect(attention?.options?.[0].input).toBe("\r");
    expect(attention?.options?.[3].kind).toBe("input");
    expect(attention?.options?.[4].kind).toBe("submit");
  });

  it("deduplicates TUI redraws and ignores numbered prose outside checkbox choices", () => {
    const prompt = [
      "\x1b[2J1. <Transcript> 无虚拟化",
      "2. 每 650ms 轮询都新开一个 SQLite 连接",
      "剩下这几个点要不要继续做？",
      "> 1. [ ] #1 首屏尾读",
      "  2. [ ] #2 连接复用",
      "  3. [ ] 都先不做",
      "  4. [ ] Type something",
      "Submit",
      "剩下这几个点要不要继续做？",
      "  1. [✓] #1 首屏尾读",
      "> 2. [✓] #2 连接复用",
      "  3. [ ] 都先不做",
      "  4. [ ] Type something",
      "Submit",
      "Enter to select · ↑/↓ to navigate · Esc to cancel",
    ].join("\r\n");
    const attention = terminalAttention(prompt, [], true);
    expect(attention?.options?.map((option) => option.label)).toEqual([
      "#1 首屏尾读", "#2 连接复用", "都先不做", "Type something", "Submit",
    ]);
    expect(attention?.options?.slice(0, 2).map((option) => option.selected)).toEqual([true, true]);
    expect(attention?.options?.[1].focused).toBe(true);
    expect(attention?.options?.[2].input).toBe("\x1b[B\r");
    expect(attention?.options?.some((option) => option.label.includes("Transcript"))).toBe(false);
  });

  it("turns Claude's long-session token warning into explicit GUI choices", () => {
    const prompt = [
      "\x1b[2JThis session is 1d 9h old and 161.3k tokens.",
      "Resuming the full session will consume a substantial portion of your usage limits.",
      "❯ 1. Resume from summary (recommended)",
      "  2. Resume full session as-is",
      "  3. Don't ask me again",
      "Enter to confirm · Esc to cancel",
    ].join("\r\n");
    const attention = terminalAttention(prompt, []);
    expect(attention?.id).toBe("claude:long-session-resume");
    expect(attention?.options?.map((option) => option.label)).toEqual([
      "Resume from summary (recommended)",
      "Resume full session as-is",
      "Don't ask me again",
    ]);
    // 菜单首尾循环，必须从 ❯ 光标做相对移动，而不是盲按上键归零。
    expect(attention?.options?.[0].input).toBe("\r");
    expect(attention?.options?.[1].input).toBe("\x1b[B\r");
    expect(attention?.options?.[2].input).toBe("\x1b[B\x1b[B\r");
  });

  /// 中文本地化 CLI 的长会话菜单：选项不带 `1.` 编号、没有英文导航提示，过去会退化成
  /// 「上一项/下一项」。现在从 ❯ 光标块提取直接选项；与选项同缩进的说明句（句号结尾）
  /// 不能被吞进选项块。
  it("无编号的中文光标菜单也给直接选项，说明句不算选项", () => {
    const prompt = [
      "\x1b[2JThis session is 2d 3h old and 98.2k tokens.",
      "? 如何恢复这个长会话？",
      "  完整恢复会消耗较多额度，建议从摘要恢复。",
      "❯ 从摘要恢复（推荐）",
      "  恢复完整会话",
      "  取消",
    ].join("\r\n");
    const attention = terminalAttention(prompt, []);
    expect(attention?.id).toBe("claude:long-session-resume");
    expect(attention?.options?.map((option) => option.label)).toEqual([
      "从摘要恢复（推荐）",
      "恢复完整会话",
      "取消",
    ]);
    expect(attention?.options?.[0].input).toBe("\r");
    expect(attention?.options?.[1].input).toBe("\x1b[B\r");
    expect(attention?.options?.[2].input).toBe("\x1b[B\x1b[B\r");
  });

  /// 中文导航提示（「回车确认」而非 enter/select）同样算菜单信号。
  it("光标菜单的导航提示支持中文", () => {
    const screen = [
      "\x1b[2JSelect a model",
      "↑↓ 移动 · 回车确认",
      "  K2.7",
      "❯ K3",
    ].join("\r\n");
    const attention = terminalAttention(screen, [], false, true);
    expect(attention?.id).toBe("interactive:cursor-menu");
    expect(attention?.options?.map((option) => option.label)).toEqual(["K2.7", "K3"]);
    expect(attention?.options?.[0].input).toBe("\x1b[A\r");
  });

  /// 画面取自真机抓屏（app/src-tauri/tests/capture_model_menu.rs 跑 kimi `/model` 的结果）。
  /// 与编号选择器是两种形态：这里没有 `1.`，只有一个 ❯ 光标 + 一句导航提示。
  it("把 kimi 的 /model 光标菜单转成 GUI 选项", () => {
    const screen = [
      "\x1b[2J ──────────────────────────────────────────────",
      " Select a model  (type to search)",
      "  Tab toggle provider · ↑↓ navigate · Enter select · Alt+S session-only · Esc cancel",
      "  All   Kimi Code",
      "     K2.7 Coding            Kimi Code",
      "     K2.7 Coding Highspeed  Kimi Code",
      "   ❯ K3                     Kimi Code ← current",
      "  Thinking  (←→ to switch)",
      "     Low      High    [ Max ]",
      " ──────────────────────────────────────────────",
    ].join("\r\n");
    // 第四个参数 expectMenu：只有刚发出会弹菜单的命令时才认，避免把正文误报成菜单。
    expect(terminalAttention(screen, [])).toBeNull();
    const attention = terminalAttention(screen, [], false, true);
    expect(attention?.id).toBe("interactive:cursor-menu");
    expect(attention?.text).toContain("Select a model");
    // 只圈出与 ❯ 同缩进的那段：provider 过滤行（缩进更浅）和 Thinking 小节都不算选项。
    expect(attention?.options?.map((option) => option.label)).toEqual([
      "K2.7 Coding  Kimi Code",
      "K2.7 Coding Highspeed  Kimi Code",
      "K3  Kimi Code ← current",
    ]);
    // 菜单首尾循环，必须从 ❯ 做相对移动。
    expect(attention?.options?.[0].input).toBe("\x1b[A\x1b[A\r");
    expect(attention?.options?.[1].input).toBe("\x1b[A\r");
    expect(attention?.options?.[2].input).toBe("\r");
    expect(attention?.options?.[2].focused).toBe(true);
  });

  /// 守卫必须在**开着识别窗口时**依然成立——否则「没开窗口所以返回 null」会让用例空转。
  it("光标菜单要同时有导航提示和多个同级项，避免把正文误报成菜单", () => {
    // 只有 ❯ 没有导航提示：提示符、列表装饰都长这样，不能算菜单。
    const prose = "\x1b[2J❯ 第一点\r\n❯ 第二点\r\n就这些";
    expect(terminalAttention(prose, [], false, true)).toBeNull();
    // 有提示但只有一项：多半是提示符本身。
    const single = "\x1b[2JPick one\r\n↑↓ navigate · Enter select\r\n  ❯ 唯一项";
    expect(terminalAttention(single, [], false, true)).toBeNull();
    // 有提示、有多项，但那几项缩进不一致（是正文段落而非菜单块）。
    const uneven = "\x1b[2JPick one\r\n↑↓ navigate · Enter select\r\n  ❯ 甲\r\n      乙\r\n  丙";
    expect(terminalAttention(uneven, [], false, true)?.options?.length ?? 0).toBeLessThan(3);
  });

  it("turns Claude's command approval into its three native choices", () => {
    const prompt = [
      "\x1b[2JBash command",
      "cargo build -p meowo-agent -p meowo-store 2>&1 | tail -20",
      "Build rust crates",
      "This command requires approval",
      "Do you want to proceed?",
      "❯ 1. Yes",
      "  2. Yes, and don't ask again for: cargo build *",
      "  3. No",
      "Esc to cancel · Tab to amend · ctrl+e to explain",
    ].join("\r\n");
    const attention = terminalAttention(prompt, []);
    expect(attention?.id).toBe("claude:command-approval");
    expect(attention?.text).toContain("cargo build -p meowo-agent");
    expect(attention?.text).toContain("Build rust crates");
    expect(attention?.text).toContain("Do you want to proceed?");
    expect(attention?.text).not.toContain("1. Yes");
    expect(attention?.options?.map((option) => option.label)).toEqual([
      "Yes",
      "Yes, and don't ask again for: cargo build *",
      "No",
    ]);
  });
});
