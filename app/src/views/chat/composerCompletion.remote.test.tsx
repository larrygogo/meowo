import { act, renderHook, waitFor } from "@testing-library/react";
import { createRef, useState } from "react";
import { describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({
  invoke: vi.fn((command: string) => Promise.resolve(command === "file_mention_candidates" ? ["src/", "src/main.rs"] : null)),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("../../remoteMode", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../remoteMode")>()),
  remoteUi: () => true,
}));

import { useAtFileCompletion } from "./composerCompletion";

describe("useAtFileCompletion（手机端）", () => {
  /** 手机端 `@` 补全只走只回名字的 file_mention_candidates：能列任意目录的 list_dir_entries
   *  与带回文件内容的 search_project_files 是远程拒绝项，绝不能调。 */
  it("走 file_mention_candidates，不碰两个文件浏览命令", async () => {
    const { result } = renderHook(() => {
      const [prompt, setPrompt] = useState("");
      return useAtFileCompletion({ cwd: "C:/repo", prompt, setPrompt, promptInputRef: createRef<HTMLTextAreaElement>() });
    });
    act(() => result.current.setAtQuery("ma"));
    await waitFor(() => expect(result.current.atFiles).toEqual(["src/", "src/main.rs"]));
    const commands = invoke.mock.calls.map((call) => call[0]);
    expect(commands).toContain("file_mention_candidates");
    expect(invoke.mock.calls.find((call) => call[0] === "file_mention_candidates")?.[1]).toEqual({ cwd: "C:/repo", query: "ma" });
    expect(commands).not.toContain("search_project_files");
    expect(commands).not.toContain("list_dir_entries");
  });
});
