//! Requirement documents, identified by a frontmatter `id` — never by filename
//! or directory. Any markdown carrying an `id:` is a requirement; markdown
//! without one is ordinary markdown.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Whether a requirement's clauses are claimed to exhaust it.
///
/// This is the denominator of every percentage shown about the requirement.
/// Defaulting to `Open` means an author has to *claim* completeness before a
/// number is presented as exact.
///
/// @implements REQ-REQDOC.decomposition_claimed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decomposition {
    Complete,
    #[default]
    Open,
}

impl Decomposition {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "complete" => Decomposition::Complete,
            _ => Decomposition::Open,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Decomposition::Complete => "complete",
            Decomposition::Open => "open",
        }
    }
}

/// Workflow state. Orthogonal to evidence: a requirement can be approved with
/// no evidence at all, or draft and fully conformant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Draft,
    Approved,
    Linked,
}

impl Status {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "approved" => Status::Approved,
            "linked" => Status::Linked,
            _ => Status::Draft,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Draft => "draft",
            Status::Approved => "approved",
            Status::Linked => "linked",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String,
    pub title: String,
    /// Path relative to the project root. Carries no meaning — identity is `id`.
    pub file: String,
    pub refines: Vec<String>,
    pub decomposition: Decomposition,
    /// Clause key -> clause text, ordered so hashes and output are stable.
    pub clauses: BTreeMap<String, String>,
    pub status: Status,
    pub body: String,
    /// Hash of the semantic content, driving re-judgement when it changes.
    pub content_hash: String,
}

impl Requirement {
    /// Clause keys, or a single implicit clause when the document declares
    /// none — so callers treat requirement-level links uniformly.
    ///
    /// @implements REQ-REQDOC.clauseless_uniform
    pub fn clause_keys(&self) -> Vec<Option<String>> {
        if self.clauses.is_empty() {
            vec![None]
        } else {
            self.clauses.keys().map(|k| Some(k.clone())).collect()
        }
    }

    pub fn has_clause(&self, clause: &str) -> bool {
        self.clauses.contains_key(clause)
    }
}

/// What is wrong with a line of frontmatter.
///
/// Named, so that a report can say which rule was broken and a differential
/// test can compare conclusions rather than two implementations' phrasing of
/// the same complaint.
///
/// @implements ARCH-HONEST.named_findings
/// @implements REQ-REQDOC.malformed_reported
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FrontmatterKind {
    /// An indented entry with no map key above it.
    IndentedWithoutKey,
    /// A line that is not `key: value`.
    NotAKeyValue,
    /// `id:` is present and empty, so the document claims to be a requirement
    /// and does not say which one.
    EmptyId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontmatterProblem {
    pub file: String,
    pub line: u32,
    pub kind: FrontmatterKind,
    pub message: String,
}

pub enum ParseOutcome {
    Requirement(Box<Requirement>, Vec<FrontmatterProblem>),
    /// Ordinary markdown — but whatever was wrong with its frontmatter is
    /// carried out rather than dropped. A document that is malformed *and* not
    /// a requirement is the case where silent skipping hides the reason the
    /// requirement everybody expected is missing.
    ///
    /// @implements REQ-REQDOC.malformed_reported
    NotARequirement(Vec<FrontmatterProblem>),
}

/// Parse a markdown document into a requirement, if it declares an `id`.
///
/// The supported frontmatter is deliberately tiny — `key: value`,
/// `key: [a, b]`, and one level of nesting under a map key — so there is no
/// YAML dependency and the failure modes stay predictable.
///
/// @implements REQ-REQDOC.id_is_identity
/// @implements REQ-REQDOC.malformed_reported
pub fn parse_markdown(file: &str, content: &str) -> ParseOutcome {
    let mut problems = Vec::new();

    let Some((frontmatter, body)) = split_frontmatter(content) else {
        return ParseOutcome::NotARequirement(problems);
    };
    let fields = parse_frontmatter(frontmatter, file, &mut problems);

    let Some(id) = fields.scalars.get("id").cloned() else {
        return ParseOutcome::NotARequirement(problems);
    };
    if id.is_empty() {
        problems.push(FrontmatterProblem {
            file: file.to_string(),
            line: 1,
            kind: FrontmatterKind::EmptyId,
            message: "`id:` is present but empty".into(),
        });
        return ParseOutcome::NotARequirement(problems);
    }

    let title = fields
        .scalars
        .get("title")
        .cloned()
        .or_else(|| first_heading(body))
        .unwrap_or_else(|| id.clone());
    let clauses = fields.maps.get("clauses").cloned().unwrap_or_default();
    let content_hash = super::hash::requirement(&clauses, body);

    ParseOutcome::Requirement(
        Box::new(Requirement {
            id,
            title,
            file: file.to_string(),
            refines: fields.lists.get("refines").cloned().unwrap_or_default(),
            decomposition: fields
                .scalars
                .get("decomposition")
                .map(|s| Decomposition::parse(s))
                .unwrap_or_default(),
            clauses,
            status: fields.scalars.get("status").map(|s| Status::parse(s)).unwrap_or_default(),
            body: body.to_string(),
            content_hash,
        }),
        problems,
    )
}

fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let rest = content.strip_prefix("---\n").or_else(|| content.strip_prefix("---\r\n"))?;
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']).trim_end() == "---" {
            return Some((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

#[derive(Default)]
struct Fields {
    scalars: BTreeMap<String, String>,
    lists: BTreeMap<String, Vec<String>>,
    maps: BTreeMap<String, BTreeMap<String, String>>,
}

fn parse_frontmatter(text: &str, file: &str, problems: &mut Vec<FrontmatterProblem>) -> Fields {
    let mut fields = Fields::default();
    let mut current_map: Option<String> = None;

    for (index, raw) in text.lines().enumerate() {
        let line_no = index as u32 + 2; // +1 opening fence, +1 for 1-indexing
        let line = raw.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }

        let indented = line.starts_with(' ') || line.starts_with('\t');
        if indented {
            let Some(map_key) = current_map.clone() else {
                problems.push(FrontmatterProblem {
                    file: file.to_string(),
                    line: line_no,
                    kind: FrontmatterKind::IndentedWithoutKey,
                    message: "indented entry without a key above it".into(),
                });
                continue;
            };
            match line.trim().split_once(':') {
                Some((k, v)) => {
                    fields
                        .maps
                        .entry(map_key)
                        .or_default()
                        .insert(k.trim().to_string(), v.trim().to_string());
                }
                None => problems.push(FrontmatterProblem {
                    file: file.to_string(),
                    line: line_no,
                    kind: FrontmatterKind::NotAKeyValue,
                    message: format!("`{}` is not a `key: value` entry", line.trim()),
                }),
            }
            continue;
        }

        current_map = None;
        let Some((key, value)) = line.split_once(':') else {
            problems.push(FrontmatterProblem {
                file: file.to_string(),
                line: line_no,
                kind: FrontmatterKind::NotAKeyValue,
                message: format!("`{line}` is not a `key: value` entry"),
            });
            continue;
        };
        let key = key.trim().to_string();
        let value = value.trim();

        if value.is_empty() {
            current_map = Some(key);
            continue;
        }
        if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            let items = inner
                .split(',')
                .map(|item| item.trim().trim_matches(['"', '\'']).to_string())
                .filter(|item| !item.is_empty())
                .collect();
            fields.lists.insert(key, items);
            continue;
        }
        fields.scalars.insert(key, value.trim_matches(['"', '\'']).to_string());
    }

    fields
}

fn first_heading(body: &str) -> Option<String> {
    body.lines()
        .find(|line| line.starts_with("# "))
        .map(|line| line[2..].trim().to_string())
}

/// Identifiers declared more than once, each with the file of the later
/// declaration.
///
/// Identity is the declared `id`, so two documents declaring one are the same
/// requirement said twice — a fault whichever file it is in. The later
/// declaration is the one named, because the earlier one is the one the index
/// kept and a reader needs to be sent to the other.
///
/// @implements REQ-REQDOC.id_unique
/// @drt REQ-REQDOC.id_unique
pub fn duplicate_ids(declared: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();
    for (id, file) in declared {
        if !seen.insert(id.clone()) {
            out.push((id, file));
        }
    }
    out
}

/// The refinement graph, as identifiers and what they refine.
///
/// The graph questions are about identifiers and edges, and nothing else a
/// requirement carries. Taking the graph rather than the documents is what lets
/// them be checked on their own.
///
/// @implements ARCH-CORE-SHELL.decision_total
pub fn refinement_graph(requirements: &BTreeMap<String, Requirement>) -> BTreeMap<String, Vec<String>> {
    requirements.iter().map(|(id, req)| (id.clone(), req.refines.clone())).collect()
}

/// Both graph questions at once: is there a cycle, and what does not resolve.
///
/// Together because a reader has to be able to trust that they answer about the
/// same graph — a parent that does not exist is a dangling reference and *not*
/// a cycle, and an implementation that treated it as one would report a
/// blocking fault for a typo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphReport {
    pub cycle: Option<Vec<String>>,
    pub dangling: Vec<(String, String)>,
}

/// @implements REQ-REQDOC.refines_dag
/// @implements REQ-REQDOC.refines_resolves
/// @drt REQ-REQDOC.refines_dag
/// @drt REQ-REQDOC.refines_resolves
pub fn graph_report(edges: Vec<(String, Vec<String>)>) -> GraphReport {
    // Through a map, so a repeated identifier resolves the way the index
    // resolves it rather than the way a list happens to be ordered.
    let graph: BTreeMap<String, Vec<String>> = edges.into_iter().collect();
    GraphReport {
        cycle: cycle_in(&graph),
        dangling: dangling_in(&graph).into_iter().collect(),
    }
}

/// A cycle in the refinement graph, as the identifiers on it.
///
/// @implements REQ-REQDOC.refines_dag
pub fn refinement_cycle(requirements: &BTreeMap<String, Requirement>) -> Option<Vec<String>> {
    cycle_in(&refinement_graph(requirements))
}

fn cycle_in(requirements: &BTreeMap<String, Vec<String>>) -> Option<Vec<String>> {
    // Iterative depth-first search with an explicit stack, so a deep graph
    // cannot overflow on a machine with a small stack.
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Open,
        Done,
    }
    let mut marks: BTreeMap<&str, Mark> = BTreeMap::new();

    for root in requirements.keys() {
        if marks.get(root.as_str()) == Some(&Mark::Done) {
            continue;
        }
        let mut path: Vec<&str> = vec![root.as_str()];
        let mut next: Vec<usize> = vec![0];
        marks.insert(root.as_str(), Mark::Open);

        while let Some(&node) = path.last() {
            let parents = requirements.get(node).map(|r| r.as_slice()).unwrap_or(&[]);
            let index = *next.last().expect("parallel stacks");
            if index >= parents.len() {
                marks.insert(node, Mark::Done);
                path.pop();
                next.pop();
                continue;
            }
            *next.last_mut().expect("parallel stacks") += 1;
            let parent = parents[index].as_str();
            // A parent that does not exist is a dangling reference, reported by
            // the checker; it is not a cycle.
            if !requirements.contains_key(parent) {
                continue;
            }
            match marks.get(parent) {
                Some(Mark::Open) => {
                    let start = path.iter().position(|n| *n == parent).unwrap_or(0);
                    let mut cycle: Vec<String> =
                        path[start..].iter().map(|s| s.to_string()).collect();
                    cycle.push(parent.to_string());
                    return Some(cycle);
                }
                Some(Mark::Done) => continue,
                None => {
                    marks.insert(parent, Mark::Open);
                    path.push(parent);
                    next.push(0);
                }
            }
        }
    }
    None
}

/// Identifiers named by `refines:` that no document declares.
///
/// @implements REQ-REQDOC.refines_resolves
pub fn dangling_refines(requirements: &BTreeMap<String, Requirement>) -> BTreeSet<(String, String)> {
    dangling_in(&refinement_graph(requirements))
}

fn dangling_in(requirements: &BTreeMap<String, Vec<String>>) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for (id, refines) in requirements {
        for parent in refines {
            if !requirements.contains_key(parent) {
                out.insert((id.clone(), parent.clone()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "---\nid: REQ-EVID\ntitle: Evidence algebra\nrefines: [ARCH-HONEST, ARCH-CORE-SHELL]\nstatus: approved\ndecomposition: complete\nclauses:\n  ladder: Levels are ordered.\n  weakest_link: The minimum over bonds.\n---\n\n# Evidence algebra\n\nProse.\n";

    fn parse(content: &str) -> Requirement {
        match parse_markdown("reqs/x.md", content) {
            ParseOutcome::Requirement(r, problems) => {
                assert!(problems.is_empty(), "{problems:?}");
                *r
            }
            ParseOutcome::NotARequirement(_) => panic!("expected a requirement"),
        }
    }

    /// @tests REQ-REQDOC.id_is_identity
    #[test]
    fn frontmatter_is_read() {
        let req = parse(DOC);
        assert_eq!(req.id, "REQ-EVID");
        assert_eq!(req.title, "Evidence algebra");
        assert_eq!(req.refines, vec!["ARCH-HONEST", "ARCH-CORE-SHELL"]);
        assert_eq!(req.status, Status::Approved);
        assert_eq!(req.decomposition, Decomposition::Complete);
        assert_eq!(req.clauses.len(), 2);
        assert_eq!(req.clauses["weakest_link"], "The minimum over bonds.");
    }

    /// @tests REQ-REQDOC.decomposition_claimed
    #[test]
    fn decomposition_defaults_to_open() {
        let req = parse("---\nid: REQ-X\n---\nbody");
        assert_eq!(req.decomposition, Decomposition::Open);
        assert_eq!(req.status, Status::Draft);
    }

    #[test]
    fn markdown_without_an_id_is_not_a_requirement() {
        assert!(matches!(
            parse_markdown("notes.md", "# Notes\n\nprose"),
            ParseOutcome::NotARequirement(_)
        ));
        assert!(matches!(
            parse_markdown("notes.md", "---\ntitle: Notes\n---\nprose"),
            ParseOutcome::NotARequirement(_)
        ));
    }

    /// @tests REQ-REQDOC.clauseless_uniform
    #[test]
    fn a_clauseless_requirement_has_one_implicit_clause() {
        let req = parse("---\nid: REQ-X\n---\nbody");
        assert_eq!(req.clause_keys(), vec![None]);
    }

    /// @tests REQ-REQDOC.malformed_reported
    #[test]
    fn malformed_frontmatter_is_reported_not_skipped() {
        match parse_markdown("reqs/x.md", "---\nid: REQ-X\nthis line has no colon\n---\nbody") {
            ParseOutcome::Requirement(req, problems) => {
                assert_eq!(req.id, "REQ-X");
                assert_eq!(problems.len(), 1);
                assert_eq!(problems[0].line, 3);
            }
            ParseOutcome::NotARequirement(_) => panic!("a malformed line must not hide the document"),
        }
    }

    /// The hash is over content, so prose edits move it and the judgement that
    /// was about the old pair stops applying.
    #[test]
    fn content_hash_tracks_clauses_and_prose() {
        let a = parse(DOC);
        let b = parse(&DOC.replace("The minimum over bonds.", "The mean over bonds."));
        let c = parse(&DOC.replace("Prose.", "Different prose."));
        assert_ne!(a.content_hash, b.content_hash);
        assert_ne!(a.content_hash, c.content_hash);
    }

    fn graph(edges: &[(&str, &[&str])]) -> BTreeMap<String, Requirement> {
        edges
            .iter()
            .map(|(id, parents)| {
                let doc = format!(
                    "---\nid: {id}\nrefines: [{}]\n---\nbody",
                    parents.join(", ")
                );
                (id.to_string(), parse(&doc))
            })
            .collect()
    }

    /// @tests REQ-REQDOC.refines_dag
    #[test]
    fn a_cycle_is_found() {
        let g = graph(&[("A", &["B"]), ("B", &["C"]), ("C", &["A"])]);
        let cycle = refinement_cycle(&g).expect("a cycle");
        assert!(cycle.len() >= 3, "{cycle:?}");
    }

    #[test]
    fn a_diamond_is_not_a_cycle() {
        // A requirement may refine several, so a shared ancestor is ordinary.
        let g = graph(&[("A", &["B", "C"]), ("B", &["D"]), ("C", &["D"]), ("D", &[])]);
        assert_eq!(refinement_cycle(&g), None);
    }

    #[test]
    fn a_self_refinement_is_a_cycle() {
        let g = graph(&[("A", &["A"])]);
        assert!(refinement_cycle(&g).is_some());
    }

    /// @tests REQ-REQDOC.refines_resolves
    #[test]
    fn dangling_parents_are_listed_and_are_not_cycles() {
        let g = graph(&[("A", &["MISSING"])]);
        assert_eq!(refinement_cycle(&g), None);
        assert_eq!(
            dangling_refines(&g).into_iter().collect::<Vec<_>>(),
            vec![("A".to_string(), "MISSING".to_string())]
        );
    }
}

/// What parsing one document concluded, in a shape a model can be compared
/// against.
///
/// The file path is deliberately absent. Identity is the declared `id`, and a
/// projection that carried the path would let a model agree with an
/// implementation that had quietly started using it.
///
/// @implements REQ-REQDOC.id_is_identity
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parsed {
    /// Whether the document declared itself a requirement at all.
    pub is_requirement: bool,
    pub id: String,
    pub title: String,
    pub refines: Vec<String>,
    pub decomposition: Decomposition,
    pub status: Status,
    #[serde(with = "crate::wire::pairs")]
    pub clauses: BTreeMap<String, String>,
    /// The clauses a link may attach to: the declared keys, or one implicit
    /// clause when there are none.
    pub addressable: Vec<Option<String>>,
    /// Line and kind only. Two implementations cannot be expected to phrase a
    /// complaint the same way, and the phrasing is not the claim.
    pub problems: Vec<(u32, FrontmatterKind)>,
}

/// Parse a document given as lines, and report what it concluded.
///
/// Lines rather than one string because that is what a generator can produce
/// interesting cases of: a frontmatter fence in the wrong place, an indented
/// line with nothing above it, a list that is not closed. Joined with `\n`,
/// which is the only line ending this format has.
///
/// @implements REQ-REQDOC.id_is_identity
/// @implements REQ-REQDOC.id_unique
/// @implements REQ-REQDOC.clauseless_uniform
/// @implements REQ-REQDOC.decomposition_claimed
/// @implements REQ-REQDOC.malformed_reported
/// @drt REQ-REQDOC.id_is_identity
/// @drt REQ-REQDOC.clauseless_uniform
/// @drt REQ-REQDOC.decomposition_claimed
/// @drt REQ-REQDOC.malformed_reported
pub fn parse_lines(lines: Vec<String>) -> Parsed {
    let content = lines.join("\n");
    match parse_markdown("", &content) {
        ParseOutcome::NotARequirement(problems) => Parsed {
            is_requirement: false,
            id: String::new(),
            title: String::new(),
            refines: Vec::new(),
            decomposition: Decomposition::Open,
            status: Status::Draft,
            clauses: BTreeMap::new(),
            addressable: Vec::new(),
            // A document that is not a requirement is not silently skipped:
            // whatever was wrong with its frontmatter is still reported.
            problems: problems.iter().map(|p| (p.line, p.kind)).collect(),
        },
        ParseOutcome::Requirement(req, problems) => Parsed {
            is_requirement: true,
            id: req.id.clone(),
            title: req.title.clone(),
            refines: req.refines.clone(),
            decomposition: req.decomposition,
            status: req.status,
            clauses: req.clauses.clone(),
            addressable: req.clause_keys(),
            problems: problems.iter().map(|p| (p.line, p.kind)).collect(),
        },
    }
}
