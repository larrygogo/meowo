//! 输出流里的屏幕控制序列识别：DEC 私有模式开关态（快照回放基线）与首帧前的光标位置探测代答。
//! 从 pty.rs 拆出（2026-11-10 二期）：这两块只吃字节、不碰 broker 状态，单独成模块便于读和测。

/// 跟踪输出流里 DEC 私有模式（`CSI ? Pm h/l`）的开关态，供快照回放基线用。
///
/// 只认 [`Self::TRACKED`] 里的几个「TUI 启动时设一次、影响 xterm 输入/视口行为」的模式。
/// 其它模式（光标可见 25、自动换行 7、同步输出 2026 …）要么回放里自带、要么 TUI 每帧
/// 都重发，不需要基线。`ESC c`（RIS）视为全部复位。
///
/// 序列可能跨 chunk 断开：尾部若是一段未闭合的疑似前缀（`ESC` / `ESC [` / `ESC [ ?` +
/// 参数），暂存到下一次 feed 前面拼接再扫。前缀超过 [`Self::CARRY_MAX`] 就不是模式序列，丢弃。
pub(super) struct ModeTracker {
    bits: u32,
    carry: Vec<u8>,
}

impl ModeTracker {
    /// 位序即下标。1049 排第一不是巧合：前端按此序补写，`?1049h` 会清屏并切缓冲，
    /// 必须先于鼠标/粘贴模式生效（后者与缓冲无关，但顺序稳定便于测试与阅读）。
    pub(super) const TRACKED: [u16; 10] = [1049, 47, 1047, 1000, 1002, 1003, 1005, 1006, 1015, 2004];
    pub(super) const CARRY_MAX: usize = 24;

    pub(super) fn new() -> Self {
        Self {
            bits: 0,
            carry: Vec::new(),
        }
    }

    pub(super) fn bits(&self) -> u32 {
        self.bits
    }

    /// 位图 → 开着的模式号列表（按 TRACKED 顺序）。
    pub(super) fn modes_of(bits: u32) -> Vec<u16> {
        Self::TRACKED
            .iter()
            .enumerate()
            .filter(|(i, _)| bits & (1 << i) != 0)
            .map(|(_, m)| *m)
            .collect()
    }

    pub(super) fn apply(&mut self, params: &[u8], on: bool) {
        for param in params.split(|b| *b == b';') {
            let Ok(mode) = std::str::from_utf8(param).unwrap_or("").parse::<u16>() else {
                continue;
            };
            if let Some(idx) = Self::TRACKED.iter().position(|m| *m == mode) {
                if on {
                    self.bits |= 1 << idx;
                } else {
                    self.bits &= !(1 << idx);
                }
            }
        }
    }

    pub(super) fn feed(&mut self, chunk: &[u8]) {
        let data: std::borrow::Cow<'_, [u8]> = if self.carry.is_empty() {
            std::borrow::Cow::Borrowed(chunk)
        } else {
            let mut joined = std::mem::take(&mut self.carry);
            joined.extend_from_slice(chunk);
            std::borrow::Cow::Owned(joined)
        };
        let data = &*data;
        let mut i = 0;
        while i < data.len() {
            if data[i] != 0x1b {
                i += 1;
                continue;
            }
            // ESC 之后的字节不足以判定:暂存尾巴(最多 "ESC [ ?" 三字节)等下一块。
            let Some(&b1) = data.get(i + 1) else {
                self.carry.extend_from_slice(&data[i..]);
                return;
            };
            if b1 == b'c' {
                // ESC c = RIS:全部复位。
                self.bits = 0;
                i += 2;
                continue;
            }
            if b1 != b'[' {
                i += 1;
                continue;
            }
            let Some(&b2) = data.get(i + 2) else {
                self.carry.extend_from_slice(&data[i..]);
                return;
            };
            if b2 != b'?' {
                // 普通 CSI:不解析,跳过前导继续。
                i += 2;
                continue;
            }
            // ESC [ ? <params> <final>
            let mut j = i + 3;
            while j < data.len() && (data[j].is_ascii_digit() || data[j] == b';') {
                j += 1;
            }
            let Some(&final_byte) = data.get(j) else {
                // 未闭合:尾部暂存(过长则不是模式序列,丢弃)。
                if data.len() - i <= Self::CARRY_MAX {
                    self.carry.extend_from_slice(&data[i..]);
                }
                return;
            };
            if final_byte == b'h' || final_byte == b'l' {
                self.apply(&data[i + 3..j], final_byte == b'h');
            }
            i = j + 1;
        }
    }
}

#[cfg(test)]
mod mode_tracker_tests {
    use super::ModeTracker;

    #[test]
    pub(super) fn tracks_set_and_reset_in_tracked_order() {
        let mut t = ModeTracker::new();
        t.feed(b"\x1b[?1004h\x1b[?1049h\x1b[2J\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[?2004h");
        assert_eq!(
            ModeTracker::modes_of(t.bits()),
            vec![1049, 1000, 1002, 1006, 2004]
        );
        t.feed(b"\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?1049l");
        assert_eq!(ModeTracker::modes_of(t.bits()), vec![2004]);
    }

    /// 跨 chunk 断在任意位置都不能漏:ConPTY 的 16KB 读块与 TUI 的写边界无关。
    #[test]
    pub(super) fn survives_sequences_split_across_chunks() {
        let full = b"ab\x1b[?1049h\x1b[?1000;1006hxy";
        for cut in 0..=full.len() {
            let mut t = ModeTracker::new();
            t.feed(&full[..cut]);
            t.feed(&full[cut..]);
            assert_eq!(
                ModeTracker::modes_of(t.bits()),
                vec![1049, 1000, 1006],
                "cut at {cut}"
            );
        }
    }

    #[test]
    pub(super) fn ris_clears_everything_and_untracked_modes_are_ignored() {
        let mut t = ModeTracker::new();
        t.feed(b"\x1b[?25l\x1b[?7h\x1b[?1049h\x1b[?2026h");
        assert_eq!(ModeTracker::modes_of(t.bits()), vec![1049]);
        t.feed(b"\x1bc");
        assert!(ModeTracker::modes_of(t.bits()).is_empty());
    }

    /// 非模式的长 `CSI ?` 序列(或垃圾)不能把 carry 撑成无限缓冲。
    #[test]
    pub(super) fn oversized_prefix_is_dropped_not_carried() {
        let mut t = ModeTracker::new();
        t.feed(b"\x1b[?11111111111111111111111111111111");
        assert!(t.carry.is_empty());
        t.feed(b"h");
        assert!(ModeTracker::modes_of(t.bits()).is_empty());
    }
}

/// TUI 启动探测(`ESC[6n` 光标位置查询)的**唯一**应答者。
///
/// claude 等 TUI 启动时先发 `ESC[6n` 探测终端,**得不到回应就永远不画第一帧**;而此刻
/// 往往还没有任何视图挂着(对话窗 WebView 冷启动要一两秒,外部 attach 更晚),查询只能
/// 躺在 backlog 里等回放。谁替它答?此前的答案是「每个视图自己答」:xterm 自动应答、
/// attach 客户端 DsrFilter 代答、前端回放代答——应答者一多,再叠上重连/重挂对同一段
/// backlog 的重扫,同一个查询就会被答多次;多出来的应答落进 agent 输入框,成了孤立的
/// 杂字符(真实案例:恢复会话后 claude 的 composer 里凭空多一个 C)。
///
/// 现在收成单一所有者:PTY reader 对输出流做**单趟**扫描,首个可见字节之前的 `ESC[6n]`
/// 由后端当场代答 `ESC[1;1R`——TUI 只要一个基准值,真实排版靠后续的清屏与绝对定位
/// 序列,(1,1) 与 attach 客户端 DsrFilter 的既有行为一致。单趟意味着每个查询字节只被
/// 检视一次,重连、回放、组件重挂都不可能触发第二次应答。首帧画出后 TUI 的实时查询
/// 交还给活着的视图(xterm 以真实光标位置应答),本扫描器永久停机。
///
/// **已代答的探测同时从流中摘除**:单一应答者的另一半含义是下游根本看不到已答的查询。
/// 摘除之前,同一条首帧前探测会被订阅在先的 attach 客户端(DsrFilter 对实时查询全数
/// 代答)或与首个可见字节挤进同一事件帧的 GUI xterm 再答一遍——多出的应答落进 agent
/// 输入框。摘除之后 DsrFilter/xterm 只会遇到首帧后的实时查询,那正是它们该答的。
pub(super) struct StartupProbeScanner {
    /// 已见到可见字节:探测期结束,feed 恒原样透传。
    painted: bool,
    state: ProbeScanState,
    /// 疑似探测前缀的暂存(`ESC`/`ESC[`/`ESC[6`,最多 3 字节):确定是探测则丢弃
    /// (已代答),确定不是则原样冲入输出。只在首帧前使用,扣住几个字节无副作用;
    /// 回到 Ground 时恒为空,painted 翻转只发生在 Ground,故停机时不会扣留字节。
    hold: Vec<u8>,
}

#[derive(Clone, Copy)]
pub(super) enum ProbeScanState {
    Ground,
    Esc,
    /// ESC + 中间字节(0x20-0x2F,如 charset 指定 `ESC ( B`):负载不是画面,等最终字节。
    /// 此前这类序列落回 Ground,负载字节被误判为可见输出,扫描器提前停机——探测从此
    /// 没人答,TUI 卡在首帧前(25s 兜底后黑屏)。
    EscIntermediate,
    /// CSI:hold 非空 = 仍可能是探测;hold 已冲出 = 已排除,透传到最终字节。
    Csi,
    Osc,
    OscEsc,
    /// DCS/SOS/PM/APC 字符串(ESC P/X/^/_):负载不是画面,吞到 ST(ESC \)。
    Str,
    StrEsc,
}

impl StartupProbeScanner {
    pub(super) fn new() -> Self {
        Self {
            painted: false,
            state: ProbeScanState::Ground,
            hold: Vec::new(),
        }
    }

    pub(super) fn painted(&self) -> bool {
        self.painted
    }

    pub(super) fn flush(&mut self, out: &mut Vec<u8>) {
        out.append(&mut self.hold);
    }

    /// 返回(应转发/入 backlog 的字节, 本 chunk 需要代答的探测个数)。已代答的探测不在
    /// 返回字节里;疑似探测前缀暂存到下一 chunk,撕裂在边界上的查询照样只答一次、不外流。
    /// 可见性口径与前端 hasVisibleOutput 一致:ESC 序列与 ≤0x20 的空白控制字节都不算画面。
    pub(super) fn feed(&mut self, chunk: &[u8]) -> (Vec<u8>, usize) {
        let mut out = Vec::with_capacity(self.hold.len() + chunk.len());
        if self.painted {
            out.extend_from_slice(chunk);
            return (out, 0);
        }
        let mut probes = 0;
        for (index, &byte) in chunk.iter().enumerate() {
            match self.state {
                ProbeScanState::Ground => {
                    if byte == 0x1b {
                        self.hold.push(byte);
                        self.state = ProbeScanState::Esc;
                    } else if byte > 0x20 && byte != 0x7f {
                        // 可见字节:探测期结束,本字节与其后全部原样透传。
                        self.painted = true;
                        out.push(byte);
                        out.extend_from_slice(&chunk[index + 1..]);
                        return (out, probes);
                    } else {
                        out.push(byte);
                    }
                }
                // Esc 状态 hold 恒为 [ESC](只从 Ground 进入)。
                ProbeScanState::Esc => match byte {
                    0x5b => {
                        self.hold.push(byte);
                        self.state = ProbeScanState::Csi;
                    }
                    0x5d => {
                        self.flush(&mut out);
                        out.push(byte);
                        self.state = ProbeScanState::Osc;
                    }
                    // ESC ESC:冲出前一个,新的接着暂存(hold 恰好不变)。
                    0x1b => out.push(0x1b),
                    0x20..=0x2f => {
                        self.flush(&mut out);
                        out.push(byte);
                        self.state = ProbeScanState::EscIntermediate;
                    }
                    b'P' | b'X' | b'^' | b'_' => {
                        self.flush(&mut out);
                        out.push(byte);
                        self.state = ProbeScanState::Str;
                    }
                    // 双字节 ESC 序列(ESC 7 等):吞掉 kind 字节回到地面。
                    _ => {
                        self.flush(&mut out);
                        out.push(byte);
                        self.state = ProbeScanState::Ground;
                    }
                },
                ProbeScanState::EscIntermediate => {
                    out.push(byte);
                    // 中间字节(0x20-0x2F)可连续多个;其余任意字节都当最终字节收尾。
                    if !(0x20..=0x2f).contains(&byte) {
                        self.state = ProbeScanState::Ground;
                    }
                }
                ProbeScanState::Csi => {
                    if self.hold.is_empty() {
                        // 已排除探测的 CSI:透传到最终字节。
                        out.push(byte);
                        if (0x40..=0x7e).contains(&byte) {
                            self.state = ProbeScanState::Ground;
                        }
                    } else if self.hold.len() == 2 && byte == b'6' {
                        self.hold.push(byte);
                    } else if self.hold.len() == 3 && byte == b'n' {
                        // 完整 `ESC[6n`:代答,并把这四个字节从流中摘除。
                        self.hold.clear();
                        probes += 1;
                        self.state = ProbeScanState::Ground;
                    } else {
                        // 参数不是恰好 "6":排除探测,冲出暂存,本字节照常处理。
                        self.flush(&mut out);
                        out.push(byte);
                        if (0x40..=0x7e).contains(&byte) {
                            self.state = ProbeScanState::Ground;
                        }
                    }
                }
                ProbeScanState::Osc => {
                    out.push(byte);
                    if byte == 0x07 {
                        self.state = ProbeScanState::Ground;
                    } else if byte == 0x1b {
                        self.state = ProbeScanState::OscEsc;
                    }
                }
                ProbeScanState::OscEsc => {
                    // ST(ESC \)收尾;其余当 OSC 内容继续吞。
                    out.push(byte);
                    self.state = if byte == 0x5c {
                        ProbeScanState::Ground
                    } else {
                        ProbeScanState::Osc
                    };
                }
                ProbeScanState::Str => {
                    out.push(byte);
                    if byte == 0x1b {
                        self.state = ProbeScanState::StrEsc;
                    }
                }
                ProbeScanState::StrEsc => {
                    out.push(byte);
                    self.state = match byte {
                        0x5c => ProbeScanState::Ground,
                        0x1b => ProbeScanState::StrEsc,
                        _ => ProbeScanState::Str,
                    };
                }
            }
        }
        (out, probes)
    }
}

/// 从**展示用**字节流中移除完整的 `ESC[6n` 查询。只用于 attach 回放:客户端 DsrFilter
/// 会对流里的每个查询代答一遍,而回放里的查询全是历史——首帧前的探测已在 reader 处
/// 被摘除根本不进 backlog(StartupProbeScanner),留下的是首帧后的查询,当年已由活着
/// 的视图答过,迟到的代答会打进正跑着的 agent 输入框(「重开外部同步终端后 composer
/// 里多一个 C」的直接来源)。backlog 本体与偏移一个字节都不能动:GUI 快照按偏移对齐
/// 增量。跨 backlog 裁剪边界的残缺前缀不匹配、原样保留(既有的碎片语义)。
pub(super) fn strip_dsr_queries(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i..].starts_with(b"\x1b[6n") {
            i += 4;
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    out
}
