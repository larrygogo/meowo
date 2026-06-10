import { describe, expect, it } from "vitest";
import { zh } from "./zh";
import { en } from "./en";
import { resolveLang } from "./index";

// en: Dict 已由编译期保证 key 对齐；这里补运行时校验嵌套 key 集合一致 + resolveLang 行为。
function keys(o: object, prefix = ""): string[] {
  return Object.entries(o).flatMap(([k, v]) =>
    v !== null && typeof v === "object" ? keys(v, `${prefix}${k}.`) : [`${prefix}${k}`],
  );
}

describe("i18n dicts", () => {
  it("zh/en key sets identical", () => {
    expect(keys(en).sort()).toEqual(keys(zh).sort());
  });

  it("function entries share arity", () => {
    const walk = (a: object, b: object) => {
      for (const [k, v] of Object.entries(a)) {
        const w = (b as Record<string, unknown>)[k];
        if (typeof v === "function") expect((w as () => void).length, k).toBe(v.length);
        else if (v !== null && typeof v === "object") walk(v, w as object);
      }
    };
    walk(zh, en);
  });
});

describe("resolveLang", () => {
  it("explicit setting wins over system", () => {
    expect(resolveLang("zh")).toBe("zh");
    expect(resolveLang("en")).toBe("en");
  });

  it("auto falls back to navigator.language", () => {
    // jsdom 默认 en-US
    expect(resolveLang("auto")).toBe("en");
    expect(resolveLang(undefined)).toBe("en");
  });
});
