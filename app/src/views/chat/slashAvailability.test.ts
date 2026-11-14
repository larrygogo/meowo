// 菜单型斜杠命令与撤回确认框按能力给出的说法（走查输入 B1、B9、C3）。
import { describe, expect, it } from "vitest";
import { chatUi } from "../../test/agents";
import { zh } from "../../i18n/zh";
import { en } from "../../i18n/en";
import { slashCommandMechanism, slashEntryAvailability, slashSendAvailability } from "./availability";
import { undoConfirmMessage } from "./undoTerminal";

const PROVIDERS = ["claude", "codex", "kimi", "gemini", "opencode"] as const;

describe("菜单型斜杠命令此刻的状态（五家）", () => {
  it.each(PROVIDERS)("%s：声明为菜单型的命令回合中置灰带原因，空闲可用，普通命令不受影响", (provider) => {
    const ui = chatUi(provider)!;
    expect(ui.menu_slash_commands.length, "夹具里每家至少有一条菜单型命令").toBeGreaterThan(0);
    for (const name of ui.menu_slash_commands) {
      expect(slashCommandMechanism(ui, name)).toBe("terminal_menu");
      // 裸发：回合在跑、有卡片在等都做不了，原因与别的要开 CLI 菜单的入口同一句。
      expect(slashSendAvailability(zh, ui, name, "running")).toEqual({ state: "disabled", reason: zh.chat.availability.afterTurn });
      expect(slashSendAvailability(zh, ui, name, "pending")).toEqual({ state: "disabled", reason: zh.chat.availability.afterCard });
      expect(slashSendAvailability(en, ui, name, "running")).toEqual({ state: "disabled", reason: en.chat.availability.afterTurn });
      expect(slashSendAvailability(zh, ui, name, "waiting")).toEqual({ state: "available" });
      // 补全项：同名命令带参数能内联执行的（声明了 model_select_command）不置灰，其余与裸发一致。
      const inline = ui.model_select_command === name;
      expect(slashEntryAvailability(zh, ui, name, "running").state).toBe(inline ? "available" : "disabled");
      expect(slashEntryAvailability(zh, ui, name, "waiting")).toEqual({ state: "available" });
    }
    for (const command of ui.slash_commands.filter((candidate) => !ui.menu_slash_commands.includes(candidate.name))) {
      expect(slashCommandMechanism(ui, command.name)).toBeNull();
      expect(slashEntryAvailability(zh, ui, command.name, "running")).toEqual({ state: "available" });
      expect(slashSendAvailability(zh, ui, command.name, "running")).toEqual({ state: "available" });
    }
  });

  it("没有能力声明时一律按普通命令处理", () => {
    expect(slashEntryAvailability(zh, null, "/model", "running")).toEqual({ state: "available" });
    expect(slashSendAvailability(zh, undefined, "/model", "running")).toEqual({ state: "available" });
  });
});

describe("撤回确认框（五家，走查输入 C3）", () => {
  it.each(PROVIDERS)("%s：只有一次撤一轮的 CLI 多一句「只能撤回最近一轮」", (provider) => {
    const undo = chatUi(provider)!.undo;
    if (!undo) return; // 上游没有撤回命令的家没有入口，也就没有确认框。
    const latestOnly = !undo.count_arg && !undo.menu;
    for (const t of [zh, en]) {
      const message = undoConfirmMessage(t, undo);
      expect(message.startsWith(undo.restores_files ? t.chat.undoConfirmFiles : t.chat.undoConfirm)).toBe(true);
      expect(message.includes(t.chat.undoLatestOnly)).toBe(latestOnly);
    }
  });

  it("OpenCode 有这一句，Kimi（带轮数）与 Claude（菜单式）没有", () => {
    expect(undoConfirmMessage(zh, chatUi("opencode")!.undo!)).toBe(`${zh.chat.undoConfirmFiles}\n${zh.chat.undoLatestOnly}`);
    expect(undoConfirmMessage(zh, chatUi("kimi")!.undo!)).toBe(zh.chat.undoConfirm);
    expect(undoConfirmMessage(zh, chatUi("claude")!.undo!)).toBe(zh.chat.undoConfirm);
  });

  it("文案不超过 20 字，不带破折号和分号", () => {
    expect(zh.chat.undoLatestOnly.length).toBeLessThanOrEqual(20);
    for (const word of [zh.chat.undoLatestOnly, en.chat.undoLatestOnly]) expect(word).not.toMatch(/[—;；]|--/);
  });
});
