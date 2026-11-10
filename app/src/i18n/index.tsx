// 轻量 i18n：嵌套字典 + context。语言来源 Settings.language（auto/zh/en，auto 按
// navigator.language 解析）；仿 appearance.ts——localStorage 缓存防首屏闪错语言，
// settings-changed 实时切换并消除 fetch-vs-subscribe 竞态。
//
// 字典按需加载：zh 是基准（Dict = typeof zh、Provider 外 useT 的默认值、测试里没包 Provider
// 的组件都靠它）所以同步带；en 动态 import，只有界面是英文时才取。两份同步打包时约 150KB
// 压在每个窗口的入口 chunk 里（构建实测 en 86KB / zh 64KB 未压缩）。入口在首次渲染前
// await bootI18n()，Provider 挂载时字典已在手；万一没等（demo、测试），Provider 先不渲染
// 子树，绝不先画一帧另一种语言或 key。
import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { getSettings, type Settings, type RepairReason } from "../api";
import { remoteUi, REMOTE_SETTINGS_EVENT } from "../remoteMode";
import { zh, type Dict } from "./zh";

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

const DICTS: Partial<Record<Lang, Dict>> = { zh };
let enLoading: Promise<Dict> | null = null;

/** 取某种语言的字典（已加载则同步命中缓存）。en 加载失败时退回 zh：宁可显示中文，也不白屏。 */
export function loadDict(lang: Lang): Promise<Dict> {
  const hit = DICTS[lang];
  if (hit) return Promise.resolve(hit);
  enLoading ??= import("./en").then(
    (mod) => (DICTS.en = mod.en),
    (error) => {
      enLoading = null;
      console.error("加载英文字典失败", error);
      return zh;
    },
  );
  return enLoading;
}

/** 已加载的字典；没加载过返回 null。 */
export function loadedDict(lang: Lang): Dict | null {
  return DICTS[lang] ?? null;
}

/** 各窗口入口在首次渲染前调用：把缓存里记的语言先取到手，首帧即是对的语言。 */
export function bootI18n(): Promise<void> {
  let lang: Lang;
  try { lang = readCache(); } catch { lang = resolveLang(undefined); }
  return loadDict(lang).then(() => {});
}

const I18nCtx = createContext<Dict>(zh);

/** 取当前语言字典：const t = useT(); t.tabs.all */
export function useT(): Dict {
  return useContext(I18nCtx);
}

/** 语言选择器的选项表（设置页与引导页共用一份）。「中文 / English」是语言自称，刻意不翻译。 */
export function languageOptions(t: Dict): { value: "auto" | "zh" | "en"; label: string }[] {
  return [
    { value: "auto", label: t.settings.langAuto },
    { value: "zh", label: "中文" },
    { value: "en", label: "English" },
  ];
}

/** 「修复连接」失败原因 → 本地化提示。reason=null 但仍失败（罕见边界）落到泛化文案。 */
export function repairFailMessage(t: Dict, reason: RepairReason | null): string {
  switch (reason) {
    case "need-login":
      return t.newSession.repairNeedLogin;
    case "reporter-not-found":
      return t.newSession.repairNoReporter;
    case "not-detected":
      return t.newSession.repairNotDetected;
    default:
      return t.newSession.repairFailed;
  }
}

export function I18nProvider({ children, initial }: { children: ReactNode; initial?: Lang }) {
  // 只有字典到手的语言才进 state：切语言时先加载、到位再换，旧语言一直显示到那一刻。
  const [shown, setShown] = useState<{ lang: Lang; dict: Dict } | null>(() => {
    const lang = initial ?? readCache();
    const dict = loadedDict(lang);
    return dict ? { lang, dict } : null;
  });
  // 最后一次要求的语言：连着切两次时，先发起、后返回的那次不能把后一次盖掉。
  const wanted = useRef<Lang>(shown?.lang ?? initial ?? readCache());
  const setLang = (lang: Lang) => {
    wanted.current = lang;
    const dict = loadedDict(lang);
    if (dict) {
      setShown((prev) => (prev?.lang === lang && prev.dict === dict ? prev : { lang, dict }));
      return;
    }
    void loadDict(lang).then((loaded) => {
      if (wanted.current === lang) setShown({ lang, dict: loaded });
    });
  };
  const setLangRef = useRef(setLang);
  setLangRef.current = setLang;
  // 入口没等 bootI18n（demo、测试）时，在这里补取首个字典。
  useEffect(() => {
    if (!shown) setLangRef.current(wanted.current);
  // 只在挂载时补一次；之后的切换走下面的订阅。
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const lang = shown?.lang;
  // <html lang> 跟随当前语言（屏幕阅读器朗读、字体回退、连字符断行都依赖它）。
  useEffect(() => {
    if (lang) document.documentElement.lang = lang;
  }, [lang]);
  useEffect(() => {
    if (initial) return; // 测试注入固定语言时不订阅
    let eventApplied = false;
    // cleanup 可能先于 listen resolve：cancelled 标记保证 resolve 后立即注销，防监听器泄漏。
    let cancelled = false;
    let un: (() => void) | undefined;
    const apply = (s: Settings) => {
      const l = resolveLang(s.language);
      setLangRef.current(l);
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
    // 远程收不到 Tauri 事件:mobile 入口轮询派发的 DOM 事件同源切语言(桌面永不派发)。
    const onRemote = (e: Event) => {
      eventApplied = true;
      apply((e as CustomEvent).detail as Settings);
    };
    if (remoteUi()) window.addEventListener(REMOTE_SETTINGS_EVENT, onRemote);
    getSettings().then((s) => { if (!cancelled && !eventApplied) apply(s); }).catch(() => {});
    return () => {
      cancelled = true;
      un?.();
      if (remoteUi()) window.removeEventListener(REMOTE_SETTINGS_EVENT, onRemote);
    };
  }, [initial]);
  if (!shown) return null;
  return <I18nCtx.Provider value={shown.dict}>{children}</I18nCtx.Provider>;
}
