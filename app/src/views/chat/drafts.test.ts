import { afterEach, describe, expect, it } from "vitest";
import { attachmentOf, loadStoredDrafts, promptWithAttachments, saveStoredDraft } from "./drafts";

afterEach(() => localStorage.clear());

describe("草稿存取", () => {
  it("写入后能读回，空草稿删掉条目", () => {
    saveStoredDraft(3, "写到一半", [attachmentOf("C:\\a\\图.png")]);
    expect(loadStoredDrafts().get(3)).toEqual({ prompt: "写到一半", attachments: [{ path: "C:\\a\\图.png", name: "图.png", image: true }] });
    saveStoredDraft(3, "  ", []);
    expect(loadStoredDrafts().has(3)).toBe(false);
  });

  // 每会话一个 key，越攒越多会撑爆 localStorage：只留最近写的 20 条。
  it("只保留最近 20 条", () => {
    for (let id = 1; id <= 22; id += 1) {
      localStorage.setItem(`meowo-chat-draft:${id}`, JSON.stringify({ prompt: `p${id}`, attachments: [], at: id }));
    }
    saveStoredDraft(99, "最新", []);
    const drafts = loadStoredDrafts();
    expect(drafts.size).toBe(20);
    expect(drafts.has(99)).toBe(true);
    expect(drafts.has(1) || drafts.has(2) || drafts.has(3)).toBe(false);
  });

  it("旧版整表格式迁移成按会话 key，坏条目丢弃", () => {
    localStorage.setItem("meowo-chat-drafts", JSON.stringify({ 5: { prompt: "旧草稿", attachments: [{ path: "x.txt" }, null] }, 6: { nope: 1 } }));
    const drafts = loadStoredDrafts();
    expect(drafts.get(5)).toEqual({ prompt: "旧草稿", attachments: [{ path: "x.txt" }] });
    expect(drafts.has(6)).toBe(false);
    expect(localStorage.getItem("meowo-chat-drafts")).toBeNull();
  });
});

describe("正文拼附件", () => {
  const instruction = (files: string) => `请读取：\n${files}`;
  it("默认把附件写成指令文本，接在正文后", () => {
    expect(promptWithAttachments("看看", [attachmentOf("D:/a.log")], instruction)).toBe("看看\n\n请读取：\n- D:/a.log");
  });
  it("插件声明 @提及时用 @路径，路径含空格或是图片时退回指令文本", () => {
    expect(promptWithAttachments("看看", [attachmentOf("D:/a.log")], instruction, true)).toBe("@D:/a.log 看看");
    expect(promptWithAttachments("看看", [attachmentOf("D:/my dir/a.log")], instruction, true)).toBe("看看\n\n请读取：\n- D:/my dir/a.log");
    expect(promptWithAttachments("看看", [attachmentOf("D:/a.png")], instruction, true)).toBe("看看\n\n请读取：\n- D:/a.png");
  });
});
