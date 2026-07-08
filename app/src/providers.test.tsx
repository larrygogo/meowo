import { describe, it, expect } from "vitest";
import { PROVIDERS, providerConfig } from "./providers";
import type { ProviderKey } from "./api";
import { zh } from "./i18n/zh";
import { en } from "./i18n/en";

// 期望的 provider key 集合，必须与 Rust 侧 meowo_store::ProviderKey::ALL 保持一致。
// 新增 CLI 的同步点共 4 处：api.ts 的 ProviderKey 联合、providers.tsx 的 PROVIDERS、
// 此 EXPECTED_KEYS、Rust meowo_store::ProviderKey::ALL。类型注解给字面量加单向类型链
// （某元素不再是合法 ProviderKey 时编译报错）；集合完整性仍由下方运行时 toEqual 守护。
const EXPECTED_KEYS: ProviderKey[] = ["claude", "codex", "kimi"];

describe("provider 注册表守护", () => {
  it("PROVIDERS 的 key 集合恰好等于期望集合", () => {
    expect(Object.keys(PROVIDERS).sort()).toEqual(EXPECTED_KEYS);
  });

  it("每个 provider 在 zh/en 都有非空展示名", () => {
    for (const key of EXPECTED_KEYS) {
      const cfg = PROVIDERS[key];
      expect(cfg, `缺少 provider 注册项: ${key}`).toBeTruthy();
      expect(cfg.label(zh).length, `zh 文案为空: ${key}`).toBeGreaterThan(0);
      expect(cfg.label(en).length, `en 文案为空: ${key}`).toBeGreaterThan(0);
    }
  });

  it("未知 provider 回退到 claude 配置", () => {
    expect(providerConfig("__nope__")).toBe(PROVIDERS.claude);
  });
});
