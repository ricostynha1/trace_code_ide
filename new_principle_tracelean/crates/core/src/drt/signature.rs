//! Reading a Rust function's parameter names out of its source.
//!
//! Rust has no runtime reflection, so the generated runner cannot bind
//! arguments by name the way the Python runner does. Parameter *order* is
//! therefore taken from the declaration, and parameter *types* are left to
//! inference at the generated call site.
//!
//! This is an ordinary total function from text to a list of names, which is
//! why it is here rather than inline in the generator: it is the part that can
//! be wrong, and it is checkable on its own.

/// Parameter names of `fn {symbol}` in `source`, in declaration order.
///
/// `self` is not a parameter a case can supply, so it is dropped; a method
/// taking `self` therefore reports only the arguments a binding has to provide,
/// and binding one is refused elsewhere rather than silently mis-called.
///
/// Returns `None` when no such function is declared, which is a different
/// answer from `Some(vec![])` — a function that takes nothing.
///
/// @implements REQ-DRT-RUST.params_from_source
/// @implements REQ-DRT-RUST.types_inferred
/// @implements REQ-DRT-BIND.call_only
pub fn parameters(source: &str, symbol: &str) -> Option<Vec<String>> {
    let open = signature_open_paren(source, symbol)?;
    let close = matching(source, open, '(', ')')?;
    let params = &source[open + 1..close];

    let mut names = Vec::new();
    for part in split_top_level(params, ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        // `&self`, `&mut self`, `self`, `self: Box<Self>` — never supplied by a
        // case.
        let head = part.trim_start_matches('&').trim_start();
        let head = head.strip_prefix("mut ").unwrap_or(head).trim_start();
        if head == "self" || head.starts_with("self:") {
            continue;
        }
        // A pattern parameter (`(a, b): (u8, u8)`) has no single name to bind,
        // so it is reported as absent rather than guessed at.
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

/// `parameters`, over owned arguments.
///
/// A binding's entry point has to take owned arguments. JSON escapes make a
/// borrowed `&str` impossible to supply: the unescaped bytes of `"a\nb"` do not
/// exist contiguously anywhere in the input, so no lifetime lets serde point at
/// them. See ADR-0010.
///
/// @implements REQ-DRT-RUST.params_from_source
/// @implements REQ-DRT-RUST.types_inferred
/// @drt REQ-DRT-RUST.params_from_source
/// @drt REQ-DRT-RUST.types_inferred
pub fn parameters_of(source: String, symbol: String) -> Option<Vec<String>> {
    parameters(&source, &symbol)
}

/// How many functions in `source` are declared with this name.
///
/// A binding names a symbol, not a path, so a file holding both a free function
/// `f` and a method `f` offers two answers to the same question. Counting is
/// what lets the caller refuse instead of taking the first — which is a wrong
/// call that compiles, runs, and reports agreement about the wrong function.
///
/// @implements REQ-DRT-BIND.call_only
pub fn declarations(source: &str, symbol: &str) -> usize {
    let mut count = 0;
    let mut from = 0usize;
    while let Some(open) = signature_open_paren(&source[from..], symbol) {
        count += 1;
        from += open + 1;
    }
    count
}

/// Byte offset of the `(` opening the parameter list of `fn {symbol}`.
///
/// Generic parameters are skipped by balance rather than by searching for the
/// next `(`, because `fn f<T: Fn(u8) -> u8>(x: T)` puts a parenthesis inside
/// them.
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

/// Offset of the delimiter closing the one at `start`.
///
/// When the delimiters are angle brackets, the `>` of an arrow is not a closing
/// bracket: `fn apply<F: Fn(u8) -> u8>(..)` would otherwise close the generic
/// list early and the parameter list would never be found.
/// Shared with the TypeScript reader: bracket balance is the one part of the
/// two parsers that is genuinely the same question, and two copies of it would
/// drift.
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

/// Split on `sep` at nesting depth zero, so a comma inside `Vec<A, B>`,
/// `(A, B)` or `[A; 2]` does not start a new parameter.
pub(crate) fn split_top_level(text: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (offset, ch) in text.char_indices() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            // `->` is not a closing angle bracket. Without this a return type
            // inside a closure parameter unbalances everything after it.
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

    /// A parenthesis inside the generic list is why the open paren is found by
    /// balance rather than by searching for the next `(`.
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

    /// A pattern parameter has no single name to bind, so it is refused rather
    /// than guessed at — a wrong guess produces a runner that calls the right
    /// function with the wrong arguments.
    #[test]
    fn pattern_parameter_is_refused() {
        assert_eq!(names("fn f((a, b): (u8, u8)) {}", "f"), None);
    }

    /// A prefix of another name must not match.
    #[test]
    fn similar_names_do_not_collide() {
        let src = "fn assure(a: u8) {}\nfn assurance(records: Vec<R>) {}";
        assert_eq!(names(src, "assurance"), Some(vec!["records".into()]));
    }

    /// The failure this exists to stop: a free function and a method sharing a
    /// name, where taking the first produces a call to the wrong one.
    ///
    /// @tests REQ-DRT-BIND.call_only
    #[test]
    fn a_name_declared_twice_is_counted_twice() {
        let src = "impl S {\n    pub fn blocks(&self) -> bool { true }\n}\npub fn blocks(state: S) -> bool { state.blocks() }\n";
        assert_eq!(declarations(src, "blocks"), 2);
        assert_eq!(declarations(src, "absent"), 0);
        assert_eq!(declarations("pub fn f(x: u8) {}", "f"), 1);
    }
}
