//! 读终端输入框：按插件声明的 [`ComposerSpec`] 在仿真后的整屏上定位输入框、取出里面的字、
//! 判断是空的、有草稿，还是处在别的输入模式。
//!
//! 这是宿主侧**唯一**的认框实现（前端的同款在 `app/src/views/chat/composer.ts`，两边对同一批
//! 实拍帧必须给出同样的结论，样例在 `app/src/test/fixtures/composer-frames.json`）。认框规则
//! 只来自插件声明，这里不写任何一家的字符。

use crate::chat_ui::{ComposerSpec, ComposerWrapEdge};

/// 读屏那一刻的硬件光标（与文字同一帧取）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub visible: bool,
    /// 光标所在格是暗色（SGR dim）：占位文字的画法（claude、codex 实拍）。
    pub dim: bool,
}

/// 草稿为什么不能确认读全（见 [`crate::chat_ui::ComposerFidelity`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Incomplete {
    /// 含粘贴或图片占位，原文不在屏幕上。
    Paste,
    /// 草稿比输入框的可视高度长，没显示全。
    Overflow,
    /// 分不清相邻两行之间是折行还是换行。
    Wrap,
    /// 这一家没有取证过的判定依据，或画面不是取证过的样子。
    Unverified,
}

impl Incomplete {
    /// 与前端 `ComposerDraft.why` 同名。
    pub fn as_str(self) -> &'static str {
        match self {
            Incomplete::Paste => "paste",
            Incomplete::Overflow => "overflow",
            Incomplete::Wrap => "wrap",
            Incomplete::Unverified => "unverified",
        }
    }
}

/// 草稿能不能从屏幕上原样读回来。只有 `Complete` 才允许清掉终端里的原稿；里面是保留行首缩进
/// 与空行的全文（行尾空白屏幕上看不见，不在「原样」之内）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Draft {
    Complete(String),
    Incomplete(Incomplete),
}

/// 认出来的输入框。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerRead {
    /// 输入框里没有用户的字（空白，或只有占位文字）。
    pub empty: bool,
    /// 输入框里的文字（去掉提示符，每行去掉首尾空白；占位文字原样）。只用来判空与比对，
    /// **不是草稿原文**，要搬走草稿用 `draft`。
    pub text: String,
    /// 有草稿时（不空、不在特殊输入模式）：能不能原样读回，以及原样的全文。
    pub draft: Option<Draft>,
    /// 处在哪种特殊输入模式（`ComposerAltMode::id`）；此时 `empty` 恒为 false，调用方先退出模式。
    pub alt_mode: Option<&'static str>,
    /// 输入框首行在屏幕上的行号与输入起点的列号（终端格）。
    pub input_row: usize,
    pub input_col: usize,
    /// 上边框与底边所在行（上边框没有单独一行时等于首行）。
    pub top_row: usize,
    pub bottom_row: usize,
}

/// 占几个终端格，只往宽里估（U+1100 起一律按两格）：估宽了只会更保守地判成「分不清」。
fn width_at_most(text: &str) -> usize {
    text.chars()
        .map(|c| if (c as u32) < 0x1100 { 1 } else { 2 })
        .sum()
}

/// 逐行的原样内容 → 能不能确认这就是草稿全文。规则只来自插件声明的 `fidelity`。
fn judge_draft(
    rows: &[String],
    screen: &[String],
    (top_row, bottom_row): (usize, usize),
    input_col: usize,
    cols: Option<usize>,
    spec: &ComposerSpec,
) -> Draft {
    let unverified = Draft::Incomplete(Incomplete::Unverified);
    let Some(rule) = spec.fidelity else {
        return unverified;
    };
    let Ok(paste) = regex::Regex::new(rule.paste_pattern) else {
        return unverified;
    };
    if rows.iter().any(|row| paste.is_match(row)) {
        return Draft::Incomplete(Incomplete::Paste);
    }
    if let Some(pattern) = rule.overflow_pattern {
        let Ok(overflow) = regex::Regex::new(pattern) else {
            return unverified;
        };
        if screen[top_row..=bottom_row]
            .iter()
            .any(|line| overflow.is_match(line))
        {
            return Draft::Incomplete(Incomplete::Overflow);
        }
    }
    if rule.overflow_rows.is_some_and(|limit| rows.len() >= limit) {
        return Draft::Incomplete(Incomplete::Overflow);
    }
    if rows.len() > 1 {
        // 相邻两行之间是用户敲的换行，还是框宽折出来的：屏幕上没有标记。只有一种情况能确认是换行：
        // 下一行开头那个词（连同它前面的缩进）本来放得进上一行剩下的宽度，CLI 却没有放。
        // 放不进的分不清，判读不全。
        let edge = match rule.wrap_edge {
            ComposerWrapEdge::Border => Some(screen[bottom_row].trim_end().chars().count()),
            ComposerWrapEdge::Terminal => cols,
        };
        let Some(edge) = edge.filter(|edge| *edge > 0) else {
            return unverified;
        };
        let width = edge as isize - input_col as isize - rule.wrap_inset as isize;
        for pair in rows.windows(2) {
            let room = width - width_at_most(&pair[0]) as isize;
            let lead = pair[1].len() - pair[1].trim_start().len();
            let word_end = pair[1][lead..]
                .find(char::is_whitespace)
                .map_or(pair[1].len(), |at| lead + at);
            let head = width_at_most(&pair[1][..word_end]) as isize;
            if room < (1 + head).max(2) {
                return Draft::Incomplete(Incomplete::Wrap);
            }
        }
    }
    let end = rows
        .iter()
        .rposition(|row| !row.is_empty())
        .map_or(0, |last| last + 1);
    Draft::Complete(rows[..end].join("\n"))
}

/// 认不出输入框返回 None。不带终端列数：以终端右沿为界的家（见
/// [`ComposerWrapEdge::Terminal`]）多行草稿一律判读不全。只判空、比对时用它就够。
pub fn read(screen: &[String], cursor: Option<Cursor>, spec: &ComposerSpec) -> Option<ComposerRead> {
    read_with_cols(screen, cursor, None, spec)
}

/// 同 [`read`]，带上终端列数（与文字、光标同一帧取），草稿保真判定才完整。
pub fn read_with_cols(
    screen: &[String],
    cursor: Option<Cursor>,
    cols: Option<usize>,
    spec: &ComposerSpec,
) -> Option<ComposerRead> {
    let re = |pattern: &str| regex::Regex::new(pattern).ok();
    let top = re(spec.top_pattern)?;
    let bottom = re(spec.bottom_pattern)?;
    let row = re(spec.row_pattern)?;
    let mut end = screen.iter().rposition(|line| bottom.is_match(line))?;
    let bottom_row = end;
    let mut start;
    let mut body: Vec<&String>;
    let top_row;
    if spec.prompt_footer {
        // footer 与正文之间有空行。不能只取最后一个非空行，否则多行尾部会遮掉真正的 prompt。
        while end > 0 && screen[end - 1].trim().is_empty() {
            end -= 1;
        }
        let mut at = end.checked_sub(1)?;
        loop {
            if top.is_match(&screen[at]) {
                break;
            }
            if !screen[at].trim().is_empty() && !row.is_match(&screen[at]) {
                return None;
            }
            at = at.checked_sub(1)?;
        }
        start = at;
        top_row = at;
        body = screen[start..end].iter().collect();
    } else {
        start = end;
        while start > 0 && row.is_match(&screen[start - 1]) {
            start -= 1;
        }
        body = screen[start..end].iter().collect();
        // OpenCode 顶线也是空内容行；从完整连续框体起点找，不能跳过正文去找最后一个空行。
        if body.first().is_some_and(|line| top.is_match(line)) {
            body.remove(0);
            top_row = start;
            start += 1;
        } else if start == 0 || !top.is_match(&screen[start - 1]) {
            return None;
        } else {
            top_row = start - 1;
        }
        if spec.tail_rows > 0 {
            let tail = re(spec.tail_pattern?)?;
            if body.len() <= spec.tail_rows || !tail.is_match(body.last()?) {
                return None;
            }
            body.truncate(body.len() - spec.tail_rows);
        }
    }
    if body.is_empty() {
        return None;
    }
    let mut text = Vec::new();
    // 每行捕获到的原样内容与它在整行里从第几个字符开始：读草稿原文时按输入起点那一列对齐，
    // 保住行首缩进。
    let mut captured: Vec<(&str, usize)> = Vec::new();
    for line in &body {
        if spec.prompt_footer && line.trim().is_empty() {
            text.push(String::new());
            captured.push(("", 0));
            continue;
        }
        let captures = row.captures(line)?;
        let group = captures.get(1)?;
        text.push(group.as_str().trim().to_string());
        captured.push((group.as_str(), line[..group.start()].chars().count()));
    }
    // 特殊输入模式先于提示符判断：shell 模式的提示符本来就不是普通的那个。
    let mut alt_mode = None;
    for mode in spec.alt_modes {
        let by_prompt = mode
            .prompt_pattern
            .and_then(re)
            .is_some_and(|pattern| pattern.is_match(&text[0]));
        let by_below = mode.below_pattern.and_then(re).is_some_and(|pattern| {
            screen[bottom_row + 1..]
                .iter()
                .any(|line| pattern.is_match(line))
        });
        if by_prompt || by_below {
            alt_mode = Some(mode.id);
            break;
        }
    }
    // prompt_prefix 可列几个提示符，用 `|` 分隔（Gemini 平常 `>`、YOLO `*`）；空串 = 没有提示符。
    let prefix = spec
        .prompt_prefix
        .split('|')
        .find(|prefix| text[0].starts_with(prefix));
    let Some(prefix) = prefix else {
        // 提示符对不上：只有认出是特殊模式（shell 的 `!`）时才算认出了框。
        return alt_mode.map(|id| ComposerRead {
            empty: false,
            draft: None,
            text: text.join("\n").trim().to_string(),
            alt_mode: Some(id),
            input_row: start,
            input_col: 0,
            top_row,
            bottom_row,
        });
    };
    let input_col = re(spec.input_start_pattern)?
        .captures(body[0])?
        .get(1)?
        .as_str()
        .chars()
        .count();
    text[0] = text[0][prefix.len()..].trim_start().to_string();
    let content = text.join("\n");
    let content = content.trim();
    let matches_empty = re(spec.empty_pattern)?.is_match(content);
    let empty = if alt_mode.is_some() {
        false
    } else if content.is_empty() {
        true
    } else {
        match cursor.filter(|c| c.visible && c.row >= start && c.row < start + body.len()) {
            // 光标在输入框里：停在输入起点才可能是占位（暗色格，或文字对得上占位）；
            // 在别处就是用户真打出来的字，哪怕和占位一模一样。
            Some(c) => c.row == start && c.col == input_col && (c.dim || matches_empty),
            // 没有光标证据（隐藏，或停在别处）：只能看文字。
            None => matches_empty,
        }
    };
    let draft = (!empty && alt_mode.is_none()).then(|| {
        // 各行从输入起点那一列起才是用户的字；那一列之前只能是提示符（首行）或空白（续行），
        // 否则这个画面不是取证过的样子，不敢说读全了。
        let mut aligned = true;
        let rows: Vec<String> = captured
            .iter()
            .enumerate()
            .map(|(index, (value, at))| {
                let skip = input_col.checked_sub(*at).unwrap_or_else(|| {
                    aligned = false;
                    0
                });
                let mut chars = value.chars();
                let before: String = chars.by_ref().take(skip).collect();
                if index > 0 && !before.trim().is_empty() {
                    aligned = false;
                }
                chars.collect::<String>().trim_end().to_string()
            })
            .collect();
        if aligned {
            judge_draft(&rows, screen, (top_row, bottom_row), input_col, cols, spec)
        } else {
            Draft::Incomplete(Incomplete::Unverified)
        }
    });
    Some(ComposerRead {
        empty,
        draft,
        text: content.to_string(),
        alt_mode,
        input_row: start,
        input_col,
        top_row,
        bottom_row,
    })
}

#[cfg(test)]
mod tests;
