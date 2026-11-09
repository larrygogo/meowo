// 双语（中文默认在根路径 / ，英文在 /en）。静态导出，不用 Next 内置 i18n 路由。
// 页面正文各自内联对应语言；此处集中放导航/页脚/共享组件的字符串与路径助手。

export type Lang = "zh" | "en";
export const LOCALES: Lang[] = ["zh", "en"];
export const DEFAULT_LOCALE: Lang = "zh";

/** 从路径判断语言：/en 或 /en/... 为英文，其余为中文。 */
export function langFromPath(pathname: string): Lang {
  return pathname === "/en" || pathname.startsWith("/en/") ? "en" : "zh";
}

/** 给一个「中文根路径」（如 "/features"、"/"）套上语言前缀。 */
export function withLang(lang: Lang, path: string): string {
  const clean = path === "/" ? "" : path.replace(/\/$/, "");
  if (lang === "en") return `/en${clean}` || "/en";
  return clean || "/";
}

/** 把当前路径切换到目标语言的等价路径（保持子路径）。 */
export function switchLangPath(pathname: string, target: Lang): string {
  const current = langFromPath(pathname);
  if (current === target) return pathname;
  if (target === "en") {
    const rest = pathname === "/" ? "" : pathname.replace(/\/$/, "");
    return `/en${rest}` || "/en";
  }
  // en -> zh：去掉 /en 前缀
  const rest = pathname.replace(/^\/en/, "");
  return rest || "/";
}

type NavDict = {
  links: { path: string; label: string }[];
  download: string;
  github: string;
  menu: string;
  switchTo: string; // 语言切换按钮的 aria/title
};

type FooterDict = {
  tagline: string;
  cols: { title: string; links: { label: string; path?: string; href?: string }[] }[];
  license: string;
  tip: string;
};

type CtaDict = { title: string; subtitle: string; download: string; star: string };

type Feature = { title: string; body: string };


export type Dict = {
  htmlLang: string;
  nav: NavDict;
  footer: FooterDict;
  cta: CtaDict;
  // 与 components/FeatureGrid.tsx 的 ICONS 一一对应，共 8 张卡。
  features: Feature[];
  featuresMore: string; // 首页「查看全部功能」
};

const ZH: Dict = {
  htmlLang: "zh-CN",
  nav: {
    links: [
      { path: "/features", label: "功能" },
      { path: "/download", label: "下载" },
      { path: "/docs", label: "文档" },
      { path: "/changelog", label: "更新日志" },
      { path: "/faq", label: "FAQ" },
    ],
    download: "下载",
    github: "GitHub",
    menu: "菜单",
    switchTo: "English",
  },
  footer: {
    tagline: "同时使用多个 AI 编程 CLI 时用的桌面应用。开源，数据存在本机。",
    cols: [
      {
        title: "产品",
        links: [
          { label: "功能", path: "/features" },
          { label: "下载", path: "/download" },
          { label: "更新日志", path: "/changelog" },
        ],
      },
      {
        title: "资源",
        links: [
          { label: "文档", path: "/docs" },
          { label: "FAQ", path: "/faq" },
          { label: "Releases", href: "releases" },
        ],
      },
      {
        title: "项目",
        links: [
          { label: "GitHub", href: "repo" },
          { label: "License", href: "license" },
        ],
      },
    ],
    license: "MIT © larrygogo",
    tip: "名字来自猫叫 meow，中文译作「喵呜」🐱",
  },
  cta: {
    title: "下载 Meowo",
    subtitle: "支持 Windows 和 macOS，免费开源。AI 编程 CLI 可以装好 Meowo 之后再装。",
    download: "下载最新版",
    star: "在 GitHub 上 Star",
  },
  features: [
    { title: "统一的会话列表", body: "五家 CLI 的会话在同一个侧栏里，可以按工作区分组，也可以拖动调整顺序。处理完的会话标记完成后移到「已完成」，需要时再找回来。" },
    { title: "托管终端", body: "会话运行在 Meowo 托管的终端里，对话窗和终端显示的是同一个会话。你在自己终端里开的会话也会出现在列表里。断开的会话可以在原目录重新接上。" },
    { title: "进度与子任务", body: "进度面板列出 Agent 的待办清单、派出的子任务和仍在后台运行的命令。后台命令没跑完时，会话不会被标成等待输入。" },
    { title: "审批和提问显示成按钮", body: "待批准的命令、Agent 的选择题、终端里的信任确认和模型菜单，都显示成按钮。点击后，Meowo 把对应的按键发给终端。" },
    { title: "多账号与 API 中转", body: "Claude Code、Codex、Kimi 和 OpenCode 可以保存多个官方账号。切换账号时，Claude Code、Codex 和 Kimi 正在运行的会话可以一起切过去。没有官方账号的，可以给这家 Agent 配 API 中转。" },
    { title: "本地优先", body: "会话和设置存在本机 ~/.meowo/ 的 SQLite 数据库里。Meowo 没有自己的服务器，不上传会话内容，断网也能用。开启 Bark 推送是唯一的例外，推送内容会经过 Bark 的服务器。" },
    { title: "安装、登录和接入", body: "五家 CLI 都可以在 Meowo 里安装和登录，Gemini CLI 需要填 API Key。所需的 hooks 由 Meowo 配置，检测到配置缺失时可以在设置里修复。" },
    { title: "额度与上下文", body: "灵动岛显示 Claude Code、Codex 和 Kimi 的 5 小时和 7 天额度，Opus 这类模型单独计算的周额度也会列出。这三家的会话还会显示上下文用量。" },
  ],
  featuresMore: "查看全部功能",
};

const EN: Dict = {
  htmlLang: "en",
  nav: {
    links: [
      { path: "/features", label: "Features" },
      { path: "/download", label: "Download" },
      { path: "/docs", label: "Docs" },
      { path: "/changelog", label: "Changelog" },
      { path: "/faq", label: "FAQ" },
    ],
    download: "Download",
    github: "GitHub",
    menu: "Menu",
    switchTo: "中文",
  },
  footer: {
    tagline: "A desktop app for using several AI coding CLIs at once. Open source, with your data kept on your machine.",
    cols: [
      {
        title: "Product",
        links: [
          { label: "Features", path: "/features" },
          { label: "Download", path: "/download" },
          { label: "Changelog", path: "/changelog" },
        ],
      },
      {
        title: "Resources",
        links: [
          { label: "Docs", path: "/docs" },
          { label: "FAQ", path: "/faq" },
          { label: "Releases", href: "releases" },
        ],
      },
      {
        title: "Project",
        links: [
          { label: "GitHub", href: "repo" },
          { label: "License", href: "license" },
        ],
      },
    ],
    license: "MIT © larrygogo",
    tip: "Named after a cat's “meow”, written 喵呜 in Chinese 🐱",
  },
  cta: {
    title: "Download Meowo",
    subtitle: "Free and open source, for Windows and macOS. You can install the AI coding CLIs after Meowo.",
    download: "Download latest",
    star: "Star on GitHub",
  },
  features: [
    { title: "One session list", body: "Sessions from all five CLIs share one sidebar. Group them by workspace or drag them into your own order. Mark a session done when you're finished and it moves to Done, where you can find it later." },
    { title: "Managed terminal", body: "Sessions run in a terminal managed by Meowo, and the chat window and the terminal show the same session. Sessions you start in your own terminal appear in the list too. A disconnected session can be resumed in its original folder." },
    { title: "Progress and subagents", body: "The progress panel lists the agent's todo list, the subagents it starts and commands still running in the background. While a background command runs, the session isn't marked as waiting for input." },
    { title: "Approvals and questions as buttons", body: "Commands pending approval, the agent's multiple-choice questions, and the terminal's trust prompts and model menus show up as buttons. When you click one, Meowo sends the matching keys to the terminal." },
    { title: "Multiple accounts and API relay", body: "Claude Code, Codex, Kimi and OpenCode can keep several official accounts. When you switch, running Claude Code, Codex and Kimi sessions can switch with you. Without an official account, you can set up an API relay for that agent." },
    { title: "Local-first", body: "Sessions and settings are stored in a SQLite database in ~/.meowo/ on your machine. Meowo has no server of its own, doesn't upload conversations and works offline. Bark push is the one exception: when it's on, pushes go through Bark's server." },
    { title: "Install, sign in, connect", body: "You can install and sign in to all five CLIs from Meowo. Gemini CLI needs an API key. Meowo sets up the hooks it needs, and if something is missing you can repair it in Settings." },
    { title: "Quota and context", body: "The island shows 5-hour and 7-day quota for Claude Code, Codex and Kimi, including separate weekly quota for models such as Opus. Sessions from those three also show context usage." },
  ],
  featuresMore: "See all features",
};

const DICTS: Record<Lang, Dict> = { zh: ZH, en: EN };

export function getDict(lang: Lang): Dict {
  return DICTS[lang] ?? ZH;
}
