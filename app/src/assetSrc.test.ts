import { describe, expect, it, vi } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative } from "node:path";

const convert = vi.hoisted(() => vi.fn((path: string) => `asset://localhost/${encodeURIComponent(path)}`));
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  convertFileSrc: convert,
}));

import { isLocalAbsolute, isRemotePath, safeFileSrc } from "./assetSrc";

const SRC = dirname(fileURLToPath(import.meta.url));

/** 前后端共用的用例表（后端 images::tests::local_absolute_whitelist_matches_shared_fixture 跑同一份）。 */
const TABLE = JSON.parse(readFileSync(join(SRC, "test", "fixtures", "local-paths.json"), "utf8")) as {
  cases: { path: string; windows: boolean; unix: boolean }[];
};

describe("不是本地绝对路径就在 convertFileSrc 之前拒掉", () => {
  // 做空第二轮：Tauri 2.12.5 的 asset 协议在判白名单之前就对路径 is_symlink/exists，
  // 一张 `//attacker/s/x.png` 的缩略图就会让 Windows 发起 SMB、可能带出 NTLM 凭据。
  // 做空第三轮：`\??\UNC\host\…` 绕过了枚举坏写法的旧规则，改成只认好写法的白名单。
  it("白名单与后端同一张用例表", () => {
    for (const { path, windows, unix } of TABLE.cases) {
      expect(isLocalAbsolute(path, "windows"), `windows ${JSON.stringify(path)}`).toBe(windows);
      expect(isLocalAbsolute(path, "unix"), `unix ${JSON.stringify(path)}`).toBe(unix);
      // 平台未知（手机页、平台还没探测到）：只会更严，Windows 规则认的都认，NT 前缀一律不认。
      if (!windows && !unix) expect(isLocalAbsolute(path, "unknown"), `unknown ${JSON.stringify(path)}`).toBe(false);
      if (windows) expect(isLocalAbsolute(path, "unknown")).toBe(true);
    }
    for (const nt of ["/??/UNC/host/s/a.png", "/./pipe/x", "/a\\b.png"]) {
      expect(isLocalAbsolute(nt, "unknown"), nt).toBe(false);
    }
    expect(isLocalAbsolute("/home/me/a.png", "unknown")).toBe(true);
  });

  it("做空第三轮的 NT 前缀写法在任何平台都当远程", () => {
    for (const remote of [
      "\\??\\UNC\\attacker\\s\\x.png",
      "\\??\\GLOBALROOT\\Device\\Mup\\attacker\\s\\x.png",
      "\\??/UNC/attacker/s/x.png",
      "\\\\?\\UNC\\attacker\\s\\x.png",
      "//attacker/s/x.png",
      "file://attacker/s/x.png",
      "relative/a.png",
    ]) {
      expect(isRemotePath(remote), remote).toBe(true);
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
