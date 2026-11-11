import { describe, it, expect, vi, beforeEach } from "vitest";

describe("bootWindowMaterial", () => {
  beforeEach(() => {
    vi.resetModules();
    document.documentElement.classList.remove("mat-on");
  });

  async function boot(invoke: ReturnType<typeof vi.fn>) {
    vi.doMock("@tauri-apps/api/core", () => ({ invoke }));
    const { bootWindowMaterial } = await import("./windowMaterial");
    return bootWindowMaterial();
  }

  it.each(["mica", "glass", "vibrancy"])("后端回 %s 时根元素加 mat-on", async (material) => {
    const invoke = vi.fn().mockResolvedValue(material);
    expect(await boot(invoke)).toBe(material);
    expect(invoke).toHaveBeenCalledWith("window_material");
    expect(document.documentElement.classList.contains("mat-on")).toBe(true);
  });

  it("后端回 none 时不加", async () => {
    expect(await boot(vi.fn().mockResolvedValue("none"))).toBe("none");
    expect(document.documentElement.classList.contains("mat-on")).toBe(false);
  });

  it("命令不存在或回了不认识的值都当没有材质", async () => {
    expect(await boot(vi.fn().mockRejectedValue(new Error("unknown command")))).toBe("none");
    expect(document.documentElement.classList.contains("mat-on")).toBe(false);
    vi.resetModules();
    expect(await boot(vi.fn().mockResolvedValue("acrylic"))).toBe("none");
    expect(document.documentElement.classList.contains("mat-on")).toBe(false);
  });
});
