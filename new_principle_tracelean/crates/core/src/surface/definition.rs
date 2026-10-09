//! Where a name is defined, found by reading the files — no language server.
//!
//! A language server is a process, and this editor starts none. What a person
//! mostly wants from "go to definition" is the line that says `fn name`,
//! `struct name`, `def name`, `theorem name`: a declaration keyword followed by
//! the name. That is found by reading the workspace, which the editor already
//! holds, and a name declared twice is answered with both.

/// Words that introduce a declaration, in the languages this editor reads.
const INTRODUCERS: &[&str] = &[
    // Rust
    "fn", "struct", "enum", "trait", "type", "mod", "const", "static", "union", "macro_rules!",
    // Lean
    "def", "theorem", "lemma", "structure", "inductive", "class", "instance", "abbrev", "axiom",
    // TypeScript / JavaScript
    "function", "interface", "let", "var",
];

/// One place a name is declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    pub path: String,
    /// One-based.
    pub line: u32,
    /// The declaring line, trimmed, for a person choosing between several.
    pub text: String,
}

/// The identifier around a position: ASCII letters, digits, `_`, and `.`
/// inside a Lean name (`Workspace.canon`).
///
/// ASCII, because `dispatch` reads the name with it and is compared with a
/// model (`TraceLean.Act.identifierAt`) whose characters are classified the
/// same way on both sides; the identifiers of the languages read here are.
pub fn identifier_at(text: &str, offset: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let part = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '!';
    let at = offset.min(chars.len());
    let mut start = at;
    while start > 0 && part(chars[start - 1]) {
        start -= 1;
    }
    let mut end = at;
    while end < chars.len() && part(chars[end]) {
        end += 1;
    }
    let word: String = chars[start..end].iter().collect();
    let word = word.trim_matches('.').trim_end_matches('!').to_string();
    (!word.is_empty() && !word.chars().all(|c| c.is_ascii_digit())).then_some(word)
}

/// Every line of every file that declares `name`, by path then line.
///
/// A dotted name (`Workspace.canon`) is also looked for by its last part,
/// since that is how most declarations spell it.
pub fn declarations<'a>(files: impl IntoIterator<Item = (&'a String, &'a String)>, name: &str) -> Vec<Declared> {
    let short = name.rsplit('.').next().unwrap_or(name);
    let mut found = Vec::new();
    for (path, text) in files {
        for (n, line) in text.lines().enumerate() {
            if declares(line, name) || (short != name && declares(line, short)) {
                found.push(Declared { path: path.clone(), line: n as u32 + 1, text: line.trim().to_string() });
            }
        }
    }
    found.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    found
}

/// Every line of every file that holds `needle`, by path then line, at most
/// `limit` of them. As a whole word only when `whole` — which is how a name's
/// uses are found — and ignoring case otherwise, which is how a person searches.
pub fn occurrences<'a>(
    files: impl IntoIterator<Item = (&'a String, &'a String)>,
    needle: &str,
    whole: bool,
    limit: usize,
) -> Vec<Declared> {
    if needle.is_empty() {
        return Vec::new();
    }
    let lowered = needle.to_lowercase();
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut found = Vec::new();
    let mut sorted: Vec<(&String, &String)> = files.into_iter().collect();
    sorted.sort();
    for (path, text) in sorted {
        for (n, line) in text.lines().enumerate() {
            let hit = if whole {
                line.match_indices(needle).any(|(at, _)| {
                    !word(line[..at].chars().next_back()) && !word(line[at + needle.len()..].chars().next())
                })
            } else {
                line.to_lowercase().contains(&lowered)
            };
            if hit {
                found.push(Declared { path: path.clone(), line: n as u32 + 1, text: line.trim().to_string() });
                if found.len() >= limit {
                    return found;
                }
            }
        }
    }
    found
}

/// Whether a line declares `name`: an introducer, then the name as a whole
/// word, with only modifiers and visibility before the introducer.
fn declares(line: &str, name: &str) -> bool {
    declared_on(line).as_deref() == Some(name)
}

/// The name a line declares, if it declares one: the word after its
/// introducer, with only modifiers and visibility before that.
pub fn declared_on(line: &str) -> Option<String> {
    // A path stays one word: `let Kind::File { .. } = x` declares no `Kind`.
    let line = line.replace("::", "∷");
    let words: Vec<&str> = line
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>' | ':' | '{' | '=' | ';' | ','))
        .filter(|w| !w.is_empty())
        .collect();
    for (i, word) in words.iter().enumerate() {
        if INTRODUCERS.contains(word) {
            return words.get(i + 1).filter(|next| !next.contains('∷')).map(|next| next.to_string());
        }
        // Only modifiers may come before the introducer.
        let modifier = matches!(
            *word,
            "pub" | "pub(crate)" | "pub(super)" | "crate" | "async" | "unsafe" | "extern" | "export"
                | "default" | "private" | "protected" | "noncomputable" | "partial" | "@[simp]"
        ) || word.starts_with("pub(")
            || word.starts_with("@[");
        if !modifier {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<(String, String)> {
        vec![
            ("src/a.rs".into(), "use x;\npub fn open(a: u8) {}\nfn other() { open(1) }\n".into()),
            ("formal/A.lean".into(), "def canon (w : W) : W := w\ntheorem open_ok : True := trivial\n".into()),
            ("web/app.ts".into(), "export function open() {}\nconst x = open();\n".into()),
        ]
    }

    #[test]
    fn a_declaration_is_found_in_every_language_and_a_use_is_not() {
        let files = files();
        let found = declarations(files.iter().map(|(p, t)| (p, t)), "open");
        let places: Vec<(&str, u32)> = found.iter().map(|d| (d.path.as_str(), d.line)).collect();
        assert_eq!(places, vec![("src/a.rs", 2), ("web/app.ts", 1)]);
        let canon = declarations(files.iter().map(|(p, t)| (p, t)), "Workspace.canon");
        assert_eq!(canon.first().map(|d| d.line), Some(1));
        // A pattern naming a path declares nothing of it.
        assert!(!declares("let Kind::File { path } = &kind else {", "Kind"));
        assert!(declares("let kind: Kind = x;", "kind"));
        assert_eq!(declared_on("    pub(crate) fn go(&self) {").as_deref(), Some("go"));
        assert_eq!(declared_on("theorem open_ok : True := trivial").as_deref(), Some("open_ok"));
        assert_eq!(declared_on("x = open(1)"), None);
    }

    #[test]
    fn uses_are_whole_words_and_a_search_ignores_case() {
        let files = files();
        let uses = occurrences(files.iter().map(|(p, t)| (p, t)), "open", true, 50);
        let places: Vec<(&str, u32)> = uses.iter().map(|d| (d.path.as_str(), d.line)).collect();
        assert_eq!(places, vec![("src/a.rs", 2), ("src/a.rs", 3), ("web/app.ts", 1), ("web/app.ts", 2)]);
        let searched = occurrences(files.iter().map(|(p, t)| (p, t)), "OPEN_ok", false, 50);
        assert_eq!(searched.len(), 1);
        assert_eq!(occurrences(files.iter().map(|(p, t)| (p, t)), "open", true, 2).len(), 2);
    }

    #[test]
    fn the_identifier_around_the_cursor_is_read_whole() {
        assert_eq!(identifier_at("x = open(1)", 6).as_deref(), Some("open"));
        assert_eq!(identifier_at("Workspace.canon w", 3).as_deref(), Some("Workspace.canon"));
        assert_eq!(identifier_at("  ( ", 2), None);
    }
}
