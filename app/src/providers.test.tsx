import { describe, it, expect } from "vitest";
import { PROVIDERS, providerConfig } from "./providers";
import { zh } from "./i18n/zh";
import { en } from "./i18n/en";

// 期望的 provider key 集合，必须与 Rust 侧 cc_store::ProviderKey::ALL 保持一致
// （加新 CLI：此处、providers.tsx 的 PROVIDERS、Rust ProviderKey 三处同步）。
const EXPECTED_KEYS = ["claude", "codex", "kimi"];

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
