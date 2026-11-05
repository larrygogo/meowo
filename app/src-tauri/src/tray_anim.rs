//! macOS 菜单栏图标：一双会动的眼睛（用户 2026-11-05 要「灵动一些」「可以有眼睛」）。
//!
//! 和灵动岛上的眼睛同一套神态，单色模板图（系统按菜单栏深浅自动反色，和旁边的系统图标一个风格）：
//! 眼睛是「眼眶 + 眼珠」：空心椭圆里一颗实心眼珠。只画两道实心竖条的话，乍看是个暂停键。
//! - 空闲：睁着，每隔几秒眨一下。
//! - 在跑：眼珠在眼眶里左右转，像在干活。
//! - 要你处理（审批、提问）：睁大，眨得勤。
//! - 跑完等你：笑眼（两道向上弯的弧）。
//! - 出错：两个叉。
//!
//! 眼睛右边的运行数、待处理数、额度由菜单栏标题文字显示（menubar.rs）。
//! 纯函数、不碰 AppKit，放在 macos 模块外，Windows 开发机上也能跑单测。

/// 画布宽高（像素，菜单栏图标高 18pt @2x）。
pub(crate) const WIDTH: usize = 32;
pub(crate) const HEIGHT: usize = 36;

/// 两只眼睛的中心 x、竖直中心。
const EYES_X: [f32; 2] = [8.5, 23.5];
const EYE_CY: f32 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mood {
    Idle,
    Running,
    Pending,
    Waiting,
    Error,
}

impl Mood {
    /// 前端传来的整体状态（Island.tsx 的 islandTone）。认不出的按空闲。
    pub(crate) fn parse(tone: &str) -> Mood {
        match tone {
            "running" => Mood::Running,
            "pending" => Mood::Pending,
            "waiting" => Mood::Waiting,
            "error" => Mood::Error,
            _ => Mood::Idle,
        }
    }
}

struct Canvas(Vec<f32>);

impl Canvas {
    fn new() -> Self {
        Canvas(vec![0.0; WIDTH * HEIGHT])
    }

    /// 按像素中心到图形的有符号距离画（SDF 抗锯齿），各图形取覆盖度最大值。
    fn paint(&mut self, dist: impl Fn(f32, f32) -> f32) {
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let a = (0.5 - dist(x as f32 + 0.5, y as f32 + 0.5)).clamp(0.0, 1.0);
                let i = y * WIDTH + x;
                self.0[i] = self.0[i].max(a);
            }
        }
    }

    /// 眼眶：竖着的椭圆（圆角矩形）描边，宽 w、高 h、线宽 stroke。
    fn ring(&mut self, cx: f32, cy: f32, w: f32, h: f32, stroke: f32) {
        let r = w.min(h) / 2.0;
        self.paint(|px, py| {
            let dx = ((px - cx).abs() - (w / 2.0 - r)).max(0.0);
            let dy = ((py - cy).abs() - (h / 2.0 - r)).max(0.0);
            ((dx * dx + dy * dy).sqrt() - r).abs() - stroke / 2.0
        });
    }

    /// 眼珠。
    fn disc(&mut self, cx: f32, cy: f32, r: f32) {
        self.paint(|px, py| ((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - r);
    }

    /// 粗线段（叉、笑眼的两笔）。
    fn line(&mut self, (x0, y0): (f32, f32), (x1, y1): (f32, f32), half: f32) {
        self.paint(|px, py| {
            let (vx, vy) = (x1 - x0, y1 - y0);
            let t = (((px - x0) * vx + (py - y0) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
            let (qx, qy) = (x0 + vx * t - px, y0 + vy * t - py);
            (qx * qx + qy * qy).sqrt() - half
        });
    }

    fn rgba(&self) -> Vec<u8> {
        let mut out = vec![0u8; WIDTH * HEIGHT * 4];
        for (i, a) in self.0.iter().enumerate() {
            out[i * 4 + 3] = (a * 255.0).round() as u8;
        }
        out
    }
}

/// 眨眼的开合度：每 `period` 秒眨一次，持续 0.16 秒，其余时间全开（1.0）。
fn blink(t: f32, period: f32) -> f32 {
    let phase = t % period;
    let dur = 0.16;
    if phase < dur {
        // 先闭后开：0 → 中点闭到 0.1 → 回到 1。
        let k = (phase / dur * 2.0 - 1.0).abs();
        0.1 + 0.9 * k
    } else {
        1.0
    }
}

/// 某一时刻（秒）的一帧，黑色 + alpha 的 RGBA（模板图只看 alpha）。
pub(crate) fn render(t: f32, mood: Mood) -> Vec<u8> {
    let mut c = Canvas::new();
    match mood {
        Mood::Error => {
            for cx in EYES_X {
                c.line((cx - 4.0, EYE_CY - 4.0), (cx + 4.0, EYE_CY + 4.0), 1.6);
                c.line((cx - 4.0, EYE_CY + 4.0), (cx + 4.0, EYE_CY - 4.0), 1.6);
            }
        }
        Mood::Waiting => {
            // 笑眼：「^」两笔。
            for cx in EYES_X {
                c.line((cx - 4.5, EYE_CY + 2.5), (cx, EYE_CY - 2.5), 1.6);
                c.line((cx, EYE_CY - 2.5), (cx + 4.5, EYE_CY + 2.5), 1.6);
            }
        }
        Mood::Idle | Mood::Running | Mood::Pending => {
            // 要你处理：眼眶睁大、眼珠缩小（吃惊），眨得勤。
            let (w, h, pupil, period) = match mood {
                Mood::Pending => (12.0, 17.0, 2.2, 1.6),
                _ => (11.0, 14.0, 2.8, 4.0),
            };
            // 在跑：眼珠左右转（±1.8 像素，约 1.8 秒一个来回）。
            let dx = if mood == Mood::Running { 1.8 * (t * 3.5).sin() } else { 0.0 };
            let open = blink(t, period);
            for cx in EYES_X {
                if open < 0.4 {
                    // 眨眼的那一下：闭成一道横线。
                    c.line((cx - w / 2.0 + 1.0, EYE_CY), (cx + w / 2.0 - 1.0, EYE_CY), 1.0);
                } else {
                    c.ring(cx, EYE_CY, w, h * open, 1.8);
                    c.disc(cx + dx, EYE_CY, pupil);
                }
            }
        }
    }
    c.rgba()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_at(rgba: &[u8], x: usize, y: usize) -> u8 {
        rgba[(y * WIDTH + x) * 4 + 3]
    }

    #[test]
    fn idle_eyes_are_open_then_blink() {
        // 0.5 秒：睁着，眼珠在正中（实心），眼眶顶边（第 11 行）有线，眼珠和眼眶之间是空的。
        let open = render(0.5, Mood::Idle);
        assert_eq!(alpha_at(&open, 8, 17), 255);
        assert!(alpha_at(&open, 8, 11) > 200);
        assert_eq!(alpha_at(&open, 8, 13), 0);
        // 眨眼的中点：闭成一道横线，眼眶顶边那里透明。
        let shut = render(4.0 + 0.08, Mood::Idle);
        assert_eq!(alpha_at(&shut, 8, 11), 0);
        assert_eq!(alpha_at(&shut, 8, 17), 255);
    }

    #[test]
    fn running_eyes_look_around() {
        assert_ne!(render(0.5, Mood::Running), render(1.0, Mood::Running));
    }

    #[test]
    fn each_mood_draws_something_different() {
        let frames: Vec<Vec<u8>> = [Mood::Idle, Mood::Pending, Mood::Waiting, Mood::Error]
            .into_iter()
            .map(|m| render(0.5, m))
            .collect();
        for (i, a) in frames.iter().enumerate() {
            assert!(a.iter().skip(3).step_by(4).any(|&v| v > 0), "mood {i} 空白");
            for b in frames.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn parse_falls_back_to_idle() {
        assert_eq!(Mood::parse("running"), Mood::Running);
        assert_eq!(Mood::parse("whatever"), Mood::Idle);
    }
}
