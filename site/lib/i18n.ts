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

type ThemeDict = {
  eyebrowHome: string;
  headingHome: string;
  subHome: string;
  eyebrowFeat: string;
  headingFeat: string;
  subFeat: string;
  extra: string;
  color: string;
  style: string;
  theme: string;
  flat: string;
  emboss: string;
  dark: string;
  light: string;
  hint: string;
  swatches: Record<string, string>;
};

export type Dict = {
  htmlLang: string;
  nav: NavDict;
  footer: FooterDict;
  cta: CtaDict;
  supported: {
    eyebrow: string;
    heading: string;
    body: string;
    status: string;
  };
  features: Feature[];
  theme: ThemeDict;
  sticker: {
    tabs: { all: string; waiting: string; running: string; archived: string };
    quota5h: string;
    quota7d: string;
    ai: string;
    you: string;
    justNow: string;
    menu: { star: string; note: string; rename: string; archive: string; newSession: string; openDir: string };
  };
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
    tagline: "本地优先的 AI 编程代理桌面工作台。展开是桌面贴纸，收起是电子红绿灯。少切终端，少输命令。",
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
    title: "把多开 AI 编程，收进桌面一角",
    subtitle: "少切终端，少输命令。每个会话的状态、配额与待办，一切尽在计划之中。",
    download: "下载最新版",
    star: "在 GitHub 上 Star",
  },
  supported: {
    eyebrow: "开箱即用",
    heading: "安装、登录、接入，都在 Meowo 里完成",
    body: "不必先去终端找安装命令。直接在应用内安装 Claude Code、Codex、Kimi、Gemini CLI 或 OpenCode，发起登录后自动接入所需 hooks；已经安装的也会自动检测。同一个工具还能保存多个官方账号一键切换，或按模型接入 API 中转。",
    status: "一键安装 · 登录",
  },
  features: [
    { title: "会话看板", body: "每张卡片显示项目名、会话标题、最近一条 AI 输出和连接状态。支持读取 Context 的 AI Agent 还会显示上下文已用百分比。" },
    { title: "待交互与通知", body: "会话等待输入或报错时进入「待交互」，按等待时长排序。开启系统通知后，点通知直接跳到对应会话，同一件事只弹一次。" },
    { title: "点击直达终端 tab", body: "点卡片，直接切到该会话所在的终端标签页。会话已断开时，自动回到原目录并按对应工具的方式续接——无需记命令或会话 ID。" },
    { title: "会话菜单一站集成", body: "右键或点 ⋮：一键新建会话、打开项目目录、加星置顶、写本地便签、改名、归档。常用操作都在这，不用导出切换、也不用敲命令。" },
    { title: "展开贴纸，收起红绿灯", body: "展开时是钉在桌面一角的贴纸；拖到屏幕边缘收起，就缩成一条竖排的电子红绿灯——红黄绿三色一眼看清各会话状态。" },
    { title: "用量与上下文监控", body: "底栏显示 5 小时 / 7 天配额使用比例，越接近上限越偏红；卡片显示会话上下文用量。不焦虑，一切都在计划之中。" },
    { title: "一键安装、登录与接入", body: "无需先在终端配置环境：直接在 Meowo 里安装 AI CLI、发起登录，并自动接入所需 hooks。检测到连接缺失时，也能一键修复。" },
    { title: "多账号 + API 中转", body: "同一个工具保存多个官方账号，一键切换，各自独立登录与会话历史；也支持按模型接入 API 中转，配置期间仍走官方账号。" },
    { title: "按 AI 工具设置代理", body: "设置全局默认代理，也可以为每个 AI 工具单独选择直连、跟随系统或自定义代理，支持 HTTP / SOCKS5 及带认证的地址。" },
    { title: "多风格 · 多配色", body: "7 种贴纸配色、扁平与立体两种风格、深浅主题随系统或手动切换，还能调不透明度与界面密度——挑一套顺眼的摆在桌面上。" },
    { title: "本地优先", body: "会话与设置保存在本机。Meowo 通过本地 SQLite 汇总状态，不把会话内容上传到自己的服务器。" },
  ],
  theme: {
    eyebrowHome: "你的桌面，你说了算",
    headingHome: "多种风格与配色，随手切换",
    subHome: "下面这块就是活的，点点看它怎么变。",
    eyebrowFeat: "外观",
    headingFeat: "多种风格与配色，随手切换",
    subFeat: "7 种贴纸配色、扁平与立体两种风格、深浅主题——点点下面这块试试。",
    extra: "另外还能调整不透明度（配合系统毛玻璃透出桌面）与界面密度。",
    color: "配色",
    style: "风格",
    theme: "明暗",
    flat: "扁平",
    emboss: "立体",
    dark: "深色",
    light: "浅色",
    hint: "7 种配色 · 扁平 / 立体 · 深 / 浅 · 还能调透明度与界面密度，随手换一套。",
    swatches: { neutral: "无色", classic: "经典", slate: "石青", moss: "苔绿", plum: "暮紫", rose: "玫粉", amber: "琥珀" },
  },
  sticker: {
    tabs: { all: "全部", waiting: "待交互", running: "运行中", archived: "已归档" },
    quota5h: "5 小时配额",
    quota7d: "7 天配额",
    ai: "AI",
    you: "你",
    justNow: "刚刚",
    menu: { star: "星标置顶", note: "添加便签", rename: "重命名", archive: "归档", newSession: "新建会话", openDir: "打开项目目录" },
  },
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
    tagline: "A local-first desktop workbench for AI coding agents. A sticker when expanded, an electronic traffic light when collapsed. Fewer terminal switches, fewer commands.",
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
    title: "Tuck your parallel AI coding into a corner of your desktop",
    subtitle: "Fewer terminal switches, fewer commands. Every session's status, quota, and to-dos — all under control.",
    download: "Download latest",
    star: "Star on GitHub",
  },
  supported: {
    eyebrow: "Ready out of the box",
    heading: "Install, sign in, and connect — all inside Meowo",
    body: "No need to hunt for install commands in a terminal. Install Claude Code, Codex, Kimi, Gemini CLI, or OpenCode right in the app, start the login, and the required hooks are wired up automatically; already-installed tools are detected too. Each tool can also keep multiple official accounts to switch between, or connect an API relay per model.",
    status: "Install · Sign in",
  },
  features: [
    { title: "Session board", body: "Each card shows the project name, session title, the latest AI output, and connection status. Agents that expose Context also show how much of the context window is used." },
    { title: "Needs-you & notifications", body: "When a session waits for input or errors out, it moves to “Needs you”, sorted by wait time. Enable system notifications and clicking one jumps straight to that session — the same event only pings once." },
    { title: "Click to jump to the terminal tab", body: "Click a card to switch to the terminal tab that session lives in. If it's disconnected, Meowo returns to its directory and resumes it the tool's way — no commands or session IDs to remember." },
    { title: "Everything in the session menu", body: "Right-click or the ⋮ button: new session, open project directory, star to pin, jot a local note, rename, archive. Common actions live here — no exporting, switching, or typing commands." },
    { title: "Sticker expanded, traffic light collapsed", body: "Expanded, it's a sticker pinned to a corner of your desktop; drag it to a screen edge and it shrinks into a vertical electronic traffic light — red / amber / green tells each session's state at a glance." },
    { title: "Quota & context monitoring", body: "The bottom bar shows 5-hour / 7-day quota usage, redder as it nears the cap; cards show each session's context usage. No anxiety — everything stays under control." },
    { title: "One-click install, sign-in & connect", body: "No terminal setup first: install an AI CLI, start the login, and wire up the required hooks right in Meowo. Missing a connection? One click fixes it." },
    { title: "Multiple accounts + API relay", body: "Keep several official accounts per tool and switch with one click — each with its own login and history; or connect an API relay per model, still using the official account while you configure it." },
    { title: "Per-tool network proxy", body: "Set a global default proxy, or choose direct / follow-system / custom proxy for each AI tool individually. Supports HTTP / SOCKS5 and authenticated addresses." },
    { title: "Styles & colors", body: "7 sticker colors, flat or dimensional styles, and dark/light themes that follow the system or switch by hand — plus opacity and UI density. Pick a look that fits your desktop." },
    { title: "Local-first", body: "Sessions and settings stay on your machine. Meowo aggregates state through a local SQLite database and never uploads session content to its own servers." },
  ],
  theme: {
    eyebrowHome: "Your desktop, your call",
    headingHome: "Styles and colors, switched on a whim",
    subHome: "The block below is live — click around and watch it change.",
    eyebrowFeat: "Appearance",
    headingFeat: "Styles and colors, switched on a whim",
    subFeat: "7 sticker colors, flat or dimensional styles, dark or light — try the live block below.",
    extra: "You can also tune opacity (to let the frosted desktop show through) and UI density.",
    color: "Color",
    style: "Style",
    theme: "Theme",
    flat: "Flat",
    emboss: "3D",
    dark: "Dark",
    light: "Light",
    hint: "7 colors · Flat / 3D · Dark / Light · plus opacity and UI density — swap a whole set anytime.",
    swatches: { neutral: "None", classic: "Classic", slate: "Slate", moss: "Moss", plum: "Plum", rose: "Rose", amber: "Amber" },
  },
  sticker: {
    tabs: { all: "All", waiting: "Needs you", running: "Running", archived: "Archived" },
    quota5h: "5-hr",
    quota7d: "7-day",
    ai: "AI",
    you: "You",
    justNow: "just now",
    menu: { star: "Star to pin", note: "Add note", rename: "Rename", archive: "Archive", newSession: "New session", openDir: "Open project dir" },
  },
  featuresMore: "See all features",
};

const DICTS: Record<Lang, Dict> = { zh: ZH, en: EN };

export function getDict(lang: Lang): Dict {
  return DICTS[lang] ?? ZH;
}
