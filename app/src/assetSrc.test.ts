import { describe, expect, it, vi } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative } from "node:path";

const convert = vi.hoisted(() => vi.fn((path: string) => `asset://localhost/${encodeURIComponent(path)}`));
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  convertFileSrc: convert,
}));

import { isRemotePath, safeFileSrc } from "./assetSrc";

const SRC = dirname(fileURLToPath(import.meta.url));

describe("网络共享路径在 convertFileSrc 之前拒掉", () => {
  // 做空第二轮：Tauri 2.12.5 的 asset 协议在判白名单之前就对路径 is_symlink/exists，
  // 一张 `//attacker/s/x.png` 的缩略图就会让 Windows 发起 SMB、可能带出 NTLM 凭据。
  it("与后端 images::is_remote_path 同一套规则", () => {
    for (const remote of [
      "\\\\attacker\\s\\x.png",
      "//attacker/s/x.png",
      "\\/attacker/s/x.png",
      "\\\\?\\UNC\\attacker\\s\\x.png",
      "\\\\.\\pipe\\meowo",
      "\\\\?\\GLOBALROOT\\Device\\Mup\\attacker\\s\\x.png",
      "file://attacker/s/x.png",
      "FILE://attacker/s/x.png",
      "file:////attacker/s/x.png",
      "  //attacker/s/x.png",
    ]) {
      expect(isRemotePath(remote), remote).toBe(true);
    }
    for (const local of [
      "C:\\Users\\me\\.meowo\\images\\tool\\a.png",
      "C:/Users/me/a.png",
      "/home/me/a.png",
      "\\\\?\\C:\\Users\\me\\a.png",
      "file:///C:/Users/me/a.png",
      "relative/a.png",
      "\\single\\a.png",
    ]) {
      expect(isRemotePath(local), local).toBe(false);
    }
  });

  it("safeFileSrc 遇到网络路径不调 convertFileSrc", () => {
    convert.mockClear();
    expect(safeFileSrc("//attacker/s/x.png")).toBeNull();
    expect(safeFileSrc("\\\\attacker\\s\\x.png")).toBeNull();
    expect(convert).not.toHaveBeenCalled();
    expect(safeFileSrc("C:\\a.png")).toBe(`asset://localhost/${encodeURIComponent("C:\\a.png")}`);
  });

  /** 新调用点绕过守卫是静默的（不会有测试变红），只能靠扫描拦：生产代码里只有 assetSrc.ts
   *  能直接用 convertFileSrc；mobile/transport.ts 是给它换实现（手机走 /file），不是调用点。 */
  it("生产代码里只有 assetSrc.ts 直接调用 convertFileSrc", () => {
    const allowed = new Set(["assetSrc.ts", "mobile/transport.ts"]);
    const offenders: string[] = [];
    const walk = (dir: string) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const path = join(dir, entry.name);
        if (entry.isDirectory()) {
          if (!["generated", "test"].includes(entry.name)) walk(path);
          continue;
        }
        if (!/\.(ts|tsx)$/.test(entry.name) || /\.test\.(ts|tsx)$/.test(entry.name) || /\.harness\.tsx$/.test(entry.name)) continue;
        const rel = relative(SRC, path).replace(/\\/g, "/");
        if (allowed.has(rel)) continue;
        const code = readFileSync(path, "utf8").replace(/\/\/.*$|\/\*[\s\S]*?\*\//gm, "");
        if (/\bconvertFileSrc\b/.test(code)) offenders.push(rel);
      }
    };
    walk(SRC);
    expect(offenders).toEqual([]);
  });
});
