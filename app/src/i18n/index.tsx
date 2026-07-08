// 轻量 i18n：嵌套字典 + context。语言来源 Settings.language（auto/zh/en，auto 按
// navigator.language 解析）；仿 appearance.ts——localStorage 缓存防首屏闪错语言，
// settings-changed 实时切换并消除 fetch-vs-subscribe 竞态。
import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { getSettings, type Settings } from "../api";
import { zh, type Dict } from "./zh";
import { en } from "./en";

export type Lang = "zh" | "en";

const CACHE_KEY = "meowo-lang";

export function resolveLang(setting: string | undefined): Lang {
  if (setting === "zh" || setting === "en") return setting;
  return /^zh\b|^zh-/i.test(navigator.language) ? "zh" : "en";
}

function readCache(): Lang {
  const c = localStorage.getItem(CACHE_KEY);
  return c === "en" ? "en" : c === "zh" ? "zh" : resolveLang(undefined);
}

const DICTS: Record<Lang, Dict> = { zh, en };
const I18nCtx = createContext<Dict>(zh);

/** 取当前语言字典：const t = useT(); t.tabs.all */
export function useT(): Dict {
  return useContext(I18nCtx);
}

export function I18nProvider({ children, initial }: { children: ReactNode; initial?: Lang }) {
  const [lang, setLang] = useState<Lang>(() => initial ?? readCache());
  useEffect(() => {
    if (initial) return; // 测试注入固定语言时不订阅
    let eventApplied = false;
    // cleanup 可能先于 listen resolve：cancelled 标记保证 resolve 后立即注销，防监听器泄漏。
    let cancelled = false;
    let un: (() => void) | undefined;
    const apply = (s: Settings) => {
      const l = resolveLang(s.language);
      setLang(l);
      try { localStorage.setItem(CACHE_KEY, l); } catch { /* ignore */ }
    };
    try {
      listen<Settings>("settings-changed", (e) => { eventApplied = true; apply(e.payload); })
        .then((f) => {
          if (cancelled) f();
          else un = f;
        })
        .catch(() => {});
    } catch { /* 非 Tauri 环境 */ }
    getSettings().then((s) => { if (!eventApplied) apply(s); }).catch(() => {});
    return () => {
      cancelled = true;
      un?.();
    };
  }, [initial]);
  return <I18nCtx.Provider value={DICTS[lang]}>{children}</I18nCtx.Provider>;
}
