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
    expect(attention?.options?.[1].input).toBe("\x1b[A".repeat(8) + "\x1b[B\r");
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
