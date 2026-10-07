//! Scanning source files for traceability annotations.
//!
//! An annotation is a typed edge written in a comment:
//!
//! ```text
//! // @implements REQ-AUTH-03.post exclusive
//! // @exempt REQ-01.log reason="side effect, not in the pure model" by=ana
//! // @implements REQ-X begin   … // @end
//! ```
//!
//! Comments are found through tree-sitter so the same code works for `//`,
//! `#`, `--`, `/* */` and docstring-style comments with no per-language rules;
//! a file whose extension has no grammar falls back to a line scan and its
//! links are capped at L1 (see `AnchorKind::File`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use tree_sitter::{Node, Parser};

use crate::parser::Lang;

/// What a link claims. Qualifiers (`partial`, `exempt`) are deliberately *not*
/// roles — they modify a claim rather than being one, see [`Qualifier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// A Lean definition that formalizes a requirement clause.
    Models,
    /// Production code that realizes it.
    Implements,
    /// A test that exercises it.
    Tests,
    /// A differential-testing harness binding a model to an implementation.
    Drt,
    /// A Lean theorem discharging a property of the model.
    Proves,
    /// A Lean theorem showing the proved properties *determine* the model:
    /// anything satisfying them is that function. `@proves` says the model has
    /// a property; `@pins` says the properties leave no other model possible.
    Pins,
}

impl Role {
    pub fn parse(s: &str) -> Option<Role> {
        Some(match s {
            "models" => Role::Models,
            "implements" => Role::Implements,
            "tests" => Role::Tests,
            "drt" => Role::Drt,
            "proves" => Role::Proves,
            "pins" => Role::Pins,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Models => "models",
            Role::Implements => "implements",
            Role::Tests => "tests",
            Role::Drt => "drt",
            Role::Proves => "proves",
            Role::Pins => "pins",
        }
    }
}

/// Modifies the claim made by the nearest annotation.
///
/// `partial` caps a clause's contribution below 1.0 and suppresses nothing;
/// `exempt` removes the clause from the coverage denominator entirely, which
/// is why it must carry a reason and an approver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Qualifier {
    Partial { reason: Option<String> },
    Exempt { reason: Option<String>, by: Option<String>, until: Option<String> },
    /// This model cannot be pinned, and here is why.
    ///
    /// Some functions genuinely are not determined by their declared inputs --
    /// anything drawing on randomness, a clock, or the order of concurrent
    /// events. Asking "do the proved properties determine the output" is the
    /// wrong question there, and saying so in one place with a reason is better
    /// than an obligation that stays open forever and teaches everyone to
    /// ignore the column.
    Nondeterministic { reason: Option<String> },
}

impl Qualifier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Qualifier::Partial { .. } => "partial",
            Qualifier::Exempt { .. } => "exempt",
            Qualifier::Nondeterministic { .. } => "nondeterministic",
        }
    }
}

/// What the scanner found, before an anchor has been resolved for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawAnnotation {
    pub role: Role,
    pub req_id: String,
    pub clause: Option<String>,
    pub qualifier: Option<Qualifier>,
    pub attrs: BTreeMap<String, String>,
    pub file: PathBuf,
    /// Byte offset of the comment node this was written in.
    pub comment_start: usize,
    /// Byte offset just past the comment node.
    pub comment_end: usize,
    /// 0-indexed line of the comment.
    pub line: u32,
    /// For `@role REQ begin` … `@end`, the byte range of the enclosed body.
    pub region: Option<(usize, usize)>,
}

/// Something wrong with an annotation itself — reported, never silently dropped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationProblem {
    pub file: PathBuf,
    pub line: u32,
    pub kind: AnnotationProblemKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnnotationProblemKind {
    UnknownRole,
    UnclosedRegion,
    StrayEnd,
    /// A qualifier that carries neither a preceding annotation nor its own id.
    OrphanQualifier,
}

/// Result of scanning one file.
#[derive(Debug, Default)]
pub struct ScanResult {
    pub annotations: Vec<RawAnnotation>,
    pub problems: Vec<AnnotationProblem>,
    /// Byte ranges of every comment node — needed to exclude comment text from
    /// body hashes, so editing an annotation cannot invalidate its own evidence.
    pub comment_ranges: Vec<(usize, usize)>,
    /// Byte ranges of string and character literals, protected from whitespace
    /// normalization.
    pub literal_ranges: Vec<(usize, usize)>,
    /// False when no grammar was available and the file was line-scanned.
    pub precise: bool,
}

/// Comment node kinds, matched exactly.
///
/// `kind().contains("comment")` would over-count: tree-sitter-rust nests
/// `doc_comment` and `*_doc_comment_marker` inside `line_comment`, so a single
/// `/// @implements REQ-X` would be seen two or three times.
const COMMENT_KINDS: &[&str] = &["line_comment", "block_comment", "comment"];

const LITERAL_KINDS: &[&str] = &[
    "string_literal",
    "raw_string_literal",
    "char_literal",
    "string",
    "string_content",
    "concatenated_string",
    "interpreted_string_literal",
];

fn annotation_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"@(?P<role>[a-z_]+)(?:\s+(?P<id>[A-Z][A-Za-z0-9_-]*)(?:\.(?P<clause>[A-Za-z0-9_]+))?)?(?P<rest>[^\n]*)",
        )
        .expect("annotation regex")
    })
}

fn attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?P<key>[a-z_]+)=(?:"(?P<q>[^"]*)"|(?P<b>[^\s"]+))"#).expect("attr regex")
    })
}

/// Scan a file for annotations, comment ranges and literal ranges.
pub fn scan_file(path: &Path, root: &Path, content: &str) -> ScanResult {
    let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();

    match tree_for(path, content) {
        Some((tree, _lang)) => {
            let mut comments = Vec::new();
            let mut literals = Vec::new();
            collect_ranges(tree.root_node(), &mut comments, &mut literals);
            comments.sort_unstable();
            literals.sort_unstable();
            let mut result = parse_comments(&rel, content, &comments);
            result.comment_ranges = comments;
            result.literal_ranges = literals;
            result.precise = true;
            result
        }
        None => {
            // No grammar: treat every line as potentially carrying an
            // annotation. Links from such files are capped at L1 by the
            // checker rather than being reported as broken.
            let ranges: Vec<(usize, usize)> = Vec::new();
            let mut result = parse_lines(&rel, content);
            result.comment_ranges = ranges;
            result.precise = false;
            result
        }
    }
}

fn tree_for(path: &Path, content: &str) -> Option<(tree_sitter::Tree, Lang)> {
    let ext = path.extension()?.to_str()?;
    let lang = Lang::from_extension(ext)?;
    let mut parser = Parser::new();
    parser.set_language(&lang.tree_sitter_language).ok()?;
    let tree = parser.parse(content, None)?;
    Some((tree, lang))
}

/// Walk the tree collecting comment and literal byte ranges. Matched comment
/// nodes are not descended into, so their nested doc-marker children cannot
/// produce duplicates.
fn collect_ranges(node: Node, comments: &mut Vec<(usize, usize)>, literals: &mut Vec<(usize, usize)>) {
    let kind = node.kind();
    if COMMENT_KINDS.contains(&kind) {
        comments.push((node.start_byte(), node.end_byte()));
        return;
    }
    if LITERAL_KINDS.contains(&kind) {
        literals.push((node.start_byte(), node.end_byte()));
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_ranges(child, comments, literals);
    }
}

/// Parse annotations out of known comment ranges.
fn parse_comments(rel: &Path, content: &str, comments: &[(usize, usize)]) -> ScanResult {
    let mut out = ScanResult::default();
    let mut open_regions: Vec<(usize, RawAnnotation)> = Vec::new();

    for &(start, end) in comments {
        let text = &content[start..end];
        let line = line_of(content, start);
        parse_one(
            rel, content, text, start, end, line, &mut out, &mut open_regions,
        );
    }

    finish_regions(rel, content, &mut out, open_regions);
    out
}

/// Fallback for files with no grammar: scan every line.
fn parse_lines(rel: &Path, content: &str) -> ScanResult {
    let mut out = ScanResult::default();
    let mut open_regions: Vec<(usize, RawAnnotation)> = Vec::new();
    let mut offset = 0usize;

    for (idx, line) in content.split_inclusive('\n').enumerate() {
        if line.contains('@') {
            parse_one(
                rel,
                content,
                line,
                offset,
                offset + line.len(),
                idx as u32,
                &mut out,
                &mut open_regions,
            );
        }
        offset += line.len();
    }

    finish_regions(rel, content, &mut out, open_regions);
    out
}

#[allow(clippy::too_many_arguments)]
fn parse_one(
    rel: &Path,
    content: &str,
    text: &str,
    start: usize,
    end: usize,
    line: u32,
    out: &mut ScanResult,
    open_regions: &mut Vec<(usize, RawAnnotation)>,
) {
    for cap in annotation_re().captures_iter(text) {
        let role_str = cap.name("role").map(|m| m.as_str()).unwrap_or_default();
        let rest = cap.name("rest").map(|m| m.as_str()).unwrap_or_default();

        // `@end` closes the most recently opened region in this file (LIFO).
        if role_str == "end" {
            match open_regions.pop() {
                Some((body_start, mut ann)) => {
                    let body_end = line_start(content, start);
                    ann.region = Some((body_start, body_end.max(body_start)));
                    out.annotations.push(ann);
                }
                None => out.problems.push(AnnotationProblem {
                    file: rel.to_path_buf(),
                    line,
                    kind: AnnotationProblemKind::StrayEnd,
                    message: "`@end` with no open region".into(),
                }),
            }
            continue;
        }

        let attrs = parse_attrs(rest);

        // Qualifiers attach to the annotation they follow, or stand alone on a
        // clause of their own.
        if role_str == "partial" || role_str == "exempt" || role_str == "nondeterministic" {
            let qual = match role_str {
                "partial" => Qualifier::Partial { reason: attrs.get("reason").cloned() },
                "nondeterministic" => {
                    Qualifier::Nondeterministic { reason: attrs.get("reason").cloned() }
                }
                _ => Qualifier::Exempt {
                    reason: attrs.get("reason").cloned(),
                    by: attrs.get("by").cloned(),
                    until: attrs.get("until").cloned(),
                },
            };

            let id = cap.name("id").map(|m| m.as_str().to_string());
            let clause = cap.name("clause").map(|m| m.as_str().to_string());

            match id {
                // Standalone: `@exempt REQ-01.log reason=… by=…`
                Some(req_id) => out.annotations.push(RawAnnotation {
                    role: Role::Implements,
                    req_id,
                    clause,
                    qualifier: Some(qual),
                    attrs,
                    file: rel.to_path_buf(),
                    comment_start: start,
                    comment_end: end,
                    line,
                    region: None,
                }),
                // Bare `@partial reason=…`: qualifies the previous annotation.
                None => match out.annotations.last_mut() {
                    Some(prev) => prev.qualifier = Some(qual),
                    None => out.problems.push(AnnotationProblem {
                        file: rel.to_path_buf(),
                        line,
                        kind: AnnotationProblemKind::OrphanQualifier,
                        message: format!("`@{role_str}` does not follow any annotation"),
                    }),
                },
            }
            continue;
        }

        let Some(role) = Role::parse(role_str) else {
            // `@param`, `@returns`, `@todo` and friends are ordinary doc tags,
            // not typos — only flag words that look like a traceability role
            // by being followed by a requirement id.
            if cap.name("id").is_some() {
                out.problems.push(AnnotationProblem {
                    file: rel.to_path_buf(),
                    line,
                    kind: AnnotationProblemKind::UnknownRole,
                    message: format!("unknown annotation role `@{role_str}`"),
                });
            }
            continue;
        };

        let Some(req_id) = cap.name("id").map(|m| m.as_str().to_string()) else {
            out.problems.push(AnnotationProblem {
                file: rel.to_path_buf(),
                line,
                kind: AnnotationProblemKind::UnknownRole,
                message: format!("`@{role_str}` without a requirement id"),
            });
            continue;
        };

        let ann = RawAnnotation {
            role,
            req_id,
            clause: cap.name("clause").map(|m| m.as_str().to_string()),
            qualifier: None,
            attrs,
            file: rel.to_path_buf(),
            comment_start: start,
            comment_end: end,
            line,
            region: None,
        };

        if rest.split_whitespace().any(|w| w == "begin") {
            // Body starts after the line holding `begin`.
            open_regions.push((line_end(content, end), ann));
        } else {
            out.annotations.push(ann);
        }
    }
}

fn finish_regions(
    rel: &Path,
    content: &str,
    out: &mut ScanResult,
    open_regions: Vec<(usize, RawAnnotation)>,
) {
    for (body_start, mut ann) in open_regions {
        out.problems.push(AnnotationProblem {
            file: rel.to_path_buf(),
            line: ann.line,
            kind: AnnotationProblemKind::UnclosedRegion,
            message: format!("region opened by `@{} {}` is never closed", ann.role.as_str(), ann.req_id),
        });
        // Still record it, bounded by end of file, so the link is not lost.
        ann.region = Some((body_start, content.len()));
        out.annotations.push(ann);
    }
}

fn parse_attrs(rest: &str) -> BTreeMap<String, String> {
    let mut attrs = BTreeMap::new();
    for cap in attr_re().captures_iter(rest) {
        let key = cap.name("key").unwrap().as_str().to_string();
        let val = cap
            .name("q")
            .or_else(|| cap.name("b"))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        attrs.insert(key, val);
    }
    // Bare flags, e.g. `exclusive`, recorded with an empty value.
    for word in rest.split_whitespace() {
        if word == "exclusive" {
            attrs.insert("exclusive".into(), String::new());
        }
    }
    attrs
}

fn line_of(content: &str, byte: usize) -> u32 {
    content[..byte.min(content.len())].matches('\n').count() as u32
}

/// Byte offset of the start of the line containing `byte`.
fn line_start(content: &str, byte: usize) -> usize {
    content[..byte.min(content.len())]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0)
}

/// Byte offset just past the end of the line containing `byte`.
fn line_end(content: &str, byte: usize) -> usize {
    let from = byte.min(content.len());
    content[from..]
        .find('\n')
        .map(|i| from + i + 1)
        .unwrap_or(content.len())
}
