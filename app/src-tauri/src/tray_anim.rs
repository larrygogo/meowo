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

/// 表情：灵动岛 Eyes 算出来的那个（islandParts.tsx 的 EyeMode），由前端连同 tone 一起推来，
/// 菜单栏和岛上就是同一个神态。`Auto` = 没给或认不出，按 tone 的老画法（Mood）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Face {
    Auto,
    Open,
    Dead,
    Frustrated,
    Wide,
    Plead,
    Dizzy,
    Star,
    Surprised,
    Blush,
    Yawn,
    Happy,
    Worried,
    Focus,
    Sleep,
    Drowsy,
}

impl Face {
    pub(crate) fn parse(mode: Option<&str>) -> Face {
        match mode.unwrap_or("") {
            "open" => Face::Open,
            "dead" => Face::Dead,
            "frustrated" => Face::Frustrated,
            "wide" => Face::Wide,
            "plead" => Face::Plead,
            "dizzy" => Face::Dizzy,
            "star" => Face::Star,
            "surprised" => Face::Surprised,
            "blush" => Face::Blush,
            "yawn" => Face::Yawn,
            "happy" => Face::Happy,
            "worried" => Face::Worried,
            "focus" => Face::Focus,
            "sleep" => Face::Sleep,
            "drowsy" => Face::Drowsy,
            _ => Face::Auto,
        }
    }
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

/// 眼睛的颜色：在跑绿、要你处理琥珀、出错红（取值与灵动岛的 --isl-ok / --isl-pending / --isl-err 相同，改色两边一起改）；空闲和跑完等你不上色，
/// 用模板图跟菜单栏深浅色。
pub(crate) fn tint(mood: Mood) -> Option<[u8; 3]> {
    match mood {
        Mood::Running => Some([0x2f, 0xcf, 0xa0]),
        Mood::Pending => Some([0xff, 0xa1, 0x26]),
        Mood::Error => Some([0xff, 0x5c, 0x57]),
        Mood::Idle | Mood::Waiting => None,
    }
}

/// 带颜色的一帧：tint 为 None 时是黑色 + alpha（模板图），否则按 tint 上色。
pub(crate) fn render_rgba(t: f32, mood: Mood, face: Face, tint: Option<[u8; 3]>) -> Vec<u8> {
    let mut rgba = render_face(t, mood, face);
    if let Some([r, g, b]) = tint {
        for px in rgba.chunks_exact_mut(4) {
            px[0] = r;
            px[1] = g;
            px[2] = b;
        }
    }
    rgba
}

/// 按表情画一帧。`Auto` 走 tone 的老画法（render）；其余各画各的，眼睛位置、笔画粗细与老画法一致。
pub(crate) fn render_face(t: f32, mood: Mood, face: Face) -> Vec<u8> {
    let mut c = Canvas::new();
    // 睁着的一双眼：眼眶 w×h，眼珠半径 pupil，眼珠偏移 (dx, dy)，眼睛整体下移 oy。
    let open_eyes = |c: &mut Canvas, w: f32, h: f32, pupil: f32, dx: f32, dy: f32, oy: f32| {
        for cx in EYES_X {
            c.ring(cx, EYE_CY + oy, w, h, 1.8);
            c.disc(cx + dx, EYE_CY + oy + dy, pupil);
        }
    };
    match face {
        // 「普通睁眼」是岛上在跑（会话不多）和空闲醒着时的默认神态，前端现在总会传表情，这里要是
        // 自己画固定眼珠，菜单栏就丢了在跑时眼珠左右扫的动画，所以和 Auto 一样回落到按 tone 的老画法。
        Face::Auto | Face::Open => return render(t, mood),
        Face::Dead => {
            for cx in EYES_X {
                c.line((cx - 4.0, EYE_CY - 4.0), (cx + 4.0, EYE_CY + 4.0), 1.6);
                c.line((cx - 4.0, EYE_CY + 4.0), (cx + 4.0, EYE_CY - 4.0), 1.6);
            }
        }
        Face::Frustrated => {
            // 「> <」挤着眼，抓狂地抖。
            let shake = 0.8 * (t * 40.0).sin();
            let (l, r) = (EYES_X[0] + shake, EYES_X[1] + shake);
            c.line((l - 4.0, EYE_CY - 4.0), (l + 3.5, EYE_CY), 1.6);
            c.line((l + 3.5, EYE_CY), (l - 4.0, EYE_CY + 4.0), 1.6);
            c.line((r + 4.0, EYE_CY - 4.0), (r - 3.5, EYE_CY), 1.6);
            c.line((r - 3.5, EYE_CY), (r + 4.0, EYE_CY + 4.0), 1.6);
        }
        Face::Wide | Face::Surprised => {
            // 吃惊：眼眶撑大、眼珠缩小；待处理时还眨得勤（同老画法），吓一跳的那下不眨。
            let open = if face == Face::Wide {
                blink(t, 1.6)
            } else {
                1.0
            };
            for cx in EYES_X {
                c.ring(cx, EYE_CY, 12.0, 17.0 * open.max(0.4), 1.8);
                c.disc(cx, EYE_CY, if face == Face::Surprised { 1.6 } else { 2.2 });
            }
        }
        Face::Plead => {
            // 可怜巴巴：大眼眶、大眼珠往上望。
            open_eyes(&mut c, 12.0, 17.0, 3.8, 0.0, -2.5, 0.0);
        }
        Face::Dizzy => {
            // 头晕：眼珠贴着眼眶转圈（两只眼反着转）。
            for (i, cx) in EYES_X.into_iter().enumerate() {
                let dir = if i == 0 { 1.0 } else { -1.0 };
                let a = t * 6.0 * dir;
                c.ring(cx, EYE_CY, 11.0, 11.0, 1.6);
                c.disc(cx + 2.6 * a.cos(), EYE_CY + 2.6 * a.sin(), 1.8);
            }
        }
        Face::Star => {
            // 星星眼：十字星，一闪一闪（长短交替）。
            let k = 1.0 + 0.15 * (t * 8.0).sin();
            for cx in EYES_X {
                c.line((cx, EYE_CY - 6.0 * k), (cx, EYE_CY + 6.0 * k), 1.2);
                c.line((cx - 6.0 * k, EYE_CY), (cx + 6.0 * k, EYE_CY), 1.2);
                c.disc(cx, EYE_CY, 2.0);
                // 再加四道短斜光，不然小尺寸下像两个加号。
                let d = 3.6 * k;
                c.line((cx - d, EYE_CY - d), (cx + d, EYE_CY + d), 0.7);
                c.line((cx - d, EYE_CY + d), (cx + d, EYE_CY - d), 0.7);
            }
        }
        Face::Blush => {
            // 害羞：睁着眼，眼下两道斜线当腮红。
            open_eyes(&mut c, 11.0, 13.0, 2.6, 0.0, 0.0, -1.5);
            for cx in EYES_X {
                c.line((cx - 3.5, EYE_CY + 9.5), (cx - 1.5, EYE_CY + 7.5), 0.8);
                c.line((cx + 0.5, EYE_CY + 9.5), (cx + 2.5, EYE_CY + 7.5), 0.8);
            }
        }
        Face::Yawn => {
            // 打哈欠：眯成两道往下弯的线。
            for cx in EYES_X {
                c.line((cx - 4.5, EYE_CY - 1.0), (cx, EYE_CY + 1.5), 1.4);
                c.line((cx, EYE_CY + 1.5), (cx + 4.5, EYE_CY - 1.0), 1.4);
            }
        }
        Face::Happy => {
            for cx in EYES_X {
                c.line((cx - 4.5, EYE_CY + 2.5), (cx, EYE_CY - 2.5), 1.6);
                c.line((cx, EYE_CY - 2.5), (cx + 4.5, EYE_CY + 2.5), 1.6);
            }
        }
        Face::Worried => {
            // 担心：眼睛下移一点，上面两道内侧上扬的眉毛。
            open_eyes(&mut c, 10.0, 12.0, 2.6, 0.0, 0.0, 2.0);
            c.line(
                (EYES_X[0] - 4.0, EYE_CY - 7.5),
                (EYES_X[0] + 3.5, EYE_CY - 10.0),
                1.1,
            );
            c.line(
                (EYES_X[1] + 4.0, EYE_CY - 7.5),
                (EYES_X[1] - 3.5, EYE_CY - 10.0),
                1.1,
            );
        }
        Face::Focus => {
            // 专注：眯成一条缝，眼珠在缝里左右扫（同在跑）。
            let dx = 1.8 * (t * 3.5).sin();
            open_eyes(&mut c, 12.0, 7.0, 2.0, dx, 0.0, 0.0);
        }
        Face::Sleep => {
            // 睡着：两道横线，右上角一个小 z（慢慢浮动）。
            for cx in EYES_X {
                c.line((cx - 4.5, EYE_CY + 1.0), (cx + 4.5, EYE_CY + 1.0), 1.2);
            }
            let zy = 4.0 - (t * 1.5).sin();
            c.line((27.0, zy), (31.0, zy), 0.7);
            c.line((31.0, zy), (27.0, zy + 4.0), 0.7);
            c.line((27.0, zy + 4.0), (31.0, zy + 4.0), 0.7);
        }
        Face::Drowsy => {
            // 犯困：眼皮耷拉下来，只剩下半个眼眶，眼珠也往下沉。
            for cx in EYES_X {
                c.line((cx - 5.0, EYE_CY - 1.0), (cx + 5.0, EYE_CY - 1.0), 1.2);
                c.ring(cx, EYE_CY + 2.0, 10.0, 6.5, 1.6);
                c.disc(cx, EYE_CY + 2.5, 1.8);
            }
        }
    }
    c.rgba()
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
            let dx = if mood == Mood::Running {
                1.8 * (t * 3.5).sin()
            } else {
                0.0
            };
            let open = blink(t, period);
            for cx in EYES_X {
                if open < 0.4 {
                    // 眨眼的那一下：闭成一道横线。
                    c.line(
                        (cx - w / 2.0 + 1.0, EYE_CY),
                        (cx + w / 2.0 - 1.0, EYE_CY),
                        1.0,
                    );
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
    fn every_face_draws_something_and_they_differ() {
        let faces = [
            Face::Open,
            Face::Dead,
            Face::Frustrated,
            Face::Wide,
            Face::Plead,
            Face::Dizzy,
            Face::Star,
            Face::Surprised,
            Face::Blush,
            Face::Yawn,
            Face::Happy,
            Face::Worried,
            Face::Focus,
            Face::Sleep,
            Face::Drowsy,
        ];
        let frames: Vec<Vec<u8>> = faces
            .iter()
            .map(|f| render_face(0.5, Mood::Idle, *f))
            .collect();
        for (i, a) in frames.iter().enumerate() {
            assert!(
                a.iter().skip(3).step_by(4).any(|&v| v > 0),
                "{:?} 空白",
                faces[i]
            );
            for (j, b) in frames.iter().enumerate().skip(i + 1) {
                assert_ne!(a, b, "{:?} 和 {:?} 画得一样", faces[i], faces[j]);
            }
        }
        // 没给表情、或普通睁眼，都按 tone 的老画法；在跑时眼珠照样左右扫。
        assert_eq!(
            render_face(0.5, Mood::Running, Face::Auto),
            render(0.5, Mood::Running)
        );
        assert_eq!(
            render_face(0.5, Mood::Running, Face::Open),
            render(0.5, Mood::Running)
        );
        assert_ne!(
            render_face(0.5, Mood::Running, Face::Open),
            render_face(1.0, Mood::Running, Face::Open)
        );
        assert_eq!(Face::parse(Some("dizzy")), Face::Dizzy);
        assert_eq!(Face::parse(Some("whatever")), Face::Auto);
        assert_eq!(Face::parse(None), Face::Auto);
    }

    /// 把每种表情画成一张拼图（PGM 灰度图）给人看。手动跑：
    /// `cargo test --lib tray_anim::tests::dump_faces -- --ignored`，输出在系统临时目录。
    #[test]
    #[ignore]
    fn dump_faces() {
        let faces = [
            Face::Open,
            Face::Dead,
            Face::Frustrated,
            Face::Wide,
            Face::Plead,
            Face::Dizzy,
            Face::Star,
            Face::Surprised,
            Face::Blush,
            Face::Yawn,
            Face::Happy,
            Face::Worried,
            Face::Focus,
            Face::Sleep,
            Face::Drowsy,
        ];
        let cols = faces.len();
        let (w, h) = (WIDTH * cols + cols - 1, HEIGHT);
        let mut pixels = vec![255u8; w * h];
        for (i, face) in faces.iter().enumerate() {
            let frame = render_face(0.5, Mood::Idle, *face);
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    pixels[y * w + i * (WIDTH + 1) + x] = 255 - frame[(y * WIDTH + x) * 4 + 3];
                }
            }
        }
        let mut out = format!("P5 {w} {h} 255\n").into_bytes();
        out.extend(pixels);
        std::fs::write(std::env::temp_dir().join("meowo-tray-faces.pgm"), out).unwrap();
    }

    #[test]
    fn parse_falls_back_to_idle() {
        assert_eq!(Mood::parse("running"), Mood::Running);
        assert_eq!(Mood::parse("whatever"), Mood::Idle);
    }
}
