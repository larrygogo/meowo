// Agent 的**视觉资产**表：图标 + 着色方式。展示名与安装态由后端 list_agents() 下发，不在此处。
//
// 为什么资产留在前端：kimi 的 logo 是位图 PNG（渐变颗粒纹理，矢量化会失真），claude 的品牌橙在
// 浅色/深色主题下取不同明度。位图与主题相关的颜色都无法诚实地塞进后端的一个字符串字段——它们是
// 资产，不是数据。加一个 agent 仍要在这里加一项（总得有人提供图标），但**前端不再有任何 agent
// 的逻辑分支**：未知 id 走中性兜底，不会崩、也不会伪装成 claude。
import type { ReactElement } from "react";
// Kimi 官方位图 logo（渐变颗粒纹理、非矢量友好）：作静态资源随打包分发（Vite 输出带哈希的文件），
// 不再把整张 PNG 以超长 base64 内嵌进源码（增大 bundle/难 diff）。四角本就透明，无需圆角裁剪。
import kimiLogo from "./assets/kimi.png";

function ClaudeMark() {
  // 官方 Claude logomark（赤陶色 sunburst）：fill=currentColor，由容器着色——裸图标场景
  // (.stk-agent/.stk-utab) 给 --cc-claude 品牌橙、断开转灰；橙方块底座 (.provider-card-icon-tile) 给白。
  return (
    <svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z" />
    </svg>
  );
}

function KimiMark() {
  return (
    <img
      src={kimiLogo}
      width={13}
      height={13}
      alt=""
      aria-hidden="true"
      style={{ display: "block" }}
    />
  );
}

function CodexMark() {
  // OpenAI 官方 app 图标风格：黑圆角方块 + 白「六瓣结」logomark。固定品牌色（不随主题/连接态着色，
  // 只靠 .stk-agent-off 的 opacity 变暗），与 Kimi 徽标同款处理——故其 tint 为 undefined。
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" aria-hidden="true">
      <rect x="0.5" y="0.5" width="23" height="23" rx="6.5" fill="#0a0a0c" />
      <g transform="translate(4 4) scale(0.6667)" fill="#fff">
        <path d="M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 0-6.5098-2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 6.5146 2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718-4.2058 5.9894 5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm-9.022 12.6081a4.4755 4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582a.7948.7948 0 0 0 .3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052v5.5826a4.504 4.504 0 0 1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1-.5346-3.0137l.142.0852 4.783 2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685v2.3324a.0804.0804 0 0 1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408-1.6464zM2.3408 7.8956a4.485 4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 .3879.6765l5.8144 3.3543-2.0201 1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865A4.504 4.504 0 0 1 2.3408 7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2a.0757.0757 0 0 1 .071 0l4.8303 2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v-5.6772a.79.79 0 0 0-.407-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818a.7759.7759 0 0 0-.7854 0L9.409 9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 4.66zM8.3065 12.863l-2.02-1.1638a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 0 1 7.3757-3.4537l-.142.0805L8.704 5.459a.7948.7948 0 0 0-.3927.6813zm1.0976-2.3654l2.602-1.4998 2.6069 1.4998v2.9994l-2.5974 1.4997-2.6067-1.4997Z" />
      </g>
    </svg>
  );
}

function GeminiMark() {
  // Gemini 的四角星（sparkle）+ 官方蓝紫渐变。自带固定品牌色，与 kimi/codex 同款处理（tint 为
  // undefined，断开态只靠 .stk-agent-off 的 opacity 变暗）。
  //
  // 渐变 id 必须唯一：同一页面会同时渲染多个徽标，撞 id 会让后挂载的那个引用到前一个的 <defs>。
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" aria-hidden="true">
      <defs>
        <linearGradient id="meowo-gemini-grad" x1="0" y1="24" x2="24" y2="0" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#4285F4" />
          <stop offset="0.5" stopColor="#9B72CB" />
          <stop offset="1" stopColor="#D96570" />
        </linearGradient>
      </defs>
      {/* 缩到 0.82 居中留白：设置页的 .provider-card-icon 会把徽标 svg 拉满外框，而 sparkle 不像
          codex/opencode 那样自带方块底——满幅铺开会比邻座重得多。留白后视觉重量才对得齐。 */}
      <path
        d="M12 24A14.304 14.304 0 0 0 0 12 14.304 14.304 0 0 0 12 0a14.305 14.305 0 0 0 12 12 14.305 14.305 0 0 0-12 12Z"
        fill="url(#meowo-gemini-grad)"
        transform="translate(2.16 2.16) scale(0.82)"
      />
    </svg>
  );
}

function OpencodeMark() {
  // ⚠️ **不是** OpenCode 的官方 logomark——找不到可靠的矢量原件，与其编一个假的品牌标识，不如放一个
  // 明摆着是占位的终端提示符（黑方块 + 白 `>_`）。它可辨识、不冒充，拿到官方资产后直接换掉这个函数
  // 即可，别处无需改动。自带固定色，故 tint 为 undefined。
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" aria-hidden="true">
      <rect x="0.5" y="0.5" width="23" height="23" rx="6.5" fill="#0a0a0c" />
      <g stroke="#fff" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" fill="none">
        <path d="M6.5 8.5 10 12l-3.5 3.5" />
        <path d="M12.5 16h5" />
      </g>
    </svg>
  );
}

/** 未知 agent（DB 里存着本版本不认识的 id）：中性占位方块，绝不伪装成 claude。 */
function UnknownMark() {
  return (
    <svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <rect x="3" y="3" width="18" height="18" rx="4" opacity="0.35" />
    </svg>
  );
}

export type AgentAssets = {
  /** 品牌图标。 */
  Icon: () => ReactElement;
  /**
   * 徽标以 `currentColor` 绘制时的着色变量名（如 `"--cc-claude"`），由容器 `color` 应用；
   * 主题明暗变体由该 CSS 变量自己承担。自带固定品牌色的徽标（kimi/codex）为 undefined。
   */
  tint?: string;
  /**
   * 设置页的 agent 卡片是否需要给徽标补一个品牌色方块底座。裸 logomark（claude）需要；
   * 自带方块/位图的（codex/kimi）不需要。
   */
  needsTile: boolean;
};

const ASSETS: Record<string, AgentAssets> = {
  claude: { Icon: ClaudeMark, tint: "--cc-claude", needsTile: true },
  kimi: { Icon: KimiMark, needsTile: false },
  codex: { Icon: CodexMark, needsTile: false },
  gemini: { Icon: GeminiMark, needsTile: false },
  opencode: { Icon: OpencodeMark, needsTile: false },
};

const UNKNOWN: AgentAssets = { Icon: UnknownMark, needsTile: false };

/**
 * 徽标容器的着色。以 `currentColor` 绘制的徽标（claude）走它自己的 CSS 变量；自带固定品牌色的
 * （kimi/codex）返回空对象，不设 `color`。
 *
 * 此前是 CSS 里一句 `.stk-agent { color: var(--cc-claude) }`——给**所有** agent 的容器抹上 claude
 * 的橙，只因为 kimi/codex 恰好不吃 `color`。任何新 agent 只要用 currentColor 徽标就会被染成橙。
 *
 * 断开态传 `enabled=false`，让位给 `.stk-agent-off` 的灰：inline style 的优先级高于 class。
 */
export function tintStyle(id: string, enabled = true): { color?: string } {
  const { tint } = agentAssets(id);
  return enabled && tint ? { color: `var(${tint})` } : {};
}

/** 取 agent 的视觉资产；未知 id → 中性兜底（不回退成 claude 的图标）。 */
export function agentAssets(id: string): AgentAssets {
  return ASSETS[id] ?? UNKNOWN;
}
