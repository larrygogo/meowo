import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { AgentExtensionsDto } from "../../generated/contracts/AgentExtensionsDto";

const api = vi.hoisted(() => ({
  listAgentExtensions: vi.fn(),
  revealPathInFileManager: vi.fn(),
}));
vi.mock("../../api", () => api);

import { ExtensionsSection } from "./ExtensionsSection";

const SAMPLE: AgentExtensionsDto[] = [
  {
    provider: "claude",
    displayName: "Claude Code",
    dataDir: "C:\\Users\\me\\.claude",
    groups: [
      {
        kind: "hook",
        source: "C:\\Users\\me\\.claude\\settings.json",
        shared: false,
        items: [
          { name: "Stop", description: null, rel: null, detail: "1", enabled: null, managed: true },
          { name: "SessionStart", description: null, rel: null, detail: "1", enabled: null, managed: true },
          { name: "PreToolUse", description: null, rel: null, detail: "2", enabled: null, managed: false },
        ],
      },
      {
        kind: "skill",
        source: "C:\\Users\\me\\.claude\\skills",
        shared: false,
        items: [
          { name: "lark-doc", description: "飞书云文档：读取和编辑飞书文档内容", rel: "lark-doc/SKILL.md", detail: null, enabled: null, managed: false },
          { name: "lark-base", description: "飞书多维表格：建表、字段、记录、视图", rel: "lark-base/SKILL.md", detail: null, enabled: null, managed: false },
        ],
      },
      {
        kind: "mcp",
        source: "C:\\Users\\me\\.claude.json",
        shared: false,
        items: [{ name: "kimi-cu", description: null, rel: null, detail: "stdio", enabled: null, managed: false }],
      },
      {
        kind: "plugin",
        source: "C:\\Users\\me\\.claude\\plugins",
        shared: false,
        items: [{ name: "rust-analyzer-lsp", description: null, rel: null, detail: "claude-plugins-official · 1.0.0", enabled: false, managed: false }],
      },
      // 空组不显示。
      { kind: "command", source: "C:\\Users\\me\\.claude\\commands", shared: false, items: [] },
    ],
  },
  {
    provider: "codex",
    displayName: "Codex",
    dataDir: "C:\\Users\\me\\.codex",
    groups: [
      {
        kind: "skill",
        source: "C:\\Users\\me\\.agents\\skills",
        shared: true,
        items: [{ name: "lark-doc", description: null, rel: "lark-doc/SKILL.md", detail: null, enabled: null, managed: false }],
      },
    ],
  },
];

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  try { localStorage.clear(); } catch { /* noop */ }
});

describe("设置 › 扩展", () => {
  it("摘要只数用户自己装的，Meowo 的钩子合成一行并标明自带；空组不显示", async () => {
    api.listAgentExtensions.mockResolvedValue(SAMPLE);
    render(<ExtensionsSection />);
    await screen.findByText("lark-doc");
    expect(document.querySelector(".ext-summary")?.textContent).toContain("2 个技能 · 1 个 MCP 服务 · 1 个插件 · 1 个钩子");
    expect(screen.getByText("Meowo 状态上报")).toBeTruthy();
    expect(screen.getByText("挂在 2 个事件上，看板靠它显示状态")).toBeTruthy();
    expect(screen.getByText("PreToolUse")).toBeTruthy();
    expect(screen.getByText("2 条命令")).toBeTruthy();
    expect(screen.queryByText("Stop")).toBeNull();
    expect(screen.getByText("已关闭")).toBeTruthy();
    expect(screen.queryByText("自定义命令")).toBeNull();
    // 组的顺序：技能在钩子前面。
    const captions = [...document.querySelectorAll(".ext-caption > span:first-child")].map((el) => el.textContent);
    expect(captions).toEqual(["技能", "MCP 服务", "插件", "钩子"]);
  });

  it("在文件夹中显示：技能定位到它的 SKILL.md，组标题定位到来源目录本身", async () => {
    api.listAgentExtensions.mockResolvedValue(SAMPLE);
    api.revealPathInFileManager.mockResolvedValue(undefined);
    render(<ExtensionsSection />);
    await screen.findByText("lark-doc");
    fireEvent.click(document.querySelector(".ext-card .ext-icon-btn")!);
    expect(api.revealPathInFileManager).toHaveBeenLastCalledWith("C:\\Users\\me\\.claude\\skills", "lark-doc/SKILL.md");
    fireEvent.click(document.querySelector(".ext-caption .ext-link")!);
    expect(api.revealPathInFileManager).toHaveBeenLastCalledWith("C:\\Users\\me\\.claude", "skills");
  });

  it("记住上次看的 agent；共享目录注明几家共用", async () => {
    localStorage.setItem("meowo-extensions-agent", "codex");
    api.listAgentExtensions.mockResolvedValue(SAMPLE);
    render(<ExtensionsSection />);
    await waitFor(() => expect(document.querySelector(".ext-caption")?.textContent).toContain("几家共用"));
    expect(screen.queryByText("kimi-cu")).toBeNull();
  });

  it("读取失败给重试入口", async () => {
    api.listAgentExtensions.mockRejectedValueOnce(new Error("boom")).mockResolvedValue(SAMPLE);
    render(<ExtensionsSection />);
    fireEvent.click(await screen.findByText("重试"));
    await screen.findByText("lark-doc");
  });
});
