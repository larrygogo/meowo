//! OSC 7501（Program Status Protocol）：CLI 在自己 TUI 的输出流里报告状态。
//!
//! 规范：<https://www.superlogical.com/rex/docs/build/program-status>。程序先发探测
//! `ESC ] 7501 ; ? ST`，终端应答同形的 `?` 序列后，程序在每次状态变化时发报告
//! `ESC ] 7501 ; key=value(:key=value)* ST`（ST 为 BEL 或 `ESC \`）。2026-11-26 本机实测
//! （任务 .lead/20261126-program-status/agents/spike）：claude 2.1.332、codex 0.169.0、
//! kimi 2.7.3 在应答之后都发报告；不应答时 claude 只发探测、照旧把转圈写进标题。
//!
//! 为什么要它：会话状态（在跑 / 等批准 / 等回答 / 空闲）此前全靠认屏幕（plugins/*/screen.rs），
//! CLI 每改一次界面就可能误判。报告是 CLI 自己说的，不随界面文案变化；它走的仍是官方 TUI
//! 的输出，不另起进程、不增加流量（product.md 2026-11-13「不绕开官方 TUI」）。
//!
//! 本模块只吃字节、不碰会话：[`StatusFilter`] 在 PTY reader 的单趟流上摘出 7501 序列，
//! [`parse_report`] 按规范校验，[`ProgramStatus`] 按 id 维护记录。msg/title 是**不受信任的
//! 模型输出**：控制字符整条丢弃，双向控制与不可见格式字符剔除，只当纯文本用。

use std::borrow::Cow;
use std::collections::HashMap;

use base64::Engine as _;

use crate::screen::ScreenState;

/// 终端对探测的应答：声明支持的状态与阻塞类别（规范的默认全集）。BEL 结尾，各家都认。
pub const PROBE_REPLY: &[u8] =
    b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x07";

/// `ESC ] 7501 ;`：序列前缀。
const PREFIX: &[u8] = b"\x1b]7501;";
/// 整条序列上限（规范 4096 字节，含前缀与终止符）。超出的报告整条丢弃。
const MAX_SEQUENCE: usize = 4096;
const MAX_KEY: usize = 16;
const MAX_MSG_ENCODED: usize = 2732;
const MAX_MSG_DECODED: usize = 2048;
const MAX_TITLE_ENCODED: usize = 256;
const MAX_TITLE_DECODED: usize = 192;
const MAX_APP: usize = 32;
const MAX_ID: usize = 128;
const MAX_ID_SEGMENT: usize = 32;
const MAX_ID_DEPTH: usize = 8;
/// 每个终端保留的记录数上限（规范要求至少 64，建议 256）。超出淘汰最久未更新的。
pub const MAX_RECORDS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
    Clear,
}

impl ReportState {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "idle" => Self::Idle,
            "working" => Self::Working,
            "done" => Self::Done,
            "blocked" => Self::Blocked,
            "error" => Self::Error,
            "clear" => Self::Clear,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Error => "error",
            Self::Clear => "clear",
        }
    }

    /// 折成 Meowo 的三态。done/error/idle 都是「主回合停了、轮到人」；error 的原因在 msg 里。
    pub fn screen_state(self) -> Option<ScreenState> {
        match self {
            Self::Working => Some(ScreenState::Working),
            Self::Blocked => Some(ScreenState::Blocked),
            Self::Idle | Self::Done | Self::Error => Some(ScreenState::Idle),
            Self::Clear => None,
        }
    }
}

/// blocked 的原因：批准、回答、登录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedKind {
    Permission,
    Question,
    Auth,
}

impl BlockedKind {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "permission" => Self::Permission,
            "question" => Self::Question,
            "auth" => Self::Auth,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Permission => "permission",
            Self::Question => "question",
            Self::Auth => "auth",
        }
    }
}

/// 一条通过校验的报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// None = 根记录；否则是 `/` 分隔的路径（子代理等）。
    pub id: Option<String>,
    pub state: ReportState,
    /// 只在 blocked 时有意义，其余状态下恒为 None。
    pub kind: Option<BlockedKind>,
    /// 0-100，只在 working/blocked 时有意义；None = 不确定进度。
    pub progress: Option<u8>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub msg: Option<String>,
}

/// 从流里摘出的事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    /// 程序发来探测：终端应回 [`PROBE_REPLY`]。
    Probe,
    Report(Report),
    /// `ESC c`（RIS）：规范要求清掉全部记录。RIS 本身原样透传，只是同时报告。
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterState {
    Ground,
    /// 暂存区里是 PREFIX 的一个真前缀。
    Prefix,
    /// 前缀已齐，正在收报文。
    Body,
    /// 报文里遇到 ESC：下一字节是 `\` 则收尾，否则整条作废。
    BodyEsc,
}

/// PTY reader 单趟流上的 7501 摘除器。
///
/// 只摘 `ESC ] 7501 ;` 开头的序列，其余字节原样、按序透传。序列可能被 16KB 读块从任意
/// 位置切开：前缀的一部分先暂存（最多 6 字节），确定不是 7501 就原样冲出；报文收在内部缓冲，
/// 不会外流。
///
/// 热路径：TUI 输出几乎每块都有 ESC，所以快路径不看 ESC，而是用 SIMD 子串搜索找完整前缀
/// （[`memchr::memmem`]）。块里没有前缀、块尾也不是半截前缀时零拷贝直通——绝大多数块如此；
/// 只有真含 7501 序列或跨块的块才走逐字节的慢路径（2026-11-26 基准：逐 ESC 状态机的写法让
/// reader 管线吞吐掉到 85%，见宿主 bench_reader_pipeline_with_status_filter）。
///
/// 摘除的理由：下游（xterm、外部终端、vt100 认屏幕、backlog 回放）都不需要它；外部终端
/// 若也支持 7501，看到探测会再答一遍，第二份应答落进 agent 输入框。
#[derive(Debug)]
pub struct StatusFilter {
    state: FilterState,
    /// 前缀暂存（Prefix 态）或报文（Body/BodyEsc 态，不含前缀）。
    buf: Vec<u8>,
    /// 报文已超过上限：继续吞到终止符，然后整条丢弃。
    oversize: bool,
    prefix: memchr::memmem::Finder<'static>,
    reset: memchr::memmem::Finder<'static>,
}

impl Default for StatusFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusFilter {
    pub fn new() -> Self {
        Self {
            state: FilterState::Ground,
            buf: Vec::new(),
            oversize: false,
            prefix: memchr::memmem::Finder::new(PREFIX),
            reset: memchr::memmem::Finder::new(b"\x1bc"),
        }
    }

    /// 喂一块输出，返回应继续向下游分发的字节，事件追加到 `events`。
    pub fn feed<'a>(&mut self, chunk: &'a [u8], events: &mut Vec<StreamEvent>) -> Cow<'a, [u8]> {
        if self.state == FilterState::Ground
            && self.prefix.find(chunk).is_none()
            && !(1..PREFIX.len()).any(|len| chunk.ends_with(&PREFIX[..len]))
        {
            // 与慢路径同一结论：输出即原块；每个 `ESC c` 报一次复位。
            for _ in self.reset.find_iter(chunk) {
                events.push(StreamEvent::Reset);
            }
            return Cow::Borrowed(chunk);
        }
        self.feed_slow(chunk, events)
    }

    /// 普通字节成段输出，顺带报告段内的每个 `ESC c`。段尾的 ESC 不会是半截的 RIS：调用方
    /// 已把块尾的半截前缀（含单独一个 ESC）暂存，段后紧跟的要么是前缀（ESC 开头，不是 `c`），
    /// 要么是块尾。
    fn emit_plain(&self, segment: &[u8], out: &mut Vec<u8>, events: &mut Vec<StreamEvent>) {
        for _ in self.reset.find_iter(segment) {
            events.push(StreamEvent::Reset);
        }
        out.extend_from_slice(segment);
    }

    /// 报文成段收进缓冲。整条序列（前缀 + 报文 + 1 字节终止符）超过 [`MAX_SEQUENCE`] 即
    /// 判超限：之后只吞不存，收尾时整条丢弃。
    fn push_body(&mut self, bytes: &[u8]) {
        if self.oversize {
            return;
        }
        if self.buf.len() + bytes.len() > MAX_SEQUENCE - PREFIX.len() - 1 {
            self.oversize = true;
            self.buf.clear();
            return;
        }
        self.buf.extend_from_slice(bytes);
    }

    /// 慢路径：块里有 7501 前缀、块尾是半截前缀，或上一块留下了未完的序列时走这里。
    /// 仍按段处理：普通字节用子串搜索整段拷贝，报文用 memchr2 找终止符；只有跨块暂存的
    /// 半截前缀（最多 6 字节）逐字节判。
    fn feed_slow<'a>(&mut self, chunk: &'a [u8], events: &mut Vec<StreamEvent>) -> Cow<'a, [u8]> {
        let mut out = Vec::with_capacity(chunk.len());
        let mut index = 0;
        while index < chunk.len() {
            let byte = chunk[index];
            match self.state {
                FilterState::Ground => {
                    let rest = &chunk[index..];
                    if let Some(offset) = self.prefix.find(rest) {
                        self.emit_plain(&rest[..offset], &mut out, events);
                        index += offset + PREFIX.len();
                        self.buf.clear();
                        self.oversize = false;
                        self.state = FilterState::Body;
                        continue;
                    }
                    // 没有完整前缀：块尾若是半截前缀就暂存，等下一块定夺。
                    let tail = (1..PREFIX.len())
                        .rev()
                        .find(|len| rest.ends_with(&PREFIX[..*len]))
                        .unwrap_or(0);
                    self.emit_plain(&rest[..rest.len() - tail], &mut out, events);
                    if tail > 0 {
                        self.buf.clear();
                        self.buf.extend_from_slice(&rest[rest.len() - tail..]);
                        self.state = FilterState::Prefix;
                    }
                    break;
                }
                FilterState::Prefix => {
                    let expected = PREFIX[self.buf.len()];
                    if byte == expected {
                        self.buf.push(byte);
                        if self.buf.len() == PREFIX.len() {
                            self.buf.clear();
                            self.oversize = false;
                            self.state = FilterState::Body;
                        }
                    } else {
                        // 不是 7501：暂存原样冲出，本字节回到地面重新判（它可能是新的 ESC）。
                        if self.buf.len() == 1 && byte == b'c' {
                            events.push(StreamEvent::Reset);
                        }
                        out.append(&mut self.buf);
                        self.state = FilterState::Ground;
                        if byte == 0x1b {
                            continue;
                        }
                        out.push(byte);
                    }
                }
                FilterState::Body => {
                    let rest = &chunk[index..];
                    match memchr::memchr2(0x07, 0x1b, rest) {
                        None => {
                            self.push_body(rest);
                            break;
                        }
                        Some(offset) => {
                            self.push_body(&rest[..offset]);
                            index += offset;
                            if rest[offset] == 0x07 {
                                self.finish(events);
                            } else {
                                self.state = FilterState::BodyEsc;
                            }
                        }
                    }
                }
                FilterState::BodyEsc => {
                    if byte == b'\\' {
                        self.finish(events);
                    } else {
                        // ESC 打断了 OSC（ECMA-48：字符串里出现 ESC 即终止）：报文作废，
                        // 这个 ESC 开启的是一条新序列，按前缀重新匹配本字节。
                        self.buf.clear();
                        self.buf.push(0x1b);
                        self.state = FilterState::Prefix;
                        continue;
                    }
                }
            }
            index += 1;
        }
        Cow::Owned(out)
    }

    fn finish(&mut self, events: &mut Vec<StreamEvent>) {
        let body = std::mem::take(&mut self.buf);
        self.state = FilterState::Ground;
        if std::mem::take(&mut self.oversize) {
            return;
        }
        if body.first() == Some(&b'?') {
            events.push(StreamEvent::Probe);
        } else if let Some(report) = parse_report(&body) {
            events.push(StreamEvent::Report(report));
        }
    }
}

fn is_value_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b',' | b'+' | b'/' | b'=' | b'-')
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'+' | b'-')
}

fn valid_id(id: &str) -> bool {
    if id.is_empty() || id.len() > MAX_ID {
        return false;
    }
    let mut depth = 0;
    for segment in id.split('/') {
        depth += 1;
        if segment.is_empty()
            || segment.len() > MAX_ID_SEGMENT
            || !segment.bytes().all(is_name_byte)
        {
            return false;
        }
    }
    depth <= MAX_ID_DEPTH
}

/// msg/title 解码：标准 base64（填充可省），UTF-8，含控制字符整条拒收；剔除双向控制与
/// 不可见格式字符（规范 SHOULD：防止把界面文字倒序、藏字）。Err = 整条报告作废。
fn decode_text(encoded: &str, max_encoded: usize, max_decoded: usize) -> Result<String, ()> {
    if encoded.len() > max_encoded {
        return Err(());
    }
    let trimmed = encoded.trim_end_matches('=');
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(trimmed)
        .map_err(|_| ())?;
    if bytes.len() > max_decoded {
        return Err(());
    }
    let text = String::from_utf8(bytes).map_err(|_| ())?;
    if text.chars().any(char::is_control) {
        return Err(());
    }
    Ok(text
        .chars()
        .filter(|ch| {
            !matches!(
                *ch,
                '\u{200B}'..='\u{200F}'
                    | '\u{202A}'..='\u{202E}'
                    | '\u{2060}'..='\u{2064}'
                    | '\u{2066}'..='\u{2069}'
                    | '\u{FEFF}'
            )
        })
        .collect())
}

/// 按规范解析报文（不含前缀与终止符）。None = 整条忽略（未知 state、超限、坏 base64 等）。
pub fn parse_report(body: &[u8]) -> Option<Report> {
    let body = std::str::from_utf8(body).ok()?;
    // 规范只定义这几个键，逐个记下（重复的键取最后一个），未知键忽略。不建 HashMap：
    // 报告在 reader 热路径上解析。
    let (mut state, mut id, mut kind, mut progress, mut app, mut title, mut msg) =
        (None, None, None, None, None, None, None);
    for pair in body.split(':') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key.is_empty() || key.len() > MAX_KEY || !key.bytes().all(|b| b.is_ascii_lowercase()) {
            continue;
        }
        if !value.bytes().all(is_value_byte) {
            continue;
        }
        let slot = match key {
            "state" => &mut state,
            "id" => &mut id,
            "kind" => &mut kind,
            "progress" => &mut progress,
            "app" => &mut app,
            "title" => &mut title,
            "msg" => &mut msg,
            _ => continue,
        };
        *slot = Some(value);
    }
    let state = ReportState::parse(state?)?;
    let id = match id {
        Some(id) if valid_id(id) => Some(id.to_string()),
        Some(_) => return None,
        None => None,
    };
    let app = match app {
        Some(app) if !app.is_empty() && app.len() <= MAX_APP && app.bytes().all(is_name_byte) => {
            Some(app.to_string())
        }
        Some(_) => return None,
        None => None,
    };
    let kind = if state == ReportState::Blocked {
        kind.and_then(BlockedKind::parse)
    } else {
        None
    };
    let progress = if matches!(state, ReportState::Working | ReportState::Blocked) {
        progress
            .and_then(|value: &str| value.parse::<u8>().ok())
            .filter(|value| *value <= 100)
    } else {
        None
    };
    let msg = match msg {
        Some(encoded) => Some(decode_text(encoded, MAX_MSG_ENCODED, MAX_MSG_DECODED).ok()?),
        None => None,
    };
    let title = match title {
        Some(encoded) => Some(decode_text(encoded, MAX_TITLE_ENCODED, MAX_TITLE_DECODED).ok()?),
        None => None,
    };
    Some(Report {
        id,
        state,
        kind,
        progress,
        app,
        title,
        msg: msg.filter(|text| !text.trim().is_empty()),
    })
}

/// 一条记录（报告去掉 id 与 clear）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub state: ReportState,
    pub kind: Option<BlockedKind>,
    pub progress: Option<u8>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub msg: Option<String>,
    /// 最后更新的墙钟毫秒（诊断显示用）。
    pub updated_ms: i64,
    /// 单调更新序号：淘汰最久未更新的记录用，不受墙钟回拨影响。
    seq: u64,
}

/// 一个终端的全部记录。键 "" 为根记录。
#[derive(Debug, Default)]
pub struct ProgramStatus {
    records: HashMap<String, Record>,
    seq: u64,
}

impl ProgramStatus {
    pub fn apply(&mut self, report: Report, now_ms: i64) {
        let key = report.id.unwrap_or_default();
        if report.state == ReportState::Clear {
            if key.is_empty() {
                self.records.clear();
            } else {
                let nested = format!("{key}/");
                self.records
                    .retain(|id, _| id != &key && !id.starts_with(&nested));
            }
            return;
        }
        self.seq += 1;
        self.records.insert(
            key,
            Record {
                state: report.state,
                kind: report.kind,
                progress: report.progress,
                app: report.app,
                title: report.title,
                msg: report.msg,
                updated_ms: now_ms,
                seq: self.seq,
            },
        );
        if self.records.len() > MAX_RECORDS {
            if let Some(oldest) = self
                .records
                .iter()
                .min_by_key(|(_, record)| record.seq)
                .map(|(id, _)| id.clone())
            {
                self.records.remove(&oldest);
            }
        }
    }

    /// RIS 或进程退出：全部清掉。
    pub fn reset(&mut self) {
        self.records.clear();
    }

    pub fn root(&self) -> Option<&Record> {
        self.records.get("")
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 全部记录按 id 排序（根记录 "" 在最前），诊断用。
    pub fn records(&self) -> Vec<(&str, &Record)> {
        let mut list: Vec<(&str, &Record)> = self
            .records
            .iter()
            .map(|(id, record)| (id.as_str(), record))
            .collect();
        list.sort_by(|a, b| a.0.cmp(b.0));
        list
    }

    /// 记录的 app：子记录没写时继承最近的祖先（规范）。
    pub fn app_of(&self, id: &str) -> Option<&str> {
        let mut current = id;
        loop {
            if let Some(app) = self.records.get(current).and_then(|r| r.app.as_deref()) {
                return Some(app);
            }
            if current.is_empty() {
                return None;
            }
            current = current.rsplit_once('/').map(|(parent, _)| parent).unwrap_or("");
        }
    }

    /// 会话的状态结论：只看根记录（子记录是子代理，主回合由根记录说了算——主回合停了而
    /// 后台子代理在跑时，「在跑还是等你」由看板的 background_busy 另行判定）。
    pub fn session_state(&self) -> Option<(ScreenState, Option<BlockedKind>)> {
        let root = self.root()?;
        Some((root.state.screen_state()?, root.kind))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(text: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(text)
    }

    fn feed_all(filter: &mut StatusFilter, chunks: &[&[u8]]) -> (Vec<u8>, Vec<StreamEvent>) {
        let mut out = Vec::new();
        let mut events = Vec::new();
        for chunk in chunks {
            out.extend_from_slice(&filter.feed(chunk, &mut events));
        }
        (out, events)
    }

    #[test]
    fn plain_output_passes_through_without_copy() {
        let mut filter = StatusFilter::new();
        let mut events = Vec::new();
        let out = filter.feed(b"hello world", &mut events);
        assert!(matches!(out, Cow::Borrowed(_)));
        assert!(events.is_empty());
    }

    #[test]
    fn probe_and_report_are_removed_and_reported() {
        let mut filter = StatusFilter::new();
        let msg = b64("approve Write: a.txt");
        let stream = format!(
            "a\x1b[>0q\x1b]7501;?\x07\x1b[cb\x1b]7501;state=blocked:app=claude-code:kind=permission:msg={msg}\x1b\\c"
        );
        let (out, events) = feed_all(&mut filter, &[stream.as_bytes()]);
        assert_eq!(out, b"a\x1b[>0q\x1b[cbc");
        assert_eq!(events[0], StreamEvent::Probe);
        let StreamEvent::Report(report) = &events[1] else {
            panic!("expected report, got {events:?}");
        };
        assert_eq!(report.state, ReportState::Blocked);
        assert_eq!(report.kind, Some(BlockedKind::Permission));
        assert_eq!(report.app.as_deref(), Some("claude-code"));
        assert_eq!(report.msg.as_deref(), Some("approve Write: a.txt"));
    }

    /// 16KB 读块与 TUI 的写边界无关：在任意位置切开都要得到同样的输出和事件。
    #[test]
    fn sequences_split_at_every_position_give_identical_results() {
        let msg = b64("任务摘要");
        let stream = format!(
            "x\x1b]0;title\x07\x1b]7501;?\x07y\x1b]7501;state=working:app=codex:msg={msg}\x07\x1b[1mz\x1b]75\x07\x1bc"
        );
        let bytes = stream.as_bytes();
        let mut whole = StatusFilter::new();
        let expected = feed_all(&mut whole, &[bytes]);
        assert_eq!(expected.0, "x\x1b]0;title\x07y\x1b[1mz\x1b]75\x07\x1bc".as_bytes());
        assert_eq!(expected.1.len(), 3);
        assert_eq!(expected.1[2], StreamEvent::Reset);
        for cut in 0..=bytes.len() {
            let mut filter = StatusFilter::new();
            let got = feed_all(&mut filter, &[&bytes[..cut], &bytes[cut..]]);
            assert_eq!(got, expected, "cut at {cut}");
        }
        // 逐字节喂也一样。
        let mut filter = StatusFilter::new();
        let singles: Vec<&[u8]> = bytes.chunks(1).collect();
        assert_eq!(feed_all(&mut filter, &singles), expected);
    }

    /// 快路径只是慢路径的捷径：真实录流在各种块大小下，两条路得到的字节与事件必须完全一致。
    #[test]
    fn fast_path_matches_slow_path_on_real_streams() {
        let fixtures: [&[u8]; 3] = [
            include_bytes!("../tests/fixtures/program-status/claude-2.1.332-turn-interrupt.bin"),
            include_bytes!("../tests/fixtures/program-status/codex-0.169.0-approval.bin"),
            include_bytes!("../tests/fixtures/program-status/kimi-2.7.3-question.bin"),
        ];
        let mut with_ris = b"a\x1bcb\x1b".to_vec();
        with_ris.extend_from_slice(b"cc\x1b]7501;state=idle\x07\x1b");
        for bytes in fixtures.iter().copied().chain([with_ris.as_slice()]) {
            for size in [1, 2, 5, 7, 64, 777, 4096, 16 * 1024] {
                let mut fast = StatusFilter::new();
                let mut slow = StatusFilter::new();
                let (mut fast_out, mut fast_events) = (Vec::new(), Vec::new());
                let (mut slow_out, mut slow_events) = (Vec::new(), Vec::new());
                for piece in bytes.chunks(size) {
                    fast_out.extend_from_slice(&fast.feed(piece, &mut fast_events));
                    slow_out.extend_from_slice(&slow.feed_slow(piece, &mut slow_events));
                }
                assert_eq!(fast_out, slow_out, "chunk {size}");
                assert_eq!(fast_events, slow_events, "chunk {size}");
            }
        }
    }

    #[test]
    fn oversized_report_is_swallowed_and_dropped() {
        let mut filter = StatusFilter::new();
        let huge = "A".repeat(5000);
        let stream = format!("a\x1b]7501;state=working:msg={huge}\x07b\x1b]7501;state=idle\x07");
        let (out, events) = feed_all(&mut filter, &[stream.as_bytes()]);
        assert_eq!(out, b"ab");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], StreamEvent::Report(r) if r.state == ReportState::Idle));
        // 内部缓冲没有被撑大到超限报文的长度。
        assert!(filter.buf.capacity() <= MAX_SEQUENCE + 64);
    }

    #[test]
    fn esc_inside_body_aborts_and_starts_a_new_sequence() {
        let mut filter = StatusFilter::new();
        let (out, events) = feed_all(&mut filter, &[b"\x1b]7501;state=work\x1b[2Jrest"]);
        assert_eq!(out, b"\x1b[2Jrest");
        assert!(events.is_empty());
    }

    #[test]
    fn invalid_reports_are_ignored_entirely() {
        // 未知 state。
        assert_eq!(parse_report(b"state=sleeping"), None);
        // 缺 state。
        assert_eq!(parse_report(b"app=claude"), None);
        // 坏 base64（字符合法、长度不成立）。字符集外的值（如 `msg=@@@`）按规范只是跳过
        // 这一对，报告照收，见 lenient_parsing_follows_the_spec。
        assert_eq!(parse_report(b"state=working:msg=A"), None);
        assert!(parse_report(b"state=working:msg=@@@").unwrap().msg.is_none());
        // msg 里有换行（控制字符）。
        assert_eq!(parse_report(format!("state=working:msg={}", b64("a\nb")).as_bytes()), None);
        // 超长 msg（解码后超 2048）。
        let long = b64(&"x".repeat(2049));
        assert_eq!(parse_report(format!("state=working:msg={long}").as_bytes()), None);
        // 非法 id。
        assert_eq!(parse_report(b"state=working:id=a//b"), None);
        assert_eq!(parse_report(b"state=working:id=a/b/c/d/e/f/g/h/i"), None);
    }

    #[test]
    fn lenient_parsing_follows_the_spec() {
        // 空格剔除、未知键忽略、坏的键值对跳过、重复键取最后一个、填充可省。
        let msg = b64("hi").trim_end_matches('=').to_string();
        let report = parse_report(
            format!(" state = blocked :future=1:kind=question:bad pair:kind=auth:msg={msg}:progress=150").as_bytes(),
        )
        .unwrap();
        assert_eq!(report.state, ReportState::Blocked);
        assert_eq!(report.kind, Some(BlockedKind::Auth));
        assert_eq!(report.msg.as_deref(), Some("hi"));
        assert_eq!(report.progress, None);
        // kind 只在 blocked 时有意义。
        assert_eq!(parse_report(b"state=working:kind=permission").unwrap().kind, None);
        // 未知 kind 视为没写。
        assert_eq!(parse_report(b"state=blocked:kind=coffee").unwrap().kind, None);
    }

    #[test]
    fn bidi_and_invisible_characters_are_removed_from_text() {
        let report =
            parse_report(format!("state=working:msg={}", b64("a\u{202E}b\u{200B}c")).as_bytes()).unwrap();
        assert_eq!(report.msg.as_deref(), Some("abc"));
    }

    #[test]
    fn records_follow_replace_clear_and_inherit_rules() {
        let mut status = ProgramStatus::default();
        let report = |body: &str| parse_report(body.as_bytes()).unwrap();
        status.apply(report("state=working:app=claude-code"), 1);
        status.apply(report("state=working:id=agent/1"), 2);
        status.apply(report("state=working:id=agent/1/tool"), 3);
        status.apply(report("state=done:id=other"), 4);
        assert_eq!(status.app_of("agent/1/tool"), Some("claude-code"));
        // 新报告整条替换：上一条的 app 不保留。
        status.apply(report("state=blocked:kind=permission"), 5);
        assert_eq!(status.root().unwrap().app, None);
        assert_eq!(
            status.session_state(),
            Some((ScreenState::Blocked, Some(BlockedKind::Permission)))
        );
        // clear 带 id：删掉它和它下面的记录，不动兄弟。
        status.apply(report("state=clear:id=agent/1"), 6);
        let ids: Vec<&str> = status.records().iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, vec!["", "other"]);
        // clear 不带 id：全部清掉，会话回到「没有报告」。
        status.apply(report("state=clear"), 7);
        assert!(status.is_empty());
        assert_eq!(status.session_state(), None);
    }

    #[test]
    fn done_error_and_idle_all_mean_the_turn_stopped() {
        for state in ["idle", "done", "error"] {
            let mut status = ProgramStatus::default();
            status.apply(parse_report(format!("state={state}").as_bytes()).unwrap(), 1);
            assert_eq!(status.session_state(), Some((ScreenState::Idle, None)), "{state}");
        }
    }

    /// 真实 CLI 录下的输出流（2026-11-26，Meowo 应答了探测；用户名与时区已脱敏）。
    /// 按读块回放，断言会话状态的变化序列与录制时的真实经过一致，且 7501 序列一字节都
    /// 不外流。录制脚本与原始日志在任务目录 .lead/20261126-program-status/agents/spike。
    mod replay {
        use super::*;
        use BlockedKind::*;
        use ScreenState::*;

        type Step = Option<(ScreenState, Option<BlockedKind>)>;

        fn replay(bytes: &[u8], chunk: usize) -> (Vec<Step>, ProgramStatus, Vec<u8>, usize) {
            let mut filter = StatusFilter::new();
            let mut status = ProgramStatus::default();
            let mut steps: Vec<Step> = Vec::new();
            let mut out = Vec::new();
            let mut probes = 0;
            for piece in bytes.chunks(chunk) {
                let mut events = Vec::new();
                out.extend_from_slice(&filter.feed(piece, &mut events));
                for event in events {
                    match event {
                        StreamEvent::Probe => {
                            probes += 1;
                            continue;
                        }
                        StreamEvent::Report(report) => status.apply(report, 0),
                        StreamEvent::Reset => status.reset(),
                    }
                    let now = status.session_state();
                    if steps.last() != Some(&now) {
                        steps.push(now);
                    }
                }
            }
            (steps, status, out, probes)
        }

        /// `probes`：claude、kimi 先发探测、等应答后才报告；codex 不问，直接报告（规范允许）。
        fn check(bytes: &[u8], probes_expected: bool, expected: &[Step]) -> ProgramStatus {
            let mut last = None;
            // 4096 是常见读块；777 让序列频繁落在块边界上。
            for chunk in [16 * 1024, 4096, 777] {
                let (steps, status, out, probes) = replay(bytes, chunk);
                assert_eq!(steps, expected, "chunk {chunk}");
                assert_eq!(probes >= 1, probes_expected, "探测（chunk {chunk}）");
                assert!(
                    !out.windows(7).any(|w| w == PREFIX),
                    "7501 序列外流了（chunk {chunk}）"
                );
                last = Some(status);
            }
            last.unwrap()
        }

        #[test]
        fn claude_trust_turn_and_interrupt() {
            check(
                include_bytes!("../tests/fixtures/program-status/claude-2.1.332-turn-interrupt.bin"),
                true,
                &[
                    // 信任目录的确认框。
                    Some((Blocked, Some(Permission))),
                    // 确认后 claude 先清空记录再报空闲。
                    None,
                    Some((Idle, None)),
                    // 跑一条命令（done 折成 Idle）。
                    Some((Working, None)),
                    Some((Idle, None)),
                    // 长回合，Esc 中断后报 idle。
                    Some((Working, None)),
                    Some((Idle, None)),
                ],
            );
        }

        #[test]
        fn claude_usage_limit_is_an_error_with_reason() {
            let status = check(
                include_bytes!("../tests/fixtures/program-status/claude-2.1.332-usage-limit.bin"),
                true,
                &[
                    Some((Idle, None)),
                    Some((Working, None)),
                    Some((Idle, None)),
                    Some((Working, None)),
                    Some((Idle, None)),
                ],
            );
            let root = status.root().unwrap();
            assert_eq!(root.state, ReportState::Error);
            assert!(root.msg.as_deref().unwrap().contains("5-hour limit"));
        }

        #[test]
        fn codex_turn() {
            check(
                include_bytes!("../tests/fixtures/program-status/codex-0.169.0-turn.bin"),
                false,
                &[
                    Some((Idle, None)),
                    Some((Working, None)),
                    Some((Idle, None)),
                    Some((Working, None)),
                    Some((Idle, None)),
                ],
            );
        }

        #[test]
        fn codex_approval_is_blocked_on_permission() {
            check(
                include_bytes!("../tests/fixtures/program-status/codex-0.169.0-approval.bin"),
                false,
                &[
                    Some((Idle, None)),
                    Some((Working, None)),
                    Some((Idle, None)),
                    Some((Working, None)),
                    // 审批框（-a on-request -s read-only 下删文件）。
                    Some((Blocked, Some(Permission))),
                    // Esc 拒绝后收尾。
                    Some((Working, None)),
                    Some((Idle, None)),
                ],
            );
        }

        #[test]
        fn kimi_trust_and_turn() {
            check(
                include_bytes!("../tests/fixtures/program-status/kimi-2.7.3-trust-turn.bin"),
                true,
                &[
                    Some((Blocked, Some(Permission))),
                    Some((Idle, None)),
                    None,
                    Some((Idle, None)),
                    Some((Working, None)),
                    Some((Idle, None)),
                ],
            );
        }

        #[test]
        fn kimi_question_is_blocked_on_question() {
            check(
                include_bytes!("../tests/fixtures/program-status/kimi-2.7.3-question.bin"),
                true,
                &[
                    Some((Idle, None)),
                    None,
                    Some((Idle, None)),
                    Some((Working, None)),
                    Some((Blocked, Some(Question))),
                    Some((Working, None)),
                    Some((Idle, None)),
                ],
            );
        }
    }

    #[test]
    fn record_count_is_capped_by_evicting_the_oldest() {
        let mut status = ProgramStatus::default();
        for index in 0..(MAX_RECORDS + 10) {
            status.apply(
                parse_report(format!("state=working:id=a{index}").as_bytes()).unwrap(),
                index as i64,
            );
        }
        assert_eq!(status.records().len(), MAX_RECORDS);
        assert!(!status.records().iter().any(|(id, _)| *id == "a0"));
        assert!(status.records().iter().any(|(id, _)| *id == "a265"));
    }
}
