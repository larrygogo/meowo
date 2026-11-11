import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

// UI 调研 shell #10：工作区路径保留了开头、截掉了结尾，同一 monorepo 下的目录看起来一模一样，也没有 tooltip。
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve()) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { tildePath } from "../paths";
import { WorkspaceDirectory } from "./chat/WorkspaceDirectory";

afterEach(cleanup);

describe("路径显示", () => {
  it("主目录缩写成 ~，斜杠方向照原样", () => {
    expect(tildePath("C:\\Users\\someone\\dev\\app")).toBe("~\\dev\\app");
    expect(tildePath("C:/Users/someone/dev/app")).toBe("~/dev/app");
    expect(tildePath("/Users/me/work")).toBe("~/work");
    expect(tildePath("/home/me")).toBe("~");
    expect(tildePath("D:/src/app")).toBe("D:/src/app");
    expect(tildePath("/opt/users/x")).toBe("/opt/users/x");
  });

  it("工作区页签的路径行缩写主目录、尾部优先截断、悬停看完整路径", () => {
    const cwd = "C:/Users/someone/dev/enterprise/packages/billing";
    render(<WorkspaceDirectory options={[{ value: cwd, label: "billing" }]} onFilter={() => {}} />);
    const sub = screen.getByText("~/dev/enterprise/packages/billing").closest(".chat-ws-dir-sub")!;
    expect(sub.getAttribute("data-tip")).toBe(cwd);
    expect(sub.classList.contains("path-tail")).toBe(true);
  });
});
