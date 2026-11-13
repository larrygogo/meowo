//! 仅为managed原生成功工具事实提供保守目标识别；未知脚本不能靠提到.lead就认领。

enum Token {
    Word(String),
    Out(bool),
    In,
}

fn lex(line: &str) -> Option<Vec<Token>> {
    let mut tokens = vec![];
    let mut word = String::new();
    let mut quoted = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(quote) = quoted {
            if c == quote {
                quoted = None;
            } else if c == '\\' && quote == '"' && chars.peek() == Some(&'"') {
                word.push(chars.next()?);
            } else {
                word.push(c);
            }
            continue;
        }
        match c {
            '\'' | '"' => quoted = Some(c),
            ';' | '|' | '&' | '`' => return None,
            '#' if word.is_empty() => break,
            c if c.is_whitespace() => {
                if !word.is_empty() {
                    tokens.push(Token::Word(std::mem::take(&mut word)));
                }
            }
            '>' => {
                let fd = if !word.is_empty() && word.chars().all(|c| c.is_ascii_digit()) {
                    let fd = word.parse::<u8>().ok();
                    word.clear();
                    fd
                } else {
                    None
                };
                if !word.is_empty() {
                    tokens.push(Token::Word(std::mem::take(&mut word)));
                }
                if chars.peek() == Some(&'>') {
                    chars.next();
                }
                tokens.push(Token::Out(fd.is_none() || fd == Some(1)));
            }
            '<' => {
                if !word.is_empty() {
                    tokens.push(Token::Word(std::mem::take(&mut word)));
                }
                tokens.push(Token::In);
            }
            _ => word.push(c),
        }
        if tokens.len() > 256 || word.len() > 16384 {
            return None;
        }
    }
    if quoted.is_some() {
        return None;
    }
    if !word.is_empty() {
        tokens.push(Token::Word(word));
    }
    Some(tokens)
}

pub(crate) fn literal_lead_path(value: &str) -> Option<String> {
    if value.contains(['$', '*', '?', '`', '(', ')']) {
        return None;
    }
    let path = value.replace('\\', "/");
    (path.starts_with(".lead/") || path.contains("/.lead/")).then_some(path)
}

/// 只返回可证明的字面写入目标，不能把cp/cat的读取源交给旧后验再误认领。
pub(crate) fn targets(command: &str) -> Vec<String> {
    if !command.contains(".lead") || command.len() > 64 * 1024 || command.contains("$(") {
        return vec![];
    }
    let line = command.lines().next().unwrap_or("");
    if command.lines().nth(1).is_some() && !line.contains("<<") {
        return vec![];
    }
    let Some(tokens) = lex(line) else {
        return vec![];
    };
    let mut out = vec![];
    for pair in tokens.windows(2) {
        if let [Token::Out(true), Token::Word(value)] = pair {
            if let Some(path) = literal_lead_path(value) {
                out.push(path);
            }
        }
    }
    let words: Vec<&str> = tokens
        .iter()
        .take_while(|t| !matches!(t, Token::In | Token::Out(_)))
        .filter_map(|t| {
            if let Token::Word(w) = t {
                Some(w.as_str())
            } else {
                None
            }
        })
        .collect();
    if let Some(program) = words.first() {
        match program.to_ascii_lowercase().as_str() {
            "set-content" | "add-content" | "out-file" => {
                if words.iter().any(|w| {
                    w.to_ascii_lowercase().starts_with("-whatif")
                        || w.to_ascii_lowercase().starts_with("-confirm")
                }) {
                    return vec![];
                }
                let target = words
                    .windows(2)
                    .find(|p| {
                        ["-path", "-literalpath", "-filepath"]
                            .contains(&p[0].to_ascii_lowercase().as_str())
                    })
                    .map(|p| p[1])
                    .or_else(|| words.get(1).copied().filter(|w| !w.starts_with('-')));
                if let Some(path) = target.and_then(literal_lead_path) {
                    out.push(path);
                }
            }
            "cp" | "mv" => {
                if words.len() != 3 || words.iter().skip(1).any(|w| w.starts_with('-')) {
                    return out;
                }
                if let Some(path) = words.last().and_then(|p| literal_lead_path(p)) {
                    out.push(path);
                }
            }
            "tee" => {
                for word in words.iter().skip(1).filter(|w| !w.starts_with('-')) {
                    if let Some(path) = literal_lead_path(word) {
                        out.push(path);
                    }
                }
            }
            "sed" if words.iter().any(|w| *w == "-i" || w.starts_with("-i")) => {
                if let Some(path) = words.last().and_then(|p| literal_lead_path(p)) {
                    out.push(path);
                }
            }
            _ => {}
        }
    }
    out.sort();
    out.dedup();
    out.truncate(16);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_literal_write_destinations_are_evidence() {
        for (command, writes) in [
            ("cat .lead/read/SPEC.md", false),
            ("cp .lead/read/SPEC.md /tmp/copy", false),
            ("cp -n /tmp/source .lead/real/SPEC.md", false),
            (
                "Set-Content -Path .lead/real/SPEC.md -Value x -WhatIf",
                false,
            ),
            ("cat .lead/read/SPEC.md > /tmp/copy", false),
            ("echo '> .lead/fake/progress.md'", false),
            ("python -c \"print('.lead/fake/progress.md')\"", false),
            ("echo ok # > .lead/fake/progress.md", false),
            ("echo ok > .lead/real/progress.md", true),
            ("cp /tmp/source .lead/real/SPEC.md", true),
            (
                "Set-Content -Path '.lead/real/SPEC.md' -Value 'literal | > text'",
                true,
            ),
            ("cat > '.lead/real/SPEC.md' <<'EOF'\ntext\nEOF", true),
            ("echo ok > $DIR/.lead/unknown/progress.md", false),
            ("false && echo ok > .lead/fake/progress.md", false),
        ] {
            assert_eq!(!targets(command).is_empty(), writes, "{command}");
        }
        assert_eq!(
            targets("cat .lead/read/SPEC.md > .lead/written/SPEC.md"),
            vec![".lead/written/SPEC.md"]
        );
    }
}
