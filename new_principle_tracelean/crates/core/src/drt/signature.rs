/// @implements REQ-DRT-RUST.params_from_source
/// @implements REQ-DRT-BIND.call_only
pub fn parameters(source: &str, symbol: &str) -> Option<Vec<String>> {
    let source = &blanked(source, &['"'], true);
    if declarations_in(source, symbol) > 1 {
        return None;
    }
    let open = signature_open_paren(source, symbol)?;
    let close = matching(source, open, '(', ')')?;
    let params = &source[open + 1..close];

    let mut names = Vec::new();
    for part in split_top_level(params, ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let head = part.trim_start_matches('&').trim_start();
        let head = head.strip_prefix("mut ").unwrap_or(head).trim_start();
        if head == "self" || head.starts_with("self:") {
            continue;
        }
        let name = match part.split_once(':') {
            Some((lhs, _)) => lhs.trim(),
            None => part,
        };
        let name = name.strip_prefix("mut ").unwrap_or(name).trim();
        if name.is_empty() || !is_identifier(name) {
            return None;
        }
        names.push(name.to_string());
    }
    Some(names)
}

/// @implements REQ-DRT-RUST.params_from_source
/// @drt REQ-DRT-RUST.params_from_source
pub fn parameters_of(source: String, symbol: String) -> Option<Vec<String>> {
    parameters(&source, &symbol)
}

/// @implements REQ-DRT-BIND.call_only
pub fn declarations(source: &str, symbol: &str) -> usize {
    declarations_in(&blanked(source, &['"'], true), symbol)
}

fn declarations_in(source: &str, symbol: &str) -> usize {
    let mut count = 0;
    let mut from = 0usize;
    while let Some(open) = signature_open_paren(&source[from..], symbol) {
        count += 1;
        from += open + 1;
    }
    count
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Code,
    Line,
    Block,
    Quoted(char),
}

pub(crate) fn blanked(source: &str, quotes: &[char], char_literals: bool) -> String {
    let cs: Vec<char> = source.chars().collect();
    let keep_newline = |c: char| if c == '\n' { '\n' } else { ' ' };
    let mut out = String::with_capacity(source.len());
    let mut mode = Mode::Code;
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        let next = cs.get(i + 1).copied();
        let (emitted, consumed, after): (Vec<char>, usize, Mode) = match mode {
            Mode::Code if c == '/' && next == Some('/') => (vec![' ', ' '], 2, Mode::Line),
            Mode::Code if c == '/' && next == Some('*') => (vec![' ', ' '], 2, Mode::Block),
            Mode::Code => {
                let third = cs.get(i + 2).copied();
                let fourth = cs.get(i + 3).copied();
                if char_literals && c == '\'' && next == Some('\\') && fourth == Some('\'') && third.is_some() {
                    (cs[i..i + 4].to_vec(), 4, Mode::Code)
                } else if char_literals && c == '\'' && next.is_some() && third == Some('\'') {
                    (cs[i..i + 3].to_vec(), 3, Mode::Code)
                } else if quotes.contains(&c) {
                    (vec![' '], 1, Mode::Quoted(c))
                } else {
                    (vec![c], 1, Mode::Code)
                }
            }
            Mode::Line if c == '\n' => (vec!['\n'], 1, Mode::Code),
            Mode::Line => (vec![' '], 1, Mode::Line),
            Mode::Block if c == '*' && next == Some('/') => (vec![' ', ' '], 2, Mode::Code),
            Mode::Block => (vec![keep_newline(c)], 1, Mode::Block),
            Mode::Quoted(q) if c == '\\' && next.is_some() => (vec![' ', ' '], 2, Mode::Quoted(q)),
            Mode::Quoted(q) if c == q => (vec![' '], 1, Mode::Code),
            Mode::Quoted(q) => (vec![keep_newline(c)], 1, Mode::Quoted(q)),
        };
        out.extend(emitted);
        i += consumed;
        mode = after;
    }
    out
}

fn signature_open_paren(source: &str, symbol: &str) -> Option<usize> {
    let mut from = 0usize;
    while let Some(found) = source[from..].find("fn ") {
        let at = from + found;
        from = at + 3;
        let rest = &source[at + 3..];
        let name_len = rest
            .char_indices()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
            .count();
        if &rest[..name_len] != symbol {
            continue;
        }
        let mut cursor = at + 3 + name_len;
        let bytes = source.as_bytes();
        while cursor < bytes.len() && (bytes[cursor] as char).is_whitespace() {
            cursor += 1;
        }
        if cursor < bytes.len() && bytes[cursor] == b'<' {
            cursor = matching(source, cursor, '<', '>')? + 1;
            while cursor < bytes.len() && (bytes[cursor] as char).is_whitespace() {
                cursor += 1;
            }
        }
        if cursor < bytes.len() && bytes[cursor] == b'(' {
            return Some(cursor);
        }
    }
    None
}

pub(crate) fn matching(source: &str, start: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0i32;
    let bytes = source.as_bytes();
    for (offset, ch) in source[start..].char_indices() {
        let at = start + offset;
        if ch == close && close == '>' && at > 0 && bytes[at - 1] == b'-' {
            continue;
        }
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(at);
            }
        }
    }
    None
}

pub(crate) fn split_top_level(text: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (offset, ch) in text.char_indices() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            _ => {}
        }
        if ch == '>' && offset > 0 && text.as_bytes()[offset - 1] == b'-' {
            depth += 1;
        }
        if ch == sep && depth == 0 {
            parts.push(&text[start..offset]);
            start = offset + ch.len_utf8();
        }
    }
    parts.push(&text[start..]);
    parts
}

pub(crate) fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(src: &str, sym: &str) -> Option<Vec<String>> {
        parameters(src, sym)
    }

    /// @tests REQ-DRT-RUST.params_from_source
    #[test]
    fn plain_function() {
        assert_eq!(
            names("pub fn assurance(records: Vec<Record>, bond: Bond) -> Level {}", "assurance"),
            Some(vec!["records".into(), "bond".into()])
        );
    }

    #[test]
    fn no_parameters_differs_from_no_function() {
        assert_eq!(names("fn tick() {}", "tick"), Some(vec![]));
        assert_eq!(names("fn tick() {}", "tock"), None);
    }

    #[test]
    fn generic_parameters_are_skipped() {
        assert_eq!(
            names("fn pick<T: Ord>(items: Vec<T>, n: usize) -> T {}", "pick"),
            Some(vec!["items".into(), "n".into()])
        );
    }

    #[test]
    fn parenthesis_inside_generics() {
        assert_eq!(
            names("fn apply<F: Fn(u8) -> u8>(f: F, x: u8) -> u8 {}", "apply"),
            Some(vec!["f".into(), "x".into()])
        );
    }

    #[test]
    fn commas_inside_types_do_not_split() {
        assert_eq!(
            names("fn merge(a: BTreeMap<String, u32>, b: (u8, u8)) {}", "merge"),
            Some(vec!["a".into(), "b".into()])
        );
    }

    #[test]
    fn self_is_not_a_parameter() {
        assert_eq!(names("fn len(&self, pad: usize) {}", "len"), Some(vec!["pad".into()]));
        assert_eq!(names("fn take(mut self, n: u8) {}", "take"), Some(vec!["n".into()]));
    }

    #[test]
    fn mut_binding_is_stripped() {
        assert_eq!(names("fn f(mut a: u8) {}", "f"), Some(vec!["a".into()]));
    }

    #[test]
    fn pattern_parameter_is_refused() {
        assert_eq!(names("fn f((a, b): (u8, u8)) {}", "f"), None);
    }

    #[test]
    fn similar_names_do_not_collide() {
        let src = "fn assure(a: u8) {}\nfn assurance(records: Vec<R>) {}";
        assert_eq!(names(src, "assurance"), Some(vec!["records".into()]));
    }

    /// @tests REQ-DRT-BIND.call_only
    #[test]
    fn a_name_declared_twice_is_counted_twice() {
        let src = "impl S {\n    pub fn blocks(&self) -> bool { true }\n}\npub fn blocks(state: S) -> bool { state.blocks() }\n";
        assert_eq!(declarations(src, "blocks"), 2);
        assert_eq!(declarations(src, "absent"), 0);
        assert_eq!(declarations("pub fn f(x: u8) {}", "f"), 1);
    }

    /// @tests REQ-DRT-RUST.params_from_source
    #[test]
    fn a_commented_signature_does_not_win_and_two_are_refused() {
        let two = || Some(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(names("// fn f(b: u8, a: u8)\nfn f(a: u8, b: u8) {}", "f"), two());
        assert_eq!(names("/* fn f(b: u8, a: u8) */ fn f(a: u8, b: u8) {}", "f"), two());
        assert_eq!(names("const S: &str = \"fn f(b: u8)\";\nfn f(a: u8, b: u8) {}", "f"), two());
        assert_eq!(declarations("// fn f(b: u8)\nfn f(a: u8) {}", "f"), 1);
        assert_eq!(names("fn f(a: u8) {}\nimpl S { fn f(&self, b: u8) {} }", "f"), None);
    }
}
