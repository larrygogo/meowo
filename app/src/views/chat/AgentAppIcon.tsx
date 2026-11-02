import { agentAssets } from "../../providers";

/// Agent 的「应用图标」：统一外框里摆品牌图标，几家视觉大小一致。
/// - 自带方块 / 位图的（codex、opencode、kimi）铺满外框；
/// - 裸 logomark 里，claude 可套官方珊瑚橙方底（tiled），gemini 收一圈边。
/// 设置页 agent 卡片、新建会话页头部、空会话欢迎面板共用这一份（provider-card-icon 一套样式），
/// 外框尺寸由调用方的 className 定。
export function AgentAppIcon({ provider, className, tiled = false }: {
  provider: string;
  className?: string;
  /** 大尺寸展示（新建页头部、欢迎面板）：currentColor 的裸 logomark 也套品牌色方底，与 codex
   *  这类自带方块的图标并排时大小一致（用户实拍「logo 大小都不一样」）。设置卡片沿用原样。 */
  tiled?: boolean;
}) {
  const assets = agentAssets(provider);
  const tile = assets.needsTile || (tiled && !!assets.tint);
  return (
    <span
      className={"provider-card-icon" + (tile ? " provider-card-icon-tile" : "") + (className ? ` ${className}` : "")}
      data-agent={provider}
      // claude 是 currentColor 绘制的裸 logomark，无方框时得由容器给品牌橙（--cc-claude），
      // 否则会继承文字色变灰。有方框的由 .provider-card-icon-tile 自己给白。
      style={!tile && assets.tint ? { color: `var(${assets.tint})` } : undefined}
      aria-hidden="true"
    >
      <assets.Icon />
    </span>
  );
}
