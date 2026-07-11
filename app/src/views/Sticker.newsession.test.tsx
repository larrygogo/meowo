import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invokeMock(...a) }));

import { EmptyState } from "./sticker/EmptyState";

afterEach(() => cleanup());

describe("EmptyState 新建 CTA", () => {
  it("有 onNew 时渲染 CTA 且点击触发", () => {
    const onNew = vi.fn();
    render(<EmptyState tab="all" onNew={onNew} />);
    fireEvent.click(screen.getByTestId("empty-new-cta"));
    expect(onNew).toHaveBeenCalled();
  });

  it("无 onNew 时不渲染 CTA", () => {
    render(<EmptyState tab="all" />);
    expect(screen.queryByTestId("empty-new-cta")).toBeNull();
  });
});
