//! Building the index: scan requirement documents and annotations, resolve
//! anchors, and report what could not be resolved.
//!
//! This is the shell. Every decision it makes is delegated to a pure function
//! in a sibling module; what lives here is walking a directory and reading
//! files.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::anchor::{self, Anchor, Lang};
use super::annotation::{Directive, Problem, Qualifier, Role};
use super::requirement::{self, ParseOutcome, Requirement};

/// A resolved link: an annotation that found its anchor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub role: Role,
    pub req_id: String,
    pub clause: Option<String>,
    pub qualifier: Option<Qualifier>,
    pub attrs: BTreeMap<String, String>,
    pub anchor: Anchor,
    /// Identity hash — changes when the link is retargeted, so evidence keyed
    /// on it cannot be inherited by a different claim.
    ///
    /// @implements REQ-STALE.retarget_invalidates
    pub link_hash: String,
    pub line: u32,
}

impl Link {
    pub fn is_exempt(&self) -> bool {
        matches!(self.qualifier, Some(Qualifier::Exempt { .. }))
    }

    /// Whether this clause is checked structurally rather than by a model.
    pub fn is_structural(&self) -> bool {
        matches!(self.qualifier, Some(Qualifier::Structural { .. }))
    }

    pub fn is_partial(&self) -> bool {
        matches!(self.qualifier, Some(Qualifier::Partial { .. }))
    }
}

/// Everything the traceability system knows about a project.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Index {
    pub requirements: BTreeMap<String, Requirement>,
    pub links: Vec<Link>,
    pub problems: Vec<FileProblem>,
    /// Documents that declare what they describe.
    ///
    /// @implements REQ-DOCLINK.declares_target
    pub doc_links: Vec<super::doclink::DocLink>,
    /// Files that were read, whether or not they carried anything.
    ///
    /// An untraced file is exactly what a coverage map needs to show, and a
    /// scan that only remembered annotated files would flatter the project.
    ///
    /// @implements ARCH-HONEST.untraced_visible
    pub scanned: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileProblem {
    pub file: String,
    pub line: u32,
    pub message: String,
    /// True when the file parsed badly rather than being written badly.
    #[serde(default)]
    pub imprecise: bool,
}

/// Directories never worth walking.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".tracelean",
    "target",
    "node_modules",
    ".lake",
    ".venv",
    "__pycache__",
];

/// Scan a project tree.
///
/// @implements REQ-REQDOC.id_is_identity
/// @implements ARCH-DETERMINISM.stable_ordering
pub fn build(root: &Path) -> Index {
    let mut index = Index::default();
    let mut declared: Vec<(String, String)> = Vec::new();
    let mut files = Vec::new();
    collect_files(root, root, &mut files);
    // Sorted so the index is a function of the tree, not of directory order.
    files.sort();

    for rel in files {
        let Ok(content) = std::fs::read_to_string(root.join(&rel)) else { continue };

        if rel.ends_with(".md") {
            // A document may both be a requirement and describe something, so
            // this runs regardless of what the parse below decides.
            index.doc_links.extend(super::doclink::links_in(&rel, &content));
            match requirement::parse_markdown(&rel, &content) {
                ParseOutcome::Requirement(req, problems) => {
                    for p in problems {
                        index.problems.push(FileProblem {
                            file: p.file,
                            line: p.line,
                            message: p.message,
                            imprecise: false,
                        });
                    }
                    // Declared in scan order; which of them are redeclarations
                    // is decided once, below, by a function that is modelled.
                    // The first declaration wins and the later ones are
                    // reported, so the file named is the one being ignored.
                    declared.push((req.id.clone(), req.file.clone()));
                    index.requirements.entry(req.id.clone()).or_insert(*req);
                }
                // Ordinary markdown, but a broken frontmatter fence is still
                // worth saying: it is why the requirement somebody expected
                // here is not in the index.
                ParseOutcome::NotARequirement(problems) => {
                    for p in problems {
                        index.problems.push(FileProblem {
                            file: p.file,
                            line: p.line,
                            message: p.message,
                            imprecise: false,
                        });
                    }
                }
            }
            index.scanned.insert(rel);
            continue;
        }

        let lang = Path::new(&rel).extension().and_then(|e| e.to_str()).and_then(Lang::from_extension);
        if lang.is_none() {
            continue;
        }
        index.scanned.insert(rel.clone());

        let scan = anchor::scan(&content, lang);
        // An annotation the parser could not read is reported rather than
        // dropped: a misspelt `@implments` that vanishes silently is worse than
        // no annotation at all, because the clause then reads as unimplemented
        // and nobody can see why.
        for problem in &scan.comments.problems {
            index.problems.push(file_problem(&rel, problem));
        }
        let links = links_in(&rel, &content, &scan);
        // Precision is a property of each anchor, not of the file: a tactic
        // block the grammar cannot read caps the annotations near it and leaves
        // the rest of the file alone. Only the ones actually affected are
        // reported, so the number is the cost rather than the file count.
        let imprecise = links.iter().filter(|link| !link.anchor.precise).count();
        if imprecise > 0 {
            index.problems.push(FileProblem {
                file: rel.clone(),
                line: 1,
                message: format!(
                    "{imprecise} annotation(s) sit in or after a region the grammar could not parse, so they are capped at L1"
                ),
                imprecise: true,
            });
        }
        index.links.extend(links);
    }

    for (id, file) in requirement::duplicate_ids(declared) {
        index.problems.push(FileProblem {
            file,
            line: 1,
            message: format!("duplicate requirement id `{id}`"),
            imprecise: false,
        });
    }

    index
}

fn file_problem(file: &str, problem: &Problem) -> FileProblem {
    FileProblem {
        file: file.to_string(),
        line: problem.line + 1,
        message: problem.message.clone(),
        imprecise: false,
    }
}

/// The links one file's text makes, as `build` would find them — for text not
/// on disk yet, such as a file being edited.
pub fn links_of(file: &str, content: &str) -> Vec<Link> {
    let lang = Path::new(file).extension().and_then(|e| e.to_str()).and_then(Lang::from_extension);
    if lang.is_none() {
        return Vec::new();
    }
    links_in(file, content, &anchor::scan(content, lang))
}

/// Resolve one file's directives into links.
///
/// A qualifier attaches to the annotation above it, which is why directives are
/// walked in source order rather than filtered by kind first.
///
/// @implements REQ-ANNOT.qualifiers
fn links_in(file: &str, content: &str, scan: &anchor::Scan) -> Vec<Link> {
    let mut links: Vec<Link> = Vec::new();
    // Where each line break is, so a byte's line is a search rather than a
    // count from the top — counting made a long, well-commented file slow.
    let breaks: Vec<usize> = content.match_indices('\n').map(|(at, _)| at).collect();
    let line_of = |byte: usize| breaks.partition_point(|at| *at < byte) as u32;

    for directive in &scan.comments.directives {
        match directive {
            Directive::Annotation { annotation: a } => {
                // The comment this was written in ends where the next
                // declaration may begin.
                let comment_end = scan
                    .comment_ranges
                    .iter()
                    .find(|(start, end)| line_of(*start) <= a.line && a.line <= line_of(*end))
                    .map(|(_, end)| *end)
                    .unwrap_or(0);

                let anchor = anchor::resolve(file, content, scan, a, comment_end);
                let link_hash = link_hash(a.role, &a.req_id, a.clause.as_deref(), &anchor.ident());
                links.push(Link {
                    role: a.role,
                    req_id: a.req_id.clone(),
                    clause: a.clause.clone(),
                    qualifier: None,
                    attrs: a.attrs.clone(),
                    anchor,
                    link_hash,
                    line: a.line + 1,
                });
            }
            Directive::Qualified { qualifier, req_id, clause, .. } => {
                // With its own identifier it qualifies that link; without one,
                // the nearest annotation above it.
                let target = match req_id {
                    Some(id) => links.iter_mut().rev().find(|l| {
                        l.req_id == *id && (clause.is_none() || l.clause == *clause)
                    }),
                    None => links.last_mut(),
                };
                if let Some(link) = target {
                    link.qualifier = Some(qualifier.clone());
                }
            }
            Directive::End { .. } => {}
        }
    }

    links
}

/// A link's identity: what it claims, and what it points at.
///
/// @implements REQ-STALE.retarget_invalidates
pub fn link_hash(role: Role, req_id: &str, clause: Option<&str>, anchor_ident: &str) -> String {
    let mut clauses = BTreeMap::new();
    clauses.insert("role".to_string(), role.as_str().to_string());
    clauses.insert("req".to_string(), req_id.to_string());
    clauses.insert("clause".to_string(), clause.unwrap_or("").to_string());
    clauses.insert("anchor".to_string(), anchor_ident.to_string());
    super::hash::requirement(&clauses, "")
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if SKIP_DIRS.contains(&name.as_ref()) || name.starts_with('.') {
                continue;
            }
            collect_files(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_src(src: &str) -> Vec<Link> {
        let scan = anchor::scan(src, Some(Lang::Rust));
        links_in("f.rs", src, &scan)
    }

    /// @tests REQ-ANNOT.qualifiers
    #[test]
    fn a_qualifier_attaches_to_the_annotation_above_it() {
        let src = "// @implements REQ-X.c\n// @partial reason=\"only the happy path\"\npub fn f() {}\n";
        let links = scan_src(src);
        assert_eq!(links.len(), 1);
        assert!(links[0].is_partial());
    }

    /// @tests REQ-STALE.retarget_invalidates
    #[test]
    fn retargeting_a_link_changes_its_identity() {
        let a = scan_src("// @implements REQ-X.c\npub fn alpha() {}\n");
        let b = scan_src("// @implements REQ-X.c\npub fn beta() {}\n");
        assert_ne!(a[0].link_hash, b[0].link_hash);
    }

    /// Editing the body must move the body hash but not the link's identity:
    /// the claim is the same, its evidence is not.
    #[test]
    fn editing_a_body_moves_the_body_hash_but_not_the_link_hash() {
        let a = scan_src("// @implements REQ-X.c\npub fn alpha() { 1 }\n");
        let b = scan_src("// @implements REQ-X.c\npub fn alpha() { 2 }\n");
        assert_eq!(a[0].link_hash, b[0].link_hash);
        assert_ne!(a[0].anchor.body_hash, b[0].anchor.body_hash);
    }

    #[test]
    fn a_qualifier_may_name_its_own_link() {
        let src = "// @implements REQ-A.c\npub fn alpha() {}\n\n// @implements REQ-B.c\n// @exempt REQ-A.c reason=\"platform\" by=ana\npub fn beta() {}\n";
        let links = scan_src(src);
        let a = links.iter().find(|l| l.req_id == "REQ-A").unwrap();
        assert!(a.is_exempt(), "the qualifier named REQ-A, not the link above it");
    }
}
