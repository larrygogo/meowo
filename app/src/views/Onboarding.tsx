import { useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useShowWhenReady } from "../useShowWhenReady";
import { languageOptions, useT } from "../i18n";
import { type ThemeMode } from "../api";
import { useAgents } from "../useAgents";
import { openSettingsAt } from "../settingsNav";
import { Segmented } from "./settings/widgets";
import { Dropdown } from "./menu";
import { useSettingsState } from "./settings/state";
import logoUrl from "../../src-tauri/icons/128x128.png";
import { useEscClose } from "../hooks/useEscClose";
import { Icon } from "../icons/Icon";
import { icClose, icContrast, icGlobe } from "../icons/glyphs";

type Dict = ReturnType<typeof useT>;

// 平台判定与真实设置页一致（WKWebView 的 UA 含 "Mac"）。吸边要点仅 Windows 拼接。
const IS_MAC = typeof navigator !== "undefined" && /Mac/i.test(navigator.userAgent);

// ── 每步的「迷你界面示意图」：用 div 复刻真实界面的观感（状态色、侧栏、消息气泡）。

/** 欢迎页：迷你对话窗（侧栏会话列表 + 对话区）。原先画的是贴纸卡片列表，贴纸 2026-11-04/05
 *  已下线，首屏就教用户一个找不到的界面（UI 调研 shell #3）。 */
function HeroChat() {
  return (
    <div className="obm-chat">
      <div className="obm-chat-bar">
        <img src={logoUrl} width={14} height={14} alt="" className="obm-logo" />
        <b>Meowo</b>
      </div>
      <div className="obm-chat-body">
        <div className="obm-chat-side">
          <i className="obm-chat-item is-on"><i className="obm-island-dot is-run" /><i className="obm-island-bar" /></i>
          <i className="obm-chat-item"><i className="obm-island-dot is-wait" /><i className="obm-island-bar" /></i>
          <i className="obm-chat-item"><i className="obm-island-dot is-off" /><i className="obm-island-bar" /></i>
        </div>
        <div className="obm-chat-main">
          <i className="obm-chat-msg is-user" />
          <i className="obm-chat-msg" />
          <i className="obm-chat-msg is-short" />
          <i className="obm-chat-input" />
        </div>
      </div>
    </div>
  );
}

/** Windows 的窗口示意：屏幕顶部的胶囊，下面是点开后的黑色面板。 */
function HeroIsland() {
  return (
    <div className="obm-desktop obm-desktop-island">
      <div className="obm-island">
        <span className="obm-island-pill">
          <i className="obm-island-dot is-run" />
          <i className="obm-island-dot is-wait" />
        </span>
        <span className="obm-island-panel">
          <i className="obm-island-row"><i className="obm-island-dot is-run" /><i className="obm-island-bar" /></i>
          <i className="obm-island-row"><i className="obm-island-dot is-wait" /><i className="obm-island-bar" /></i>
          <i className="obm-island-row"><i className="obm-island-dot is-off" /><i className="obm-island-bar" /></i>
        </span>
      </div>
    </div>
  );
}

// 设置行前置图标：一眼分辨每项在调什么（尤其语言）。lucide 风格、16px、描边。
const SI = {
  lang: (
    <Icon glyph={icGlobe} size={16} />
  ),
  theme: (
    <Icon glyph={icContrast} size={16} />
  ),
};

// 欢迎步就地设置:只留最关键的语言与外观两项(其余项全用默认值,设置页随时可改)。
// 引导从 6 步压到 3 步:配置项与设置页重复的步骤删除,是「操作更简」的一部分。
function WelcomeConfig({ t }: { t: Dict }) {
  const [settings, patch] = useSettingsState();
  const language = settings?.language ?? "auto";
  const theme = (settings?.theme ?? "dark") as ThemeMode;
  return (
    <div className="ob-set">
      <div className="ob-set-row">
        <span className="ob-set-ico">{SI.lang}</span>
        <div className="ob-set-text">
          <div className="ob-set-label">{t.settings.language}</div>
        </div>
        <Dropdown
          value={language}
          options={languageOptions(t)}
          onChange={(v) => patch({ language: v })}
        />
      </div>
      <div className="ob-set-row">
        <span className="ob-set-ico">{SI.theme}</span>
        <div className="ob-set-text">
          <div className="ob-set-label">{t.settings.theme}</div>
        </div>
        <Segmented
          value={theme}
          options={[
            { value: "dark" as const, label: t.settings.themeDark },
            { value: "light" as const, label: t.settings.themeLight },
            { value: "system" as const, label: t.settings.themeSystem },
          ]}
          onChange={(v: ThemeMode) => patch({ theme: v })}
          label={t.settings.theme}
        />
      </div>
    </div>
  );
}

/** 「连接 AI CLI」一步：当场列出各 agent 装没装，未装给「去安装」直达设置「账号与用量」。
 *  原先只有三条文字、每条前面打对勾，一个 CLI 没装也像已完成（UI 调研 shell #4）；新建页早就有
 *  「检测 + 去安装」（7T-8），引导反而落后。装完会收到 install-done，useAgents 自动重取。 */
function AgentCheck({ t }: { t: Dict }) {
  const { agents } = useAgents();
  if (agents === null) return <p className="ob-agents-note">{t.newSession.detectingAgents}</p>;
  const missing = agents.some((a) => !a.installed);
  return (
    <div className="ob-agents">
      <ul className="ob-agent-list">
        {agents.map((a) => (
          <li key={a.id} className={"ob-agent" + (a.installed ? " is-ok" : "")}>
            <span className="ob-agent-name">{a.display_name}</span>
            <span className="ob-agent-state">{a.installed ? t.onboarding.connect.installed : t.onboarding.connect.missing}</span>
          </li>
        ))}
      </ul>
      {missing && (
        <button type="button" className="sbtn" onClick={() => openSettingsAt("account")}>{t.newSession.goInstall}</button>
      )}
    </div>
  );
}

type Step = {
  hero?: ReactNode;
  title: string;
  desc?: string;
  points?: string[];
  body?: ReactNode;
};

/**
 * 使用引导窗口（label "onboarding"）。首次启动由 Rust 侧 setup 自动弹出，之后可从托盘/菜单栏
 * 图标或设置页手动打开。任一方式关闭（完成 / 跳过 / 关闭按钮）都会把 onboarding_seen 落盘，
 * 保证首次弹出只弹一次。既介绍核心用法（每步配迷你界面示意图），也在最后让用户顺手做基础配置。
 */
export function Onboarding() {
  const t = useT();
  // 窗口以 visible:false 创建（window.rs），首帧渲染后再显示，消除打开瞬间的白框闪烁。
  useShowWhenReady();
  const [step, setStep] = useState(0);

  // 4 步:欢迎(含语言/外观两项就地设置) → 连接 AI CLI → Meowo 如何读到进度 → 灵动岛与对话窗口。
  // 其余配置项全用默认值,设置页随时可改——配置型步骤与设置页重复,已删。
  // 「连接 AI CLI」不能省：此前引导只字未提装 agent/登录，用户走完面对一块空看板。
  // 「如何读到进度」同理（S-12）：hooks 改写 CLI 配置曾全程零告知，必须明写备份与移除方式。
  const steps: Step[] = [
    { hero: <HeroChat />, title: t.onboarding.welcome.title, desc: t.onboarding.welcome.desc, body: <WelcomeConfig t={t} /> },
    { title: t.onboarding.connect.title, points: t.onboarding.connect.points, body: <AgentCheck t={t} /> },
    { title: t.onboarding.progress.title, points: t.onboarding.progress.points },
    // 两个平台都是灵动岛（Windows 2026-11-04、macOS 2026-11-05 起替换贴纸），没有卡片了，
    // 「看板与卡片」「窗口与设置」两步合成这一步。macOS 的岛钉在刘海上或藏在菜单栏图标里，要点另一组。
    { hero: <HeroIsland />, title: t.onboarding.window.islandTitle, points: IS_MAC ? t.onboarding.window.macIslandPoints : t.onboarding.window.islandPoints },
  ];
  const total = steps.length;
  const isFirst = step === 0;
  const isLast = step === total - 1;
  const cur = steps[step];

  // 完成/跳过/关闭都走这里：先落盘「已看过」，再关窗口。invoke 失败也照常关，别把用户卡在引导里。
  const dismiss = () => {
    invoke("mark_onboarding_seen").catch(() => {});
    getCurrentWindow().close().catch(() => {});
  };
  // Esc = 跳过（与点 ✕ 同语义，一样落盘「已看过」）。
  useEscClose(dismiss);
  const next = () => (isLast ? dismiss() : setStep((s) => Math.min(s + 1, total - 1)));
  const back = () => setStep((s) => Math.max(s - 1, 0));

  return (
    <div className="onboarding">
      <div className="ob-bar" data-tauri-drag-region>
        {!isLast && (
          <button className="ob-skip" onClick={dismiss}>
            {t.onboarding.skip}
          </button>
        )}
        {/* reopen 提示贴跳过路径：页尾那句只有走完最后一步的人才看得到，跳过者只经过这里。 */}
        {!isLast && <span className="ob-reopen ob-reopen-bar">{t.onboarding.window.reopenHint}</span>}
        <button className="winclose" data-tip={t.settings.close} aria-label={t.settings.close} onClick={dismiss}>
          <Icon glyph={icClose} size={14} />
        </button>
      </div>

      {/* key=step 触发每步淡入，弱化切换的生硬感 */}
      <div className="ob-body" key={step}>
        {cur.hero && <div className="ob-hero">{cur.hero}</div>}
        <h1 className="ob-title">{cur.title}</h1>
        {cur.desc && <p className="ob-desc">{cur.desc}</p>}
        {cur.points && (
          <ul className="ob-points">
            {cur.points.map((p, i) => (
              <li key={i}>
                {/* 说明条目用编号，不用对勾：对勾读起来是「已完成」，而这些是要用户去做或要知道的事（shell #4）。 */}
                <span className="ob-num" aria-hidden>{i + 1}</span>
                <span>{p}</span>
              </li>
            ))}
          </ul>
        )}
        {cur.body}
        {isLast && <p className="ob-reopen">{t.onboarding.window.reopenHint}</p>}
      </div>

      <div className="ob-foot">
        {/* 步骤圆点是导航，不是 tab（无对应 tabpanel）：用 aria-current="step" 表达当前步。 */}
        <div className="ob-dots" role="group" aria-label={t.onboarding.stepsLabel}>
          {steps.map((_, i) => (
            <button
              key={i}
              className={"ob-dot" + (i === step ? " on" : "")}
              aria-label={t.onboarding.stepOf(i + 1, total)}
              aria-current={i === step ? "step" : undefined}
              onClick={() => setStep(i)}
            />
          ))}
        </div>
        <div className="ob-actions">
          {!isFirst && (
            <button className="sbtn" onClick={back}>
              {t.onboarding.back}
            </button>
          )}
          <button className="sbtn primary" onClick={next}>
            {isLast ? t.onboarding.done : t.onboarding.next}
          </button>
        </div>
      </div>
    </div>
  );
}
