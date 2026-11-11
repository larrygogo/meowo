import { describe, expect, it } from "vitest";
import { en } from "../../i18n/en";

describe("设置搜索：导航名改短后旧词仍能命中（UI 优化 #27 副作用）", () => {
  it("英文搜 usage / keyboard 能匹配到账号、快捷键分区的搜索标签", () => {
    const label = (key: "account" | "shortcuts") => `${en.settings.nav[key]} ${en.settings.navSearchExtra[key]}`.toLowerCase();
    expect(label("account")).toContain("usage");
    expect(label("shortcuts")).toContain("keyboard");
  });
});
