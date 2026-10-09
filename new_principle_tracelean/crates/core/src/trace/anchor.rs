//! Resolving an annotation to a stable anchor in the code.
//!
//! Anchors are symbol *paths*, never line numbers: line numbers rot on the
//! first edit above them, while `crates/core/src/evidence.rs::assurance`
//! survives reordering, reindentation and moving a function within its file.
//!
//! Finding the declarations is the effectful shell — it needs a parser this
//! project's models cannot run. What it must satisfy is stated as a law:
//! equal normalised bodies hash equally, and an anchor's identity does not
//! move unless its body does.

use serde::{Deserialize, Serialize};

use super::annotation::{Directive, ParsedComment, Problem, ProblemKind, RawAnnotation};

/// A language this project can anchor precisely in.
///
/// A file whose language is absent anchors to the whole file and its links are
/// capped at the lowest evidence level, rather than being reported as broken.
///
/// @implements REQ-ANCHOR.imprecise_capped
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Lean4,
}

impl Lang {
    pub fn from_extension(ext: &str) -> Option<Lang> {
        match ext {
            "rs" => Some(Lang::Rust),
            "lean" => Some(Lang::Lean4),
            _ => None,
        }
    }

    pub(crate) fn language(self) -> tree_sitter::Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::Lean4 => tree_sitter_lean4::language(),
        }
    }

    /// Node kinds that are declarations, and whose name is their first
    /// identifier child.
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
        }
    }

    /// Kinds that open a named scope around their contents without being a
    /// declaration themselves. Rust's `impl Foo` is the case: it names a type
    /// that is declared elsewhere, so emitting it as a declaration would give
    /// two declarations in one file the symbol path `Foo` — and an annotation
    /// on either would then be indistinguishable from an annotation on the
    /// other.
    fn scope_only(self) -> &'static [&'static str] {
        match self {
            Lang::Rust => &["impl_item"],
            Lang::Lean4 => &[],
        }
    }

    /// Kinds that open a named scope closed by a sibling `end`, rather than by
    /// containing their contents.
    fn sibling_scope(self) -> Option<(&'static str, &'static str)> {
        match self {
            Lang::Rust => None,
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
];

/// One declaration found in a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decl {
    /// Dotted or double-colon path within the file, e.g. `Reply::is_well_formed`.
    pub symbol_path: String,
    pub start: usize,
    pub end: usize,
    pub start_line: u32,
    pub end_line: u32,
    /// Whether this declaration's own text parsed. A declaration containing an
    /// unparsed region may have the wrong extent, so anything hashed from it is
    /// a guess.
    #[serde(default = "yes")]
    pub precise: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AnchorKind {
    /// A named declaration, addressed by its path within the file.
    Decl { symbol_path: String },
    /// An explicit `begin` … `@end` byte range.
    Region { start: usize, end: usize },
    /// The whole file — when nothing named follows, or the language has no
    /// grammar here.
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub file: String,
    pub kind: AnchorKind,
    /// Hash of the normalised body, with comment text removed.
    pub body_hash: String,
    pub start_line: u32,
    pub end_line: u32,
    /// False when the anchor is the whole file for want of a grammar.
    pub precise: bool,
}

impl Anchor {
    /// Stable identity string, used in the link hash and on disk.
    ///
    /// @implements REQ-ANCHOR.symbol_not_line
    pub fn ident(&self) -> String {
        match &self.kind {
            AnchorKind::Decl { symbol_path } => format!("{}::{symbol_path}", self.file),
            AnchorKind::Region { start, end } => format!("{}@{start}..{end}", self.file),
            AnchorKind::File => self.file.clone(),
        }
    }
}

/// An anchor's identity, from the file it is in and what it names.
///
/// No line number appears anywhere in it. A line number is the identity that
/// rots on the first edit above it, and an evidence record keyed on one is
/// invalidated by inserting a blank line.
///
/// The file is part of the identity, deliberately: evidence earned for a
/// function in one file is not inherited by the same text in another
/// (`REQ-STALE.retarget_invalidates`). What `stable_under_move` asks is that
/// the identity survives edits *within* the file — moving a function down,
/// reindenting it, rewriting the comment above it — which it does, because
/// none of those changes the path or the normalised body.
///
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

/// The highest level a link through this anchor may reach.
///
/// A file with no available grammar anchors to the whole file, and everything
/// claimed through it is capped at the bottom of the ladder. Not reported as
/// broken — an unsupported language is not a fault in the project — but not
/// allowed to carry a proof either, because nothing knows which declaration the
/// claim was about.
///
/// @implements REQ-ANCHOR.imprecise_capped
/// @drt REQ-ANCHOR.imprecise_capped
pub fn anchor_ceiling(precise: bool, claimed: crate::evidence::Level) -> crate::evidence::Level {
    if precise {
        claimed
    } else {
        crate::evidence::Level::L1
    }
}

/// What a scan of one file found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scan {
    pub comments: ParsedComment,
    pub comment_ranges: Vec<(usize, usize)>,
    pub literal_ranges: Vec<(usize, usize)>,
    pub declarations: Vec<Decl>,
    /// Byte ranges the grammar could not parse. Recorded rather than collapsed
    /// into one file-wide flag so that an anchor can say whether *it* is
    /// precise: a tactic block the Lean grammar cannot read says nothing about
    /// a definition twenty lines above it.
    pub error_ranges: Vec<(usize, usize)>,
    pub precise: bool,
    /// The grammar the file was read with, if any.
    pub lang: Option<Lang>,
}

/// Scan a file: comments parsed, literal ranges recorded, declarations found.
///
/// @implements REQ-ANNOT.comments_only
/// @implements REQ-ANNOT.literals_protected
pub fn scan(content: &str, lang: Option<Lang>) -> Scan {
    let Some(lang) = lang else {
        // No grammar: nothing is anchored precisely, and nothing is scanned for
        // annotations either — a link that cannot be placed is not a link.
        return Scan { precise: false, ..Default::default() };
    };

    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&lang.language()).is_err() {
        return Scan { precise: false, ..Default::default() };
    }
    let Some(tree) = parser.parse(content, None) else {
        return Scan { precise: false, ..Default::default() };
    };

    // A tree with an error node is a parse this cannot trust: declarations
    // after the error are not found, so annotations there would silently anchor
    // to the whole file while claiming to be precise. Saying so is the
    // difference between a capped claim and a wrong one.
    let mut scan = Scan { precise: !tree.root_node().has_error(), lang: Some(lang), ..Default::default() };
    collect(tree.root_node(), content, lang, &mut Vec::new(), &mut scan);
    scan.comment_ranges.sort_unstable();
    scan.literal_ranges.sort_unstable();
    scan.error_ranges.sort_unstable();
    scan.declarations.sort_by_key(|d| d.start);

    // Comments are parsed in source order so that a qualifier attaches to the
    // annotation above it.
    let mut ranges = scan.comment_ranges.clone();
    ranges.sort_unstable();
    // Lines counted on from the last comment rather than from the top.
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
        // Matched comments are not descended into: tree-sitter nests doc-marker
        // children inside them, and each would otherwise be counted again.
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

    // A scope opened by one node and closed by a later sibling.
    if let Some((open, close)) = lang.sibling_scope() {
        if kind == open {
            if let Some(name) = declaration_name(node, content, lang) {
                scope.push(name);
            }
            return;
        }
        if kind == close {
            // Only an `end` that names a scope closes one. Lean writes a bare
            // `end` to close a `mutual` block or an anonymous section, and
            // treating that as the end of the enclosing namespace moved every
            // later declaration out of it — which produced an anchor for a
            // symbol that does not exist under that name.
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

/// The declared name: the node's `name` field where the grammar has one, else
/// its first identifier child.
fn declaration_name(node: tree_sitter::Node, content: &str, lang: Lang) -> Option<String> {
    let span = node
        .child_by_field_name("name")
        .map(|field| field.byte_range())
        .or_else(|| {
            // Rust's `impl` blocks name a type rather than a declaration; using
            // the type keeps `Reply::is_well_formed` readable.
            let mut cursor = node.walk();
            let found = node.children(&mut cursor).find_map(|child| {
                let kind = child.kind();
                let names = match lang {
                    Lang::Rust => kind == "identifier" || kind == "type_identifier",
                    Lang::Lean4 => kind == "identifier",
                };
                names.then(|| child.byte_range())
            });
            found
        })?;
    content.get(span.start..extended_end(content, span.end, lang)).map(str::to_string)
}

/// Lean names are dotted: `def Kind.progress` declares `Kind.progress`, not
/// `Kind`. The grammar hands back only the leading component, which would give
/// the inductive and each of its functions the same symbol path. The name is
/// read straight out of the source instead, which does not depend on how the
/// grammar chose to split the token.
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

/// Resolve one annotation to an anchor: the first declaration starting after
/// the comment it was written in.
///
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
            // The anchor is precise when the declaration it names parsed, and
            // when no unparsed region *begins* between the comment and it: such
            // a region could be hiding a declaration that should have been the
            // target, which would make this anchor point at the wrong thing.
            //
            // Regions that merely enclose both are not that. Recovery wraps
            // large spans — a whole Lean file, in the worst case — around
            // constructs the grammar cannot read, and the declarations inside
            // are still found. Treating an enclosing region as a failure would
            // cap every annotation in such a file on the strength of one tactic
            // block, which is the behaviour this replaces.
            precise: decl.precise
                && !scan
                    .error_ranges
                    .iter()
                    .any(|(start, _)| *start >= comment_end && *start < decl.start),
        },
        // Nothing named follows, so the claim is about the file.
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

/// The Lean declaration a comment sits inside, innermost first: an inductive
/// whose constructor carries the doc comment, or a structure whose field does.
///
/// A constructor or a field is not a declaration of its own, so the first
/// declaration after such a comment is the *next* top-level one — and the
/// annotation used to bind there, to something its author never pointed at.
/// When nothing is declared between the comment and the end of the enclosing
/// declaration, the claim is about that declaration.
///
/// Lean only. Rust has the same shape (a doc comment on an enum variant or a
/// struct field), and those annotations are left where they bind today until
/// they are sorted out, since moving them changes their links' identities.
fn enclosing(scan: &Scan, comment_end: usize) -> Option<&Decl> {
    if scan.lang != Some(Lang::Lean4) {
        return None;
    }
    let (comment_start, _) = scan.comment_ranges.iter().find(|(_, end)| *end == comment_end)?;
    scan.declarations
        .iter()
        .filter(|d| d.start < *comment_start && comment_end <= d.end)
        .max_by_key(|d| d.start)
}

/// Ranges within `[start, end)`, rebased to that slice.
fn shifted(ranges: &[(usize, usize)], start: usize, end: usize) -> Vec<(usize, usize)> {
    ranges
        .iter()
        .filter(|(s, e)| *s >= start && *e <= end)
        .map(|(s, e)| (s - start, e - start))
        .collect()
}

/// Region and `@end` balance across a file's directives.
///
/// `region_problems`, over the directives alone.
///
/// The balance question is about the sequence of directives and nothing else —
/// not the file, not the parse, not what any of them names — so this is what
/// the model is compared against.
///
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

    /// A constructor's doc comment is inside its inductive; an annotation there
    /// is about the inductive, not about whatever is declared next.
    ///
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
        // An annotation before a declaration still binds to that declaration.
        assert_eq!(bound("REQ-X.d"), AnchorKind::Decl { symbol_path: "N::after".into() });
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

    /// Editing the annotation must not invalidate the evidence it carries.
    ///
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
