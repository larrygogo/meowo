//! macOS 菜单栏图标的动画帧（用户 2026-11-05 要「灵动一些」）。
//!
//! 图标就是 Meowo 的三道竖条 logo，单色模板图（系统按菜单栏深浅自动反色，和旁边的系统图标一个风格）：
//! - 没事：三道竖条静止，和 logo 一样高低。
//! - 有会话在跑：竖条像均衡器一样此起彼伏。
//! - 有会话要你处理（审批、提问、卡住）：竖条上方一个小圆点一明一暗；数量由菜单栏标题文字显示。
//!
//! 原来是彩色计数徽章（绿运行、黄待交互），静止不动，看不出「在跑」。
//! 纯函数、不碰 AppKit，放在 macos 模块外，Windows 开发机上也能跑单测。

/// 画布边长（像素，菜单栏图标 18pt @2x）。
pub(crate) const SIZE: usize = 36;

/// 三道竖条的中心 x 与静止高度，照 icons/menubar-template.rgba 的 logo 量出来的。
const BARS: [(f32, f32); 3] = [(7.0, 20.0), (18.0, 14.0), (29.0, 24.0)];
const BAR_W: f32 = 8.0;
/// 竖条底边（像素行）。
const BOTTOM: f32 = 30.0;
/// 跳动时的最矮、最高。要你处理时圆点占了上方，竖条只跳到 18，免得顶到圆点。
const MIN_H: f32 = 8.0;
const MAX_H: f32 = 26.0;
const MAX_H_WITH_DOT: f32 = 18.0;
/// 每道竖条各自的频率与相位：三道错开，看起来才像均衡器而不是一起上下。
const SPEED: [f32; 3] = [5.3, 7.1, 6.2];
const PHASE: [f32; 3] = [0.0, 1.9, 3.7];
/// 提醒圆点：圆心、半径、闪烁频率。
const DOT: (f32, f32, f32) = (18.0, 6.0, 3.5);
const DOT_SPEED: f32 = 4.5;

/// 竖条（上下都是半圆头）的覆盖度，SDF 抗锯齿，取各图形覆盖度的最大值。
fn capsule(alpha: &mut [f32], cx: f32, top: f32, bottom: f32, r: f32) {
    let (y0, y1) = (top + r, (bottom - r).max(top + r));
    for y in 0..SIZE {
        for x in 0..SIZE {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let dy = if py < y0 { y0 - py } else if py > y1 { py - y1 } else { 0.0 };
            let d = ((px - cx).powi(2) + dy * dy).sqrt() - r;
            let a = (0.5 - d).clamp(0.0, 1.0);
            let i = y * SIZE + x;
            alpha[i] = alpha[i].max(a);
        }
    }
}

fn disc(alpha: &mut [f32], cx: f32, cy: f32, r: f32, strength: f32) {
    for y in 0..SIZE {
        for x in 0..SIZE {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt() - r;
            let a = (0.5 - d).clamp(0.0, 1.0) * strength;
            let i = y * SIZE + x;
            alpha[i] = alpha[i].max(a);
        }
    }
}

/// 某一时刻（秒）的一帧，黑色 + alpha 的 RGBA（模板图只看 alpha）。
pub(crate) fn render(t: f32, running: bool, attention: bool) -> Vec<u8> {
    let mut alpha = vec![0f32; SIZE * SIZE];
    let max_h = if attention { MAX_H_WITH_DOT } else { MAX_H };
    for (i, &(cx, rest)) in BARS.iter().enumerate() {
        let h = if running {
            MIN_H + (max_h - MIN_H) * (0.5 + 0.5 * (t * SPEED[i] + PHASE[i]).sin())
        } else {
            rest.min(max_h)
        };
        capsule(&mut alpha, cx, BOTTOM - h, BOTTOM, BAR_W / 2.0);
    }
    if attention {
        let strength = 0.35 + 0.65 * (0.5 + 0.5 * (t * DOT_SPEED).sin());
        disc(&mut alpha, DOT.0, DOT.1, DOT.2, strength);
    }
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    for (i, a) in alpha.iter().enumerate() {
        rgba[i * 4 + 3] = (a * 255.0).round() as u8;
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_at(rgba: &[u8], x: usize, y: usize) -> u8 {
        rgba[(y * SIZE + x) * 4 + 3]
    }

    #[test]
    fn idle_frame_matches_the_logo_and_does_not_move() {
        let a = render(0.0, false, false);
        let b = render(3.7, false, false);
        assert_eq!(a, b);
        // 右边那道最高（24），左边次之（20），中间最矮（14）：顶边以上透明、以下实心。
        assert_eq!(alpha_at(&a, 29, 8), 255);
        assert_eq!(alpha_at(&a, 18, 14), 0);
        assert_eq!(alpha_at(&a, 18, 20), 255);
    }

    #[test]
    fn running_bars_move_over_time() {
        assert_ne!(render(0.0, true, false), render(0.3, true, false));
    }

    #[test]
    fn attention_dot_blinks_and_bars_stay_below_it() {
        let a = render(0.0, true, true);
        let b = render(0.7, true, true);
        assert_ne!(alpha_at(&a, 18, 6), alpha_at(&b, 18, 6));
        for t in [0.0, 0.2, 0.5, 1.1, 2.3] {
            let f = render(t, true, true);
            // 圆点和竖条之间留着空：竖条最高到第 12 行。
            for x in 0..SIZE {
                assert_eq!(alpha_at(&f, x, 11), 0, "t={t} x={x}");
            }
        }
    }
}
