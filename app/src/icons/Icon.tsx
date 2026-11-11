import type { ReactNode, SVGProps } from "react";

/** 一个图形：24 网格上的形状（只写形状，不写描边粗细）。`fill` 为实心图形（圆点、停止方块），不描边。 */
export interface Glyph {
  readonly body: ReactNode;
  readonly fill?: boolean;
}

/** 只开四档：调研 sys5 实测原先 9～22px 共 12 档，同一行里 13/14/15px 混排，看着参差。 */
export type IconSize = 12 | 14 | 16 | 20;

/** 屏幕上的有效线宽（CSS px）。原先 strokeWidth 1.3～3.4 × 9～22px 折算成 0.9～1.83px 共 24 档
 *  （UI 审计 sys5），细的发虚、粗的发闷；取最常见两档 1.2（16px × 1.8）与 1.33（16px × 2）之间。 */
export const ICON_STROKE_PX = 1.25;

type IconProps = Omit<SVGProps<SVGSVGElement>, "children" | "width" | "height" | "strokeWidth" | "viewBox"> & {
  glyph: Glyph;
  size: IconSize;
  /** 图标独立表达意思（旁边没有文字、按钮也没有 aria-label）时给它读屏名；不给就按装饰处理。 */
  label?: string;
};

/**
 * 全应用唯一的线性图标出口。
 *
 * 线宽按尺寸反算（strokeWidth = 1.25 × 24 / size），没用 vector-effect: non-scaling-stroke：
 * 后者按屏幕坐标锁线宽，图标一旦被 transform: scale 放大（灵动岛展开、按下回弹），形状变大线却不变，
 * 比例走样；反算的线宽和形状一起缩放，任何缩放下粗细比例都一致，测量也只看 strokeWidth × 尺寸 / 24。
 *
 * 尺寸同时写进 style：好几处旧 CSS 用 `.xxx svg { width: 15px }` 改尺寸，那会让实际尺寸和反算线宽的
 * 依据对不上；行内 style 优先级更高，`size` 就是唯一真源。不许 flex 压缩：窄窗下输入框模式钮里的
 * 12px 图标曾被挤成 10.94px（渲染测量 panel-changes w620）。
 */
export function Icon({ glyph, size, label, className, style, ...rest }: IconProps) {
  const fill = glyph.fill === true;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={fill ? "currentColor" : "none"}
      stroke={fill ? undefined : "currentColor"}
      strokeWidth={fill ? undefined : (ICON_STROKE_PX * 24) / size}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className ? `icon ${className}` : "icon"}
      style={{ width: size, height: size, flexShrink: 0, ...style }}
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      {...rest}
    >
      {glyph.body}
    </svg>
  );
}
