const INTRODUCERS: &[&str] = &[
    "fn", "struct", "enum", "trait", "type", "mod", "const", "static", "union", "macro_rules!",
    "def", "theorem", "lemma", "structure", "inductive", "class", "instance", "abbrev", "axiom",
    "function", "interface", "let", "var",
];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Declared {
    pub path: String,
    pub line: u32,
    pub text: String,
}

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

/// @implements REQ-NAV.definitions_found
/// @drt REQ-NAV.definitions_found
pub fn declarations(files: Vec<(String, String)>, name: &str) -> Vec<Declared> {
    let short = name.rsplit('.').next().unwrap_or(name);
    let mut found = Vec::new();
    for (path, text) in &files {
        for (n, line) in text.lines().enumerate() {
            if declares(line, name) || (short != name && declares(line, short)) {
                found.push(Declared { path: path.clone(), line: n as u32 + 1, text: line.trim().to_string() });
            }
        }
    }
    found.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    found
}

/// @implements REQ-NAV.uses_found
/// @drt REQ-NAV.uses_found
pub fn occurrences(mut files: Vec<(String, String)>, needle: &str, whole: bool, limit: usize) -> Vec<Declared> {
    if needle.is_empty() || limit == 0 {
        return Vec::new();
    }
    let lowered = needle.to_lowercase();
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut found = Vec::new();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, text) in &files {
        for (n, line) in text.lines().enumerate() {
            let hit = if whole {
                line.char_indices().any(|(at, _)| {
                    line[at..].starts_with(needle)
                        && !word(line[..at].chars().next_back())
                        && !word(line[at + needle.len()..].chars().next())
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

fn declares(line: &str, name: &str) -> bool {
    declared_on(line).as_deref() == Some(name)
}

/// @implements REQ-NAV.declaration_line
/// @drt REQ-NAV.declaration_line
pub fn declared_on(line: &str) -> Option<String> {
    let line = line.replace("::", "∷");
    let words: Vec<&str> = line
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>' | ':' | '{' | '=' | ';' | ','))
        .filter(|w| !w.is_empty())
        .collect();
    for (i, word) in words.iter().enumerate() {
        if INTRODUCERS.contains(word) {
            return words.get(i + 1).filter(|next| !next.contains('∷')).map(|next| next.to_string());
        }
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

    /// @tests REQ-NAV.declaration_line
    /// @tests REQ-NAV.definitions_found
    #[test]
    fn a_declaration_is_found_in_every_language_and_a_use_is_not() {
        let files = files();
        let found = declarations(files.clone(), "open");
        let places: Vec<(&str, u32)> = found.iter().map(|d| (d.path.as_str(), d.line)).collect();
        assert_eq!(places, vec![("src/a.rs", 2), ("web/app.ts", 1)]);
        let canon = declarations(files, "Workspace.canon");
        assert_eq!(canon.first().map(|d| d.line), Some(1));
        assert!(!declares("let Kind::File { path } = &kind else {", "Kind"));
        assert!(declares("let kind: Kind = x;", "kind"));
        assert_eq!(declared_on("    pub(crate) fn go(&self) {").as_deref(), Some("go"));
        assert_eq!(declared_on("theorem open_ok : True := trivial").as_deref(), Some("open_ok"));
        assert_eq!(declared_on("x = open(1)"), None);
    }

    /// @tests REQ-NAV.uses_found
    #[test]
    fn uses_are_whole_words_and_a_search_ignores_case() {
        let files = files();
        let uses = occurrences(files.clone(), "open", true, 50);
        let places: Vec<(&str, u32)> = uses.iter().map(|d| (d.path.as_str(), d.line)).collect();
        assert_eq!(places, vec![("src/a.rs", 2), ("src/a.rs", 3), ("web/app.ts", 1), ("web/app.ts", 2)]);
        let searched = occurrences(files.clone(), "OPEN_ok", false, 50);
        assert_eq!(searched.len(), 1);
        assert_eq!(occurrences(files.clone(), "open", true, 2).len(), 2);
        assert!(occurrences(files, "open", true, 0).is_empty());
        let dots = vec![("a".to_string(), "x...".to_string())];
        assert_eq!(occurrences(dots, "..", true, 5).len(), 1);
    }

    #[test]
    fn the_identifier_around_the_cursor_is_read_whole() {
        assert_eq!(identifier_at("x = open(1)", 6).as_deref(), Some("open"));
        assert_eq!(identifier_at("Workspace.canon w", 3).as_deref(), Some("Workspace.canon"));
        assert_eq!(identifier_at("  ( ", 2), None);
    }
}
