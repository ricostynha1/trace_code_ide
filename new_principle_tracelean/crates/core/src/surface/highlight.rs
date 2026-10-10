//! Syntax highlighting: which tokens of a file are keywords, strings, comments.
//!
//! Done here, in the core, so that a frontend colours a token by the kind it
//! is handed and never decides for itself what a keyword is
//! (`REQ-VIEW.structure_over_text`). The queries are the first TraceLean's
//! (`assets/languages/<lang>/highlights.scm`), so the two colour code alike;
//! what colour a kind is belongs to `assets/theme.json`.
//!
//! @implements REQ-SHOW.file_from_text

use tree_sitter::StreamingIterator;

use crate::surface::produce::Mark;
use crate::surface::view::{Role, TokenKind};
use crate::trace::anchor::Lang;

fn query_source(lang: Lang) -> &'static str {
    match lang {
        Lang::Rust => include_str!("../../../../assets/languages/rust/highlights.scm"),
        Lang::Lean4 => include_str!("../../../../assets/languages/lean4/highlights.scm"),
    }
}

/// What kind of token a capture name means. Names are the usual tree-sitter
/// ones, read from the most specific part outwards; a name with no kind here
/// (a variable, a bracket) is left as plain text.
pub fn kind_of(capture: &str) -> Option<TokenKind> {
    Some(match capture {
        "string.escape" => TokenKind::Escape,
        "function.macro" => TokenKind::MacroCall,
        "constant.builtin" => TokenKind::Constant,
        _ => match capture.split('.').next().unwrap_or("") {
            "keyword" => TokenKind::Keyword,
            "string" => TokenKind::String,
            "number" => TokenKind::Number,
            "constant" => TokenKind::Constant,
            "comment" => TokenKind::Comment,
            "type" => TokenKind::Type,
            "function" => TokenKind::Function,
            "operator" => TokenKind::Operator,
            "property" => TokenKind::Property,
            _ => return None,
        },
    })
}

// Each language's query, compiled once rather than on every key.
thread_local! {
    static QUERIES: std::cell::RefCell<Vec<(Lang, std::rc::Rc<tree_sitter::Query>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn query_for(lang: Lang, language: &tree_sitter::Language) -> Option<std::rc::Rc<tree_sitter::Query>> {
    QUERIES.with(|held| {
        if let Some((_, query)) = held.borrow().iter().find(|(l, _)| *l == lang) {
            return Some(query.clone());
        }
        let query = std::rc::Rc::new(tree_sitter::Query::new(language, query_source(lang)).ok()?);
        held.borrow_mut().push((lang, query.clone()));
        Some(query)
    })
}

/// The tokens of `text`, in order and never overlapping, as marks a file
/// buffer is produced from. A language without a grammar, or a query this
/// grammar does not accept, gives no tokens: the file is then shown as text,
/// which is what it is.
pub fn tokens(text: &str, lang: Option<Lang>) -> Vec<Mark> {
    let Some(lang) = lang else { return Vec::new() };
    let language = lang.language();
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else { return Vec::new() };
    let Some(query) = query_for(lang, &language) else { return Vec::new() };
    let names = query.capture_names();
    let mut cursor = tree_sitter::QueryCursor::new();
    let mut captures = cursor.captures(&query, tree.root_node(), text.as_bytes());
    let mut found: Vec<(usize, usize, TokenKind)> = Vec::new();
    while let Some((matched, index)) = captures.next() {
        let capture = matched.captures[*index];
        if let Some(kind) = kind_of(names[capture.index as usize]) {
            found.push((capture.node.start_byte(), capture.node.end_byte(), kind));
        }
    }
    // The first capture of a region wins, and nothing may overlap: a span
    // inside another is dropped, as a buffer's spans never overlap.
    found.sort_by_key(|(start, end, _)| (*start, std::cmp::Reverse(*end)));
    let mut marks = Vec::new();
    let mut reached = 0usize;
    // Byte offsets from the parser, character offsets in a buffer.
    let mut chars = 0usize;
    let mut byte = 0usize;
    let mut to_chars = |target: usize| -> usize {
        while byte < target && byte < text.len() {
            let width = text[byte..].chars().next().map(char::len_utf8).unwrap_or(1);
            byte += width;
            chars += 1;
        }
        chars
    };
    for (start, end, kind) in found {
        if start < reached || end <= start {
            continue;
        }
        let from = to_chars(start);
        let to = to_chars(end);
        marks.push(Mark { start: from, stop: to, role: Role::Token { kind } });
        reached = end;
    }
    let chars: Vec<char> = text.chars().collect();
    marks.into_iter().flat_map(|mark| references(&chars, mark)).collect()
}

/// A comment, with every requirement it names cut out of it as a requirement
/// span — so the requirement an annotation in a doc comment names opens when
/// it is clicked.
///
/// A name is two or more capitals, then `-` and capitals or digits as many
/// times as it likes (`REQ-DRT-PROTO`, `ARCH-NO-DRIVING`), then optionally
/// `.` and a lower-case clause key.
///
/// @implements REQ-SHOW.references_are_links
fn references(text: &[char], mark: Mark) -> Vec<Mark> {
    if mark.role != (Role::Token { kind: TokenKind::Comment }) {
        return vec![mark];
    }
    let mut out = Vec::new();
    let mut from = mark.start;
    for (at, end) in names(text, mark.start, mark.stop) {
        if from < at {
            out.push(Mark { start: from, stop: at, role: mark.role });
        }
        out.push(Mark { start: at, stop: end, role: Role::Requirement });
        from = end;
    }
    if from < mark.stop {
        out.push(Mark { start: from, stop: mark.stop, role: mark.role });
    }
    out
}

/// A Markdown document's marks: headings, inline code, the keys of a leading
/// `---` frontmatter block, and every requirement name outside code — so a
/// requirement document reads as one, and `refines: [REQ-VIEW]` opens what it
/// refines. No grammar: Markdown is read a line at a time.
pub fn markdown(text: &str) -> Vec<Mark> {
    let chars: Vec<char> = text.chars().collect();
    let mut marks = Vec::new();
    let mut start = 0usize;
    let mut in_front = false;
    let mut fence = false;
    for (n, line) in text.split('\n').enumerate() {
        let line_chars: Vec<char> = line.chars().collect();
        let stop = start + line_chars.len();
        let trimmed = line.trim();
        if trimmed == "---" && (n == 0 || in_front) {
            in_front = n == 0;
        } else if trimmed.starts_with("```") {
            fence = !fence;
            marks.push(Mark { start, stop, role: Role::Token { kind: TokenKind::String } });
        } else if fence {
            if stop > start {
                marks.push(Mark { start, stop, role: Role::Token { kind: TokenKind::String } });
            }
        } else if trimmed.starts_with('#') && !in_front {
            // A token rather than a heading role: a heading role carries the
            // trace actions, and a click into a title is for typing.
            marks.push(Mark { start, stop, role: Role::Token { kind: TokenKind::Heading } });
        } else {
            let mut from = start;
            if in_front {
                if let Some(colon) = line_chars.iter().position(|c| *c == ':') {
                    let key_start = start + line_chars.iter().take_while(|c| c.is_whitespace()).count();
                    if key_start < start + colon {
                        marks.push(Mark { start: key_start, stop: start + colon, role: Role::Token { kind: TokenKind::Property } });
                    }
                    from = start + colon;
                }
            }
            // Inline code, bold, italic and links, then names in what is none
            // of those.
            let mut at = from;
            let mut plain = from;
            while at < stop {
                if let Some((close, kind)) = inline(&chars, at, stop) {
                    marks.extend(names(&chars, plain, at).into_iter().map(|(a, b)| Mark { start: a, stop: b, role: Role::Requirement }));
                    marks.push(Mark { start: at, stop: close, role: Role::Token { kind } });
                    at = close;
                    plain = at;
                    continue;
                }
                at += 1;
            }
            marks.extend(names(&chars, plain, stop).into_iter().map(|(a, b)| Mark { start: a, stop: b, role: Role::Requirement }));
        }
        start = stop + 1;
    }
    marks
}

/// The inline Markdown that opens at `at` — `` `code` ``, `**bold**`,
/// `*italic*` or `_italic_`, `[text](target)` — as where it ends and what it
/// is; nothing when what opens there is never closed on the line.
fn inline(chars: &[char], at: usize, stop: usize) -> Option<(usize, TokenKind)> {
    let find = |from: usize, c: char| (from..stop).find(|i| chars[*i] == c);
    match chars[at] {
        '`' => find(at + 1, '`').map(|close| (close + 1, TokenKind::String)),
        '*' if at + 1 < stop && chars[at + 1] == '*' => (at + 2..stop.saturating_sub(1))
            .find(|i| chars[*i] == '*' && chars[*i + 1] == '*' && *i > at + 2)
            .map(|close| (close + 2, TokenKind::Bold)),
        // An italic opens on a word, so `a * b` and `snake_case` stay text.
        '*' | '_' if at + 1 < stop && !chars[at + 1].is_whitespace() && (at == 0 || !chars[at - 1].is_alphanumeric()) => {
            let mark = chars[at];
            find(at + 1, mark)
                .filter(|close| *close > at + 1 && !chars[close - 1].is_whitespace() && (close + 1 >= stop || !chars[close + 1].is_alphanumeric()))
                .map(|close| (close + 1, TokenKind::Italic))
        }
        '[' => {
            let close = find(at + 1, ']')?;
            if close + 1 < stop && chars[close + 1] == '(' {
                find(close + 2, ')').map(|end| (end + 1, TokenKind::Link))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Every requirement name in characters `start` to `stop` of `text`, as
/// character ranges: `names` taking what it reads by value, as a generated
/// runner calls it (ADR-0010). A range past the end is cut at the end.
///
/// @implements REQ-SHOW.references_are_links
/// @drt REQ-SHOW.references_are_links
pub fn requirement_names(text: String, start: usize, stop: usize) -> Vec<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let stop = stop.min(chars.len());
    names(&chars, start.min(stop), stop)
}

/// Every requirement name between `start` and `stop`, as character ranges.
pub(crate) fn names(text: &[char], start: usize, stop: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut at = start;
    while at < stop {
        let boundary = at == start || !text[at - 1].is_ascii_alphanumeric();
        match boundary.then(|| reference_at(text, at, stop)).flatten() {
            Some(end) => {
                out.push((at, end));
                at = end;
            }
            None => at += 1,
        }
    }
    out
}

/// Where a requirement's name starting at `at` ends, if one does.
fn reference_at(text: &[char], at: usize, stop: usize) -> Option<usize> {
    let upper = |i: usize| i < stop && text[i].is_ascii_uppercase();
    let mut i = at;
    while upper(i) {
        i += 1;
    }
    if i - at < 2 {
        return None;
    }
    let mut groups = 0;
    while i + 1 < stop && text[i] == '-' && (text[i + 1].is_ascii_uppercase() || text[i + 1].is_ascii_digit()) {
        i += 1;
        while i < stop && (text[i].is_ascii_uppercase() || text[i].is_ascii_digit()) {
            i += 1;
        }
        groups += 1;
    }
    if groups == 0 {
        return None;
    }
    if i + 1 < stop && text[i] == '.' && text[i + 1].is_ascii_lowercase() {
        i += 1;
        while i < stop && (text[i].is_ascii_lowercase() || text[i].is_ascii_digit() || text[i] == '_') {
            i += 1;
        }
    }
    // A name runs into nothing: `REQ-Xyz` is not one.
    if i < stop && text[i].is_ascii_alphanumeric() {
        return None;
    }
    Some(i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str, lang: Lang) -> Vec<(String, TokenKind)> {
        let chars: Vec<char> = text.chars().collect();
        tokens(text, Some(lang))
            .into_iter()
            .map(|m| {
                let Role::Token { kind } = m.role else { panic!("not a token") };
                (chars[m.start..m.stop].iter().collect(), kind)
            })
            .collect()
    }

    #[test]
    fn rust_keywords_strings_and_comments_are_told_apart() {
        let found = kinds("// note\nfn é() -> u8 { let s = \"hi\"; 1 }", Lang::Rust);
        assert!(found.contains(&("// note".into(), TokenKind::Comment)), "{found:?}");
        assert!(found.contains(&("fn".into(), TokenKind::Keyword)), "{found:?}");
        assert!(found.contains(&("\"hi\"".into(), TokenKind::String)), "{found:?}");
        assert!(found.contains(&("u8".into(), TokenKind::Type)), "{found:?}");
    }

    #[test]
    fn lean_is_highlighted_too() {
        let found = kinds("def f : Nat := 1 -- c\n", Lang::Lean4);
        assert!(found.iter().any(|(t, k)| t == "def" && *k == TokenKind::Keyword), "{found:?}");
    }

    /// @tests REQ-SHOW.references_are_links
    #[test]
    fn a_requirement_named_in_a_comment_is_its_own_span() {
        let text = "/// @implements REQ-SHOW.core_produces and ARCH-NO-DRIVING\nfn a() {}";
        let chars: Vec<char> = text.chars().collect();
        let marks = tokens(text, Some(Lang::Rust));
        let named: Vec<String> = marks
            .iter()
            .filter(|m| m.role == Role::Requirement)
            .map(|m| chars[m.start..m.stop].iter().collect())
            .collect();
        assert_eq!(named, vec!["REQ-SHOW.core_produces".to_string(), "ARCH-NO-DRIVING".to_string()]);
        assert!(marks.windows(2).all(|w| w[0].stop <= w[1].start));
        let plain: Vec<char> = "// NOT-a or ABC or X-1".chars().collect();
        let comment = Mark { start: 0, stop: plain.len(), role: Role::Token { kind: TokenKind::Comment } };
        assert_eq!(references(&plain, comment.clone()), vec![comment]);
    }

    /// A requirement document reads as one: its keys, its headings, its code,
    /// and the requirements it names, each a span of its own.
    #[test]
    fn markdown_marks_headings_keys_code_and_requirements() {
        let text = "---\nid: REQ-X\nrefines: [REQ-VIEW]\n---\n\n# Title\n\nSee `x` and ARCH-NO-DRIVING.";
        let chars: Vec<char> = text.chars().collect();
        let marks = markdown(text);
        let said = |role: Role| -> Vec<String> {
            marks.iter().filter(|m| m.role == role).map(|m| chars[m.start..m.stop].iter().collect()).collect()
        };
        assert_eq!(said(Role::Token { kind: TokenKind::Property }), vec!["id", "refines"]);
        assert_eq!(said(Role::Requirement), vec!["REQ-X", "REQ-VIEW", "ARCH-NO-DRIVING"]);
        assert_eq!(said(Role::Token { kind: TokenKind::Heading }), vec!["# Title"]);
        assert_eq!(said(Role::Token { kind: TokenKind::String }), vec!["`x`"]);
        assert!(marks.windows(2).all(|w| w[0].stop <= w[1].start));
    }

    /// Prose marks as the first TraceLean coloured it: bold, italic and links
    /// apart, and a `*` or `_` that is arithmetic or inside a name left alone.
    #[test]
    fn markdown_marks_bold_italic_and_links() {
        let text = "A **firm** and *soft* or _soft_ [docs](a.md), a * b, snake_case.";
        let chars: Vec<char> = text.chars().collect();
        let marks = markdown(text);
        let said = |kind: TokenKind| -> Vec<String> {
            marks.iter().filter(|m| m.role == Role::Token { kind }).map(|m| chars[m.start..m.stop].iter().collect()).collect()
        };
        assert_eq!(said(TokenKind::Bold), vec!["**firm**"]);
        assert_eq!(said(TokenKind::Italic), vec!["*soft*", "_soft_"]);
        assert_eq!(said(TokenKind::Link), vec!["[docs](a.md)"]);
        assert!(marks.windows(2).all(|w| w[0].stop <= w[1].start));
    }

    #[test]
    fn marks_never_overlap_and_a_file_without_a_grammar_has_none() {
        let marks = tokens("fn a() { \"x\" }", Some(Lang::Rust));
        assert!(marks.windows(2).all(|w| w[0].stop <= w[1].start));
        assert!(tokens("plain words", None).is_empty());
    }
}
