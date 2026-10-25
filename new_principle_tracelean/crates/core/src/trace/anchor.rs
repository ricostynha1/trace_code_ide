use serde::{Deserialize, Serialize};

use super::annotation::{Directive, ParsedComment, Problem, ProblemKind, RawAnnotation};

/// @implements REQ-ANCHOR.imprecise_capped
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Lean4,
    TypeScript,
}

impl Lang {
    pub fn from_extension(ext: &str) -> Option<Lang> {
        match ext {
            "rs" => Some(Lang::Rust),
            "lean" => Some(Lang::Lean4),
            "ts" => Some(Lang::TypeScript),
            _ => None,
        }
    }

    pub(crate) fn language(self) -> tree_sitter::Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::Lean4 => tree_sitter_lean4::language(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        }
    }

    fn declaration_kinds(self) -> &'static [&'static str] {
        match self {
            Lang::Rust => &[
                "function_item",
                "struct_item",
                "enum_item",
                "trait_item",
                "type_item",
                "const_item",
                "static_item",
                "union_item",
                "macro_definition",
                "mod_item",
            ],
            Lang::Lean4 => &["definition", "inductive", "structure", "abbreviation"],
            Lang::TypeScript => &[
                "function_declaration",
                "class_declaration",
                "interface_declaration",
                "type_alias_declaration",
                "enum_declaration",
                "method_definition",
                "variable_declarator",
            ],
        }
    }

    fn scope_only(self) -> &'static [&'static str] {
        match self {
            Lang::Rust => &["impl_item"],
            Lang::Lean4 | Lang::TypeScript => &[],
        }
    }

    fn sibling_scope(self) -> Option<(&'static str, &'static str)> {
        match self {
            Lang::Rust | Lang::TypeScript => None,
            Lang::Lean4 => Some(("namespace", "end")),
        }
    }
}

const COMMENT_KINDS: &[&str] = &["line_comment", "block_comment", "comment"];

const LITERAL_KINDS: &[&str] = &[
    "string_literal",
    "raw_string_literal",
    "char_literal",
    "string",
    "string_content",
    "template_string",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decl {
    pub symbol_path: String,
    pub start: usize,
    pub end: usize,
    pub start_line: u32,
    pub end_line: u32,
    #[serde(default = "yes")]
    pub precise: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AnchorKind {
    Decl { symbol_path: String },
    Region { start: usize, end: usize },
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub file: String,
    pub kind: AnchorKind,
    pub body_hash: String,
    pub start_line: u32,
    pub end_line: u32,
    pub precise: bool,
}

impl Anchor {
    /// @implements REQ-ANCHOR.symbol_not_line
    pub fn ident(&self) -> String {
        match &self.kind {
            AnchorKind::Decl { symbol_path } => format!("{}::{symbol_path}", self.file),
            AnchorKind::Region { start, end } => format!("{}@{start}..{end}", self.file),
            AnchorKind::File => self.file.clone(),
        }
    }
}

/// @implements REQ-ANCHOR.symbol_not_line
/// @implements REQ-ANCHOR.stable_under_move
/// @drt REQ-ANCHOR.symbol_not_line
/// @drt REQ-ANCHOR.stable_under_move
pub fn anchor_ident(file: String, kind: AnchorKind) -> String {
    match &kind {
        AnchorKind::Decl { symbol_path } => format!("{file}::{symbol_path}"),
        AnchorKind::Region { start, end } => format!("{file}@{start}..{end}"),
        AnchorKind::File => file,
    }
}

/// @implements REQ-ANCHOR.imprecise_capped
/// @drt REQ-ANCHOR.imprecise_capped
pub fn anchor_ceiling(precise: bool, claimed: crate::evidence::Level) -> crate::evidence::Level {
    if precise {
        claimed
    } else {
        crate::evidence::Level::L1
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scan {
    pub comments: ParsedComment,
    pub comment_ranges: Vec<(usize, usize)>,
    pub literal_ranges: Vec<(usize, usize)>,
    pub declarations: Vec<Decl>,
    pub error_ranges: Vec<(usize, usize)>,
    pub precise: bool,
    pub lang: Option<Lang>,
}

/// @implements REQ-ANNOT.comments_only
/// @implements REQ-ANNOT.literals_protected
pub fn scan(content: &str, lang: Option<Lang>) -> Scan {
    let Some(lang) = lang else {
        return Scan { precise: false, ..Default::default() };
    };

    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&lang.language()).is_err() {
        return Scan { precise: false, ..Default::default() };
    }
    let Some(tree) = parser.parse(content, None) else {
        return Scan { precise: false, ..Default::default() };
    };

    let mut scan = Scan { precise: !tree.root_node().has_error(), lang: Some(lang), ..Default::default() };
    collect(tree.root_node(), content, lang, &mut Vec::new(), &mut scan);
    scan.comment_ranges.sort_unstable();
    scan.literal_ranges.sort_unstable();
    scan.error_ranges.sort_unstable();
    scan.declarations.sort_by_key(|d| d.start);

    let mut ranges = scan.comment_ranges.clone();
    ranges.sort_unstable();
    let (mut counted, mut line) = (0usize, 0u32);
    for (start, end) in ranges {
        line += content[counted..start].matches('\n').count() as u32;
        counted = start;
        let parsed = super::annotation::parse_comment(&content[start..end], line);
        scan.comments.directives.extend(parsed.directives);
        scan.comments.problems.extend(parsed.problems);
    }
    scan
}

fn collect(
    node: tree_sitter::Node,
    content: &str,
    lang: Lang,
    scope: &mut Vec<String>,
    out: &mut Scan,
) {
    let kind = node.kind();

    if node.is_error() || node.is_missing() {
        out.error_ranges.push((node.start_byte(), node.end_byte()));
    }

    if COMMENT_KINDS.contains(&kind) {
        out.comment_ranges.push((node.start_byte(), node.end_byte()));
        return;
    }
    if LITERAL_KINDS.contains(&kind) {
        out.literal_ranges.push((node.start_byte(), node.end_byte()));
        return;
    }

    if lang.scope_only().contains(&kind) {
        let named = declaration_name(node, content, lang);
        if let Some(name) = named.clone() {
            scope.push(name);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect(child, content, lang, scope, out);
        }
        if named.is_some() {
            scope.pop();
        }
        return;
    }

    if lang.declaration_kinds().contains(&kind) {
        if let Some(name) = declaration_name(node, content, lang) {
            let mut path = scope.clone();
            path.push(name);
            out.declarations.push(Decl {
                symbol_path: path.join("::"),
                start: node.start_byte(),
                end: node.end_byte(),
                start_line: node.start_position().row as u32,
                end_line: node.end_position().row as u32,
                precise: !node.has_error(),
            });
            scope.push(path.pop().expect("just pushed"));
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect(child, content, lang, scope, out);
            }
            scope.pop();
            return;
        }
    }

    if let Some((open, close)) = lang.sibling_scope() {
        if kind == open {
            if let Some(name) = declaration_name(node, content, lang) {
                scope.push(name);
            }
            return;
        }
        if kind == close {
            let text = node.utf8_text(content.as_bytes()).unwrap_or_default();
            let named = text.trim_start_matches("end").trim();
            for segment in named.split('.').rev() {
                if segment.is_empty() {
                    continue;
                }
                if scope.last().map(String::as_str) == Some(segment) {
                    scope.pop();
                }
            }
            return;
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, content, lang, scope, out);
    }
}

fn declaration_name(node: tree_sitter::Node, content: &str, lang: Lang) -> Option<String> {
    let span = node
        .child_by_field_name("name")
        .map(|field| field.byte_range())
        .or_else(|| {
            let mut cursor = node.walk();
            let found = node.children(&mut cursor).find_map(|child| {
                let kind = child.kind();
                let names = match lang {
                    Lang::Rust | Lang::TypeScript => kind == "identifier" || kind == "type_identifier",
                    Lang::Lean4 => kind == "identifier",
                };
                names.then(|| child.byte_range())
            });
            found
        })?;
    content.get(span.start..extended_end(content, span.end, lang)).map(str::to_string)
}

fn extended_end(content: &str, mut end: usize, lang: Lang) -> usize {
    if !matches!(lang, Lang::Lean4) {
        return end;
    }
    while let Some(next) = content[end..].chars().next() {
        if next == '.' || next == '_' || next == '\'' || next == '!' || next == '?'
            || next.is_alphanumeric()
        {
            end += next.len_utf8();
        } else {
            break;
        }
    }
    end
}

/// @implements REQ-ANCHOR.symbol_not_line
/// @implements REQ-ANCHOR.stable_under_move
pub fn resolve(
    file: &str,
    content: &str,
    scan: &Scan,
    annotation: &RawAnnotation,
    comment_end: usize,
) -> Anchor {
    let next = scan
        .declarations
        .iter()
        .filter(|d| d.start >= comment_end)
        .min_by_key(|d| d.start);
    let target = enclosing(scan, comment_end)
        .filter(|outer| next.map_or(true, |d| d.start >= outer.end))
        .or(next);

    match target {
        Some(decl) => Anchor {
            file: file.to_string(),
            kind: AnchorKind::Decl { symbol_path: decl.symbol_path.clone() },
            body_hash: super::hash::body(
                &content[decl.start..decl.end],
                &shifted(&scan.comment_ranges, decl.start, decl.end),
                &shifted(&scan.literal_ranges, decl.start, decl.end),
            ),
            start_line: decl.start_line,
            end_line: decl.end_line,
            precise: decl.precise
                && !scan
                    .error_ranges
                    .iter()
                    .any(|(start, _)| *start >= comment_end && *start < decl.start),
        },
        None => Anchor {
            file: file.to_string(),
            kind: AnchorKind::File,
            body_hash: super::hash::body(content, &scan.comment_ranges, &scan.literal_ranges),
            start_line: annotation.line,
            end_line: content.matches('\n').count() as u32,
            precise: scan.precise,
        },
    }
}

fn enclosing(scan: &Scan, comment_end: usize) -> Option<&Decl> {
    let (comment_start, _) = scan.comment_ranges.iter().find(|(_, end)| *end == comment_end)?;
    scan.declarations
        .iter()
        .filter(|d| d.start < *comment_start && comment_end <= d.end)
        .max_by_key(|d| d.start)
}

fn shifted(ranges: &[(usize, usize)], start: usize, end: usize) -> Vec<(usize, usize)> {
    ranges
        .iter()
        .filter(|(s, e)| *s >= start && *e <= end)
        .map(|(s, e)| (s - start, e - start))
        .collect()
}

/// @implements REQ-ANNOT.region_balanced
/// @drt REQ-ANNOT.region_balanced
pub fn region_balance(directives: Vec<Directive>) -> Vec<Problem> {
    region_problems(&ParsedComment { directives, problems: Vec::new() })
}

/// @implements REQ-ANNOT.region_balanced
pub fn region_problems(comments: &ParsedComment) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut open: Vec<u32> = Vec::new();

    for directive in &comments.directives {
        match directive {
            Directive::Annotation { annotation: a } if a.opens_region => open.push(a.line),
            Directive::End { line } => {
                if open.pop().is_none() {
                    problems.push(Problem {
                        kind: ProblemKind::StrayEnd,
                        line: *line,
                        message: "`@end` closes nothing".into(),
                    });
                }
            }
            _ => {}
        }
    }
    for line in open {
        problems.push(Problem {
            kind: ProblemKind::UnclosedRegion,
            line,
            message: "a region opened with `begin` is never closed".into(),
        });
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUST: &str = r#"
// @implements REQ-X.one
pub fn alpha(a: u8) -> u8 { a }

pub struct Holder;

impl Holder {
    /// @implements REQ-X.two
    pub fn beta(&self) -> u8 { 2 }
}
"#;

    fn annotation(line: u32) -> RawAnnotation {
        RawAnnotation {
            role: super::super::annotation::Role::Implements,
            req_id: "REQ-X".into(),
            clause: None,
            attrs: Default::default(),
            opens_region: false,
            line,
        }
    }

    /// @tests REQ-ANCHOR.symbol_not_line
    #[test]
    fn a_nested_declaration_gets_a_path() {
        let scan = scan(RUST, Some(Lang::Rust));
        let paths: Vec<&str> = scan.declarations.iter().map(|d| d.symbol_path.as_str()).collect();
        assert!(paths.contains(&"alpha"), "{paths:?}");
        assert!(paths.contains(&"Holder::beta"), "{paths:?}");
    }

    /// @tests REQ-ANCHOR.stable_under_move
    #[test]
    fn moving_a_function_does_not_change_its_anchor() {
        let moved = "\npub struct Holder;\n\nimpl Holder {\n    /// @implements REQ-X.two\n    pub fn beta(&self) -> u8 { 2 }\n}\n\n// @implements REQ-X.one\npub fn alpha(a: u8) -> u8 { a }\n";
        let before = scan(RUST, Some(Lang::Rust));
        let after = scan(moved, Some(Lang::Rust));

        let find = |s: &Scan, content: &str, path: &str| {
            let decl = s.declarations.iter().find(|d| d.symbol_path == path).unwrap().clone();
            resolve("f.rs", content, s, &annotation(0), decl.start)
        };
        let a = find(&before, RUST, "Holder::beta");
        let b = find(&after, moved, "Holder::beta");
        assert_eq!(a.ident(), b.ident());
        assert_eq!(a.body_hash, b.body_hash);
        assert_ne!(a.start_line, b.start_line, "the line did move");
    }

    /// @tests REQ-ANNOT.comments_only
    /// @structural REQ-ANNOT.comments_only reason="a claim about which byte ranges the scanner reads, which needs the parser the model cannot run"
    #[test]
    fn an_annotation_in_a_string_is_not_a_link() {
        let src = "fn f() { let s = \"@implements REQ-FAKE.c\"; }";
        let scan = scan(src, Some(Lang::Rust));
        assert!(scan.comments.directives.is_empty(), "{:?}", scan.comments.directives);
        let code = "macro_rules! m { (@implements REQ-FAKE.c) => {} }\n";
        assert!(super::scan(code, Some(Lang::Rust)).comments.directives.is_empty());
        let ts = "const s = `@implements REQ-FAKE.c`;\n// @implements REQ-X.c\nfunction f() {}\n";
        let read = super::scan(ts, Some(Lang::TypeScript));
        assert_eq!(read.comments.directives.len(), 1, "{:?}", read.comments.directives);
    }

    #[test]
    fn typescript_declarations_are_anchored_by_name() {
        let src = "export interface Piece { a: number }\nexport function render(b: Buffer): void {}\nconst KEY = 1;\nclass Box {\n  draw() {}\n}\n";
        let scanned = scan(src, Some(Lang::TypeScript));
        let paths: Vec<&str> = scanned.declarations.iter().map(|d| d.symbol_path.as_str()).collect();
        assert_eq!(paths, vec!["Piece", "render", "KEY", "Box", "Box::draw"]);
        assert!(scanned.precise);
    }

    #[test]
    fn lean_declarations_carry_their_namespace() {
        let src = "namespace TraceLean\n\n/-- @models REQ-X.c -/\ndef assurance (n : Nat) : Nat := n\n\nend TraceLean\n";
        let scan = scan(src, Some(Lang::Lean4));
        let paths: Vec<&str> = scan.declarations.iter().map(|d| d.symbol_path.as_str()).collect();
        assert!(
            paths.iter().any(|p| p.contains("assurance")),
            "{paths:?}"
        );
    }

    /// @tests REQ-ANCHOR.symbol_not_line
    #[test]
    fn a_constructor_doc_comment_binds_to_its_inductive() {
        let src = "namespace N\n\ninductive Origin where\n  | node (n : Nat)\n  /-- The honest answer.\n\n  @models REQ-X.c -/\n  | base\n\ninductive BackStep where\n  | moved\n\n/-- @models REQ-X.d -/\ndef after (n : Nat) : Nat := n\n\nend N\n";
        let scanned = scan(src, Some(Lang::Lean4));
        let bound = |needle: &str| {
            let end = scanned
                .comment_ranges
                .iter()
                .find(|(s, e)| src[*s..*e].contains(needle))
                .map(|(_, e)| *e)
                .unwrap();
            resolve("f.lean", src, &scanned, &annotation(0), end).kind
        };
        assert_eq!(bound("REQ-X.c"), AnchorKind::Decl { symbol_path: "N::Origin".into() });
        assert_eq!(bound("REQ-X.d"), AnchorKind::Decl { symbol_path: "N::after".into() });
    }

    /// @tests REQ-ANCHOR.symbol_not_line
    #[test]
    fn a_rust_comment_inside_an_item_binds_to_that_item() {
        let src = "pub enum State {\n    /// @implements REQ-X.v\n    Current,\n}\n\npub struct Holder {\n    /// @implements REQ-X.f\n    pub field: u8,\n}\n\nfn body() {\n    // @implements REQ-X.b\n    let _ = 1;\n}\n\nimpl Holder {\n    /// @implements REQ-X.m\n    fn method(&self) {}\n}\n\nfn later() {}\n";
        let scanned = scan(src, Some(Lang::Rust));
        let bound = |needle: &str| {
            let end = scanned
                .comment_ranges
                .iter()
                .find(|(s, e)| src[*s..*e].contains(needle))
                .map(|(_, e)| *e)
                .unwrap();
            resolve("f.rs", src, &scanned, &annotation(0), end).kind
        };
        assert_eq!(bound("REQ-X.v"), AnchorKind::Decl { symbol_path: "State".into() });
        assert_eq!(bound("REQ-X.f"), AnchorKind::Decl { symbol_path: "Holder".into() });
        assert_eq!(bound("REQ-X.b"), AnchorKind::Decl { symbol_path: "body".into() });
        assert_eq!(bound("REQ-X.m"), AnchorKind::Decl { symbol_path: "Holder::method".into() });
    }

    /// @tests REQ-ANCHOR.imprecise_capped
    #[test]
    fn a_language_without_a_grammar_is_not_scanned() {
        let scan = scan("// @implements REQ-X.c\nsomething", None);
        assert!(!scan.precise);
        assert!(scan.comments.directives.is_empty());
    }

    /// @tests REQ-ANNOT.region_balanced
    #[test]
    fn unbalanced_regions_are_reported() {
        let src = "// @implements REQ-X begin\nfn f() {}\n";
        let opened = scan(src, Some(Lang::Rust));
        let problems = region_problems(&opened.comments);
        assert_eq!(problems[0].kind, ProblemKind::UnclosedRegion);

        let stray = scan("// @end\nfn f() {}\n", Some(Lang::Rust));
        assert_eq!(region_problems(&stray.comments)[0].kind, ProblemKind::StrayEnd);
    }

    /// @tests REQ-ANCHOR.comments_excluded
    #[test]
    fn editing_a_doc_comment_does_not_move_the_body_hash() {
        let a = "impl Holder {\n    /// @implements REQ-X.two\n    pub fn beta(&self) -> u8 { 2 }\n}";
        let b = "impl Holder {\n    /// @implements REQ-X.two reason=\"clearer\"\n    pub fn beta(&self) -> u8 { 2 }\n}";
        let hash = |src: &str| {
            let s = scan(src, Some(Lang::Rust));
            let d = s.declarations.iter().find(|d| d.symbol_path == "Holder::beta").unwrap();
            resolve("f.rs", src, &s, &annotation(0), d.start).body_hash
        };
        assert_eq!(hash(a), hash(b));
    }
}
