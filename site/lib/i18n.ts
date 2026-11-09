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
    tagline: "本地优先的 AI 编程代理桌面工作台。会话在对话窗里跑，状态常驻屏幕顶部，离开电脑手机接着推进。",
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
    title: "多开 AI 编程，从这里开始",
    subtitle: "少切终端，少输命令。会话、审批、改动和额度都在一处，人不在电脑前也能推进。",
    download: "下载最新版",
    star: "在 GitHub 上 Star",
  },
  features: [
    { title: "五家 Agent，一个工作台", body: "Claude Code、Codex、Kimi、Gemini CLI、OpenCode 的会话排在同一个侧栏里，可以按工作区分组、拖着排序。做完的会话收进「已完成」，随时找回。" },
    { title: "托管终端", body: "会话跑在 Meowo 托管的终端里，对话和终端是同一个会话的两种视图。你在自己终端里开的会话也会被收录，断开的会话点一下就在原目录续上。" },
    { title: "进度与子任务", body: "Agent 的待办清单、派出去的子任务、后台还在跑的命令，都在进度面板里列着。主回合停了但后台还在跑，不会误报成「等你输入」。" },
    { title: "审批与提问按钮化", body: "等批准的命令、Agent 的选择题、终端弹出的信任确认和模型菜单，都渲染成按钮。点一下就把对应按键发回终端，不用敲键盘应答。" },
    { title: "多账号 + API 中转", body: "同一个 Agent 保存多个官方账号，一键切换，正在跑的会话也跟着换。没有官方账号时，按模型接入 API 中转。" },
    { title: "本地优先", body: "会话与设置只存在本机 ~/.meowo/ 的 SQLite 里。Meowo 没有自己的服务器，不上传会话内容，断网也照常工作。" },
    { title: "一键安装、登录与接入", body: "五家 CLI 直接在应用内一键安装并发起登录，所需 hooks 自动接好。检测到接入缺失时，再点一下就修好。" },
    { title: "额度与上下文", body: "灵动岛显示各家 5 小时 / 7 天额度，Opus 这类模型单独的周额度也列出来，快用完时变红。每个会话还显示上下文用量。" },
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
    tagline: "A local-first desktop workbench for AI coding agents. Sessions run in the chat window, status stays at the top of your screen, and your phone takes over when you step away.",
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
    title: "Run AI coding in parallel, starting here",
    subtitle: "Fewer terminal switches, fewer commands. Sessions, approvals, changes and quota in one place, even when you're away from your desk.",
    download: "Download latest",
    star: "Star on GitHub",
  },
  features: [
    { title: "Five agents, one workbench", body: "Claude Code, Codex, Kimi, Gemini CLI and OpenCode sessions share one sidebar, grouped by workspace and sorted by drag. Finished sessions move to Done, ready to bring back." },
    { title: "Managed terminal", body: "Sessions run in a terminal hosted by Meowo; chat and terminal are two views of the same session. Sessions you start in your own terminal show up too, and a disconnected one resumes in its folder with one click." },
    { title: "Progress & subagents", body: "The agent's todo list, the subagents it sends out and commands still running in the background all show in the progress panel. A background command keeps the session from being shown as waiting for you." },
    { title: "Approvals & questions as buttons", body: "Pending commands, the agent's multiple-choice questions, and terminal trust prompts and model menus all render as buttons. One click sends the matching keys back to the terminal." },
    { title: "Multiple accounts + API relay", body: "Keep several official accounts per agent and switch in one click; running sessions switch too. No official account? Connect an API relay per model." },
    { title: "Local-first", body: "Sessions and settings live only in ~/.meowo/ as local SQLite. Meowo has no server of its own, uploads no conversations, and works offline." },
    { title: "One-click install, sign-in & connect", body: "Install any of the five CLIs and start the login right in the app; the hooks are wired for you. If a connection breaks, one more click repairs it." },
    { title: "Quota & context", body: "The island shows each agent's 5-hour / 7-day quota, plus per-model weekly quota such as Opus, turning red near the limit. Every session shows its context usage too." },
  ],
  featuresMore: "See all features",
};

const DICTS: Record<Lang, Dict> = { zh: ZH, en: EN };

export function getDict(lang: Lang): Dict {
  return DICTS[lang] ?? ZH;
}
