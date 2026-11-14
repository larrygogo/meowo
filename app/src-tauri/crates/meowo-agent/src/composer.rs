//! 读终端输入框：按插件声明的 [`ComposerSpec`] 在仿真后的整屏上定位输入框、取出里面的字、
//! 判断是空的、有草稿，还是处在别的输入模式。
//!
//! 这是宿主侧**唯一**的认框实现（前端的同款在 `app/src/views/chat/composer.ts`，两边对同一批
//! 实拍帧必须给出同样的结论，样例在 `app/src/test/fixtures/composer-frames.json`）。认框规则
//! 只来自插件声明，这里不写任何一家的字符。

use crate::chat_ui::ComposerSpec;

/// 读屏那一刻的硬件光标（与文字同一帧取）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub visible: bool,
    /// 光标所在格是暗色（SGR dim）：占位文字的画法（claude、codex 实拍）。
    pub dim: bool,
}

/// 认出来的输入框。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerRead {
    /// 输入框里没有用户的字（空白，或只有占位文字）。
    pub empty: bool,
    /// 输入框里的文字（去掉提示符与首尾空白；占位文字原样）。
    pub text: String,
    /// 处在哪种特殊输入模式（`ComposerAltMode::id`）；此时 `empty` 恒为 false，调用方先退出模式。
    pub alt_mode: Option<&'static str>,
    /// 输入框首行在屏幕上的行号与输入起点的列号（终端格）。
    pub input_row: usize,
    pub input_col: usize,
    /// 上边框与底边所在行（上边框没有单独一行时等于首行）。
    pub top_row: usize,
    pub bottom_row: usize,
}

/// 认不出输入框返回 None。
pub fn read(screen: &[String], cursor: Option<Cursor>, spec: &ComposerSpec) -> Option<ComposerRead> {
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
    for line in &body {
        if spec.prompt_footer && line.trim().is_empty() {
            text.push(String::new());
            continue;
        }
        let captures = row.captures(line)?;
        text.push(captures.get(1)?.as_str().trim().to_string());
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
    Some(ComposerRead {
        empty,
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
