//! The project graph: the code, what relates to what, and what each part is for.
//!
//! This replaces the trace graph, which drew requirements as a force-directed
//! node-link diagram. That view answered a question the requirements panel
//! already answers better, and it answered it as a hairball. The question it
//! could not answer, and the one people actually have, is about the *code*:
//! this module, this function — which requirement does it serve, what evidence
//! stands behind that, and is anything wrong with it?
//!
//! So the nodes here are code. Directories, files, and top-level declarations,
//! from the same tree-sitter symbol table the anchors resolve against. The
//! requirements are an overlay on top: a tint, a list, a badge.
//!
//! Three rules this module keeps, which the old graph did not:
//!
//! 1. **Untraced code is in the graph.** A graph of only the annotated parts
//!    flatters the project exactly the way a coverage map of only the covered
//!    lines would. Grey means "nothing claims this", and it has to be visible
//!    to mean anything.
//! 2. **Assurance aggregates by the minimum.** A file serving one `L4`
//!    requirement and one `L1` requirement is an `L1` file. Averaging is how a
//!    diagram comes to report a number nobody should act on.
//! 3. **Edges carry their confidence.** Containment is a fact. A reference
//!    edge found by matching a name in text is a guess, and it says so. An
//!    inferred edge drawn like a certain one is what makes a diagram
//!    untrustworthy.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::coverage::{LineCoverage, SpanCoverage};
use super::{Finding, Level, Role, TraceIndex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Directory,
    File,
    Declaration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    /// A directory contains a file; a file contains a declaration.
    Contains,
    /// One declaration mentions another.
    References,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Read off the file system or the parse tree. Not a guess.
    Certain,
    /// A name in one declaration's body matching another declaration's name.
    ///
    /// It is wrong in both directions: a shadowed local produces an edge that
    /// is not a call, and a dynamic dispatch produces a call with no edge. It
    /// is here because it needs no language server, so the graph is useful on a
    /// machine with no toolchain installed — but it is drawn differently, and
    /// this is why.
    Textual,
}

/// A finding, reduced to what a badge on a node needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeFinding {
    pub kind: String,
    pub message: String,
    pub blocking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// Stable across rebuilds: a directory or file is its project-relative
    /// path, a declaration is `path::symbol`.
    pub id: String,
    pub kind: NodeKind,
    /// What to draw in the tile: the last path segment, or the symbol name.
    pub name: String,
    pub parent: Option<String>,
    pub file: PathBuf,
    pub start_line: u32,
    pub end_line: u32,
    /// Size for the layout. Lines, so a large module is not drawn the same as
    /// a one-line one.
    pub lines: u32,
    /// `REQ-ID` or `REQ-ID.clause`, sorted and deduplicated.
    pub requirements: Vec<String>,
    /// The roles annotated here: `models`, `implements`, `tests`, `drt`,
    /// `proves`.
    pub roles: Vec<String>,
    /// Weakest assurance across this node's requirements. `None` is the grey:
    /// nothing claims this code.
    pub assurance: Option<Level>,
    /// True when an annotation here resolves to a body that has changed since
    /// the evidence was recorded.
    pub stale: bool,
    pub findings: Vec<NodeFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// Declarations left out because the cap was reached. Reported rather than
    /// dropped silently: a graph that quietly shows two thirds of a project is
    /// worse than one that shows a third and says so.
    pub omitted_declarations: usize,
    pub node_cap: usize,
}

/// How many declaration nodes to emit before giving up on the rest.
///
/// A real repository has thousands; a view that renders all of them hangs, and
/// a view that hangs is one nobody opens twice. Files and directories are never
/// capped, so the shape of the project survives even when its detail does not.
pub const DEFAULT_DECLARATION_CAP: usize = 600;

/// Build the graph from the index, the sources on disk, and the checker's
/// findings.
pub fn build(
    root: &Path,
    index: &TraceIndex,
    findings: &[Finding],
    declaration_cap: usize,
) -> ProjectGraph {
    let mut graph = ProjectGraph { node_cap: declaration_cap, ..ProjectGraph::default() };

    // Findings by the file they are about, so a node can carry its own.
    let mut findings_by_file: BTreeMap<PathBuf, Vec<&Finding>> = BTreeMap::new();
    for finding in findings {
        findings_by_file.entry(finding.file.clone()).or_default().push(finding);
    }

    // Links by file, then by the anchor they resolved to.
    let mut by_file: BTreeMap<&PathBuf, BTreeMap<String, Vec<&super::Link>>> = BTreeMap::new();
    for link in &index.links {
        by_file
            .entry(&link.anchor.file)
            .or_default()
            .entry(link.anchor.ident())
            .or_default()
            .push(link);
    }

    let mut directories: BTreeSet<PathBuf> = BTreeSet::new();
    let mut declarations_emitted = 0usize;
    // `(declaration id, name, file)` for the reference pass.
    let mut symbols: Vec<(String, String, PathBuf)> = Vec::new();
    let mut bodies: Vec<(String, String)> = Vec::new();

    for file in &index.scanned_files {
        // Requirement documents describe the project; they are not part of it.
        if file.extension().and_then(|e| e.to_str()) == Some("md") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(root.join(file)) else { continue };
        let lines = content.lines().count() as u32;

        for ancestor in file.ancestors().skip(1) {
            if ancestor.as_os_str().is_empty() {
                break;
            }
            directories.insert(ancestor.to_path_buf());
        }

        let anchors = by_file.get(file);
        let file_links: Vec<&super::Link> = anchors
            .map(|a| a.values().flatten().copied().collect())
            .unwrap_or_default();
        let file_findings = findings_by_file.get(file).cloned().unwrap_or_default();

        let parent = file
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_string_lossy().into_owned());

        graph.nodes.push(GraphNode {
            id: file.to_string_lossy().into_owned(),
            kind: NodeKind::File,
            name: file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| file.to_string_lossy().into_owned()),
            parent,
            file: file.clone(),
            start_line: 0,
            end_line: lines.saturating_sub(1),
            lines,
            requirements: requirements_of(&file_links),
            roles: roles_of(&file_links),
            assurance: weakest_of(index, &file_links),
            stale: file_links.iter().any(|l| is_stale(index, l)),
            // Every finding in the file, so a file-level badge is right even
            // when nothing narrower matches. A declaration below repeats the
            // ones that land inside it, which is what makes "where is the
            // problem" answerable by looking rather than by reading a list.
            findings: file_findings.iter().map(|f| badge(f)).collect(),
        });

        let declarations = super::anchor::declarations_in(&root.join(file), &content);
        let source_lines: Vec<&str> = content.lines().collect();

        for declaration in declarations {
            if declarations_emitted >= declaration_cap {
                graph.omitted_declarations += 1;
                continue;
            }
            declarations_emitted += 1;

            let id = format!("{}::{}", file.display(), declaration.name);
            // A link belongs to this declaration when its anchor starts inside
            // it. Comparing spans rather than names is what makes this work for
            // an explicit `begin`/`@end` region, which has no symbol at all.
            let links: Vec<&super::Link> = file_links
                .iter()
                .copied()
                .filter(|l| {
                    l.anchor.start_line >= declaration.start_line
                        && l.anchor.start_line <= declaration.end_line
                })
                .collect();

            let here: Vec<&Finding> = file_findings
                .iter()
                .copied()
                .filter(|f| {
                    f.line >= declaration.start_line && f.line <= declaration.end_line
                })
                .collect();

            let from = declaration.start_line as usize;
            let to = (declaration.end_line as usize + 1).min(source_lines.len());
            let body = source_lines[from.min(source_lines.len())..to].join("\n");

            symbols.push((id.clone(), declaration.name.clone(), file.clone()));
            bodies.push((id.clone(), body));

            graph.nodes.push(GraphNode {
                id,
                kind: NodeKind::Declaration,
                name: declaration.name,
                parent: Some(file.to_string_lossy().into_owned()),
                file: file.clone(),
                start_line: declaration.start_line,
                end_line: declaration.end_line,
                lines: declaration.end_line.saturating_sub(declaration.start_line) + 1,
                requirements: requirements_of(&links),
                roles: roles_of(&links),
                assurance: weakest_of(index, &links),
                stale: links.iter().any(|l| is_stale(index, l)),
                findings: here.iter().map(|f| badge(f)).collect(),
            });
        }
    }

    // A directory's size is the code inside it. Leaving it at zero would give
    // every directory a zero-area tile, which is the same bug the old coverage
    // map had: a rectangle whose area does not mean anything.
    let mut directory_lines: BTreeMap<PathBuf, u32> = BTreeMap::new();
    for node in &graph.nodes {
        if node.kind != NodeKind::File {
            continue;
        }
        for ancestor in node.file.ancestors().skip(1) {
            if ancestor.as_os_str().is_empty() {
                break;
            }
            *directory_lines.entry(ancestor.to_path_buf()).or_default() += node.lines;
        }
    }

    for directory in &directories {
        let id = directory.to_string_lossy().into_owned();
        let parent = directory
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_string_lossy().into_owned());
        graph.nodes.push(GraphNode {
            id: id.clone(),
            kind: NodeKind::Directory,
            name: directory
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(id),
            parent,
            file: directory.clone(),
            start_line: 0,
            end_line: 0,
            lines: directory_lines.get(directory).copied().unwrap_or(0),
            requirements: Vec::new(),
            roles: Vec::new(),
            assurance: None,
            stale: false,
            findings: Vec::new(),
        });
    }

    for node in &graph.nodes {
        if let Some(parent) = &node.parent {
            graph.edges.push(GraphEdge {
                from: parent.clone(),
                to: node.id.clone(),
                kind: EdgeKind::Contains,
                confidence: Confidence::Certain,
            });
        }
    }

    graph.edges.extend(reference_edges(&symbols, &bodies));
    graph
}

/// Reference edges, from one declaration's body naming another's symbol.
///
/// Word-boundary matching, and a declaration never references itself. Only
/// names of three characters or more: a one-letter binder matches everywhere
/// and would bury the real edges in noise.
///
/// Within one language only. A Python test mentioning `price` is not a
/// reference to the Lean `price`, and drawing that edge made the two most
/// interesting relations in the example project -- test to implementation, and
/// model to implementation -- indistinguishable from a coincidence of naming.
/// The model/implementation relation is real, but it is carried by the
/// annotations, which know it rather than guess it.
fn reference_edges(
    symbols: &[(String, String, PathBuf)],
    bodies: &[(String, String)],
) -> Vec<GraphEdge> {
    let mut edges = Vec::new();
    let language_of = |file: &Path| {
        file.extension().and_then(|e| e.to_str()).unwrap_or("").to_string()
    };
    let sources: BTreeMap<&str, &PathBuf> =
        symbols.iter().map(|(id, _, file)| (id.as_str(), file)).collect();

    for (from, body) in bodies {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let from_language = sources.get(from.as_str()).map(|f| language_of(f));
        for (to, name, file) in symbols {
            if to == from || name.len() < 3 || seen.contains(to.as_str()) {
                continue;
            }
            if from_language.as_deref() != Some(language_of(file).as_str()) {
                continue;
            }
            if mentions(body, name) {
                seen.insert(to.as_str());
                edges.push(GraphEdge {
                    from: from.clone(),
                    to: to.clone(),
                    kind: EdgeKind::References,
                    confidence: Confidence::Textual,
                });
            }
        }
    }
    edges
}

/// Does `body` contain `name` as a whole word?
fn mentions(body: &str, name: &str) -> bool {
    let bytes = body.as_bytes();
    let mut from = 0;
    while let Some(offset) = body[from..].find(name) {
        let start = from + offset;
        let end = start + name.len();
        let before_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_word_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = end.max(start + 1);
        if from >= body.len() {
            break;
        }
    }
    false
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

fn badge(finding: &Finding) -> NodeFinding {
    NodeFinding {
        kind: format!("{:?}", finding.kind),
        message: finding.message.clone(),
        blocking: finding.blocking,
    }
}

fn requirements_of(links: &[&super::Link]) -> Vec<String> {
    let mut out: Vec<String> = links
        .iter()
        .map(|l| match &l.clause {
            Some(clause) => format!("{}.{}", l.req_id, clause),
            None => l.req_id.clone(),
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

fn roles_of(links: &[&super::Link]) -> Vec<String> {
    let mut out: Vec<String> = links
        .iter()
        .map(|l| format!("{:?}", l.role).to_lowercase())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Weakest assurance across the requirements anchored here.
///
/// `None` when nothing is anchored here at all — which is the grey, and is not
/// the same as `L1`. `L1` says "somebody claims this and nothing has checked
/// it"; grey says "nobody has claimed it". Collapsing the two would make an
/// unannotated file look like an annotated one that has not been verified yet.
fn weakest_of(index: &TraceIndex, links: &[&super::Link]) -> Option<Level> {
    links
        .iter()
        .map(|l| index.assurance(&l.req_id, l.clause.as_deref()).weakest())
        .min()
}

/// Ordering for the weakest-link rule. An unasked question is weaker than an
/// unfinished answer, which is weaker than a proof. `nondeterministic` sits at
/// the bottom deliberately: it is a considered decision that applies to the
/// declaration as a whole, so one clause declaring it must not be overridden by
/// another clause that simply never asked.
fn strength_rank(state: &str) -> u8 {
    match state {
        "nondeterministic" => 0,
        "open" => 1,
        "attempted" => 2,
        "pinned" => 3,
        _ => 1,
    }
}

fn weaker_strength(a: String, b: String) -> String {
    if strength_rank(&a) <= strength_rank(&b) {
        a
    } else {
        b
    }
}

fn is_stale(index: &TraceIndex, link: &super::Link) -> bool {
    !index.assurance(&link.req_id, link.clause.as_deref()).stale.is_empty()
}

// --- The role graph --------------------------------------------------------
//
// A different question from the one above, and a different picture. The
// containment graph answers "how much of this project is untraced" -- a
// question about proportion, which nested boxes sized by lines answer well. It
// does not answer "what relates to what", and that is what somebody opening a
// project view is usually asking: which code serves this requirement, what
// models it, what evidence stands behind it.
//
// So this builds the graph of *annotations*: requirement clauses on the left,
// then the models, then the implementations, then the evidence. The edges are
// the roles somebody wrote down, which makes every edge in the picture a claim
// a person made rather than an inference.

/// Which column a node belongs in. The order is the argument the project
/// makes: a requirement is formalized by a model, realized by an
/// implementation, and backed by evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Column {
    Requirement,
    Model,
    Implementation,
    Evidence,
}

impl Column {
    fn for_role(role: Role) -> Column {
        match role {
            Role::Models => Column::Model,
            Role::Implements => Column::Implementation,
            Role::Tests | Role::Drt | Role::Proves | Role::Pins => Column::Evidence,
        }
    }
}

/// How a differential-testing binding is standing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessState {
    /// Present in `.tracelean/drt.json`. A binding can exist with no run behind
    /// it, which is a different state from a run that found nothing.
    pub bound: bool,
    pub cases: usize,
    pub divergences: usize,
    pub coverage_covered: usize,
    pub coverage_total: usize,
    /// The recorded run's inputs no longer match the code.
    pub stale: bool,
    /// No run at all yet.
    pub never_run: bool,
}

/// Tests claimed for a clause, and how they went.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TestState {
    /// `@tests` annotations pointing at this implementation.
    pub claimed: usize,
    /// How many passed, when a result file says. `None` means nobody has run
    /// them through TraceLean -- which is not the same as zero.
    pub passing: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleNode {
    /// `REQ-ID.clause` for a requirement, `path::symbol` for code.
    pub id: String,
    pub column: Column,
    /// What to draw: the clause key, or the symbol's last segment.
    pub label: String,
    /// The second line of the label: the requirement's title, or the file.
    pub sublabel: String,
    pub file: PathBuf,
    pub start_line: u32,
    pub end_line: u32,
    /// Roles annotated here, for the badge.
    pub roles: Vec<String>,
    pub assurance: Option<Level>,
    /// Spec strength. Set on model nodes as well as requirements, because it is
    /// a property of the model: do the theorems proved about *this* declaration
    /// determine it?
    pub strength: Option<String>,
    /// Line coverage over this declaration's span. Only ever set where a
    /// coverage tool actually measured the file — a node with no data shows no
    /// badge rather than 0%, because "nobody measured this" and "this never
    /// runs" are different facts.
    pub coverage: Option<SpanCoverage>,
    /// Tests, on the implementation they exercise.
    pub tests: Option<TestState>,
    /// Differential-testing state, on the harness node.
    pub harness: Option<HarnessState>,
    pub stale: bool,
    pub exempt: bool,
    pub partial: bool,
    pub findings: Vec<NodeFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleEdge {
    pub from: String,
    pub to: String,
    /// The annotation role this edge was written as.
    pub role: String,
    /// True when the annotation's target has changed since evidence was taken.
    pub stale: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleGraph {
    pub nodes: Vec<RoleNode>,
    pub edges: Vec<RoleEdge>,
    /// Requirement clauses with no annotation at all. Counted rather than
    /// drawn as floating nodes: an empty column of orphans crowds out the part
    /// of the picture that carries information, and the number is the finding.
    pub unlinked_clauses: Vec<String>,
}

/// Build the role graph: requirements, what models them, what implements them,
/// and what stands behind that.
pub fn role_graph(
    index: &TraceIndex,
    findings: &[Finding],
    coverage: &LineCoverage,
    tests: &super::test_results::TestResults,
    bindings: &[(String, Option<String>)],
) -> RoleGraph {
    let mut nodes: BTreeMap<String, RoleNode> = BTreeMap::new();
    let mut edges: Vec<RoleEdge> = Vec::new();
    let mut unlinked = Vec::new();

    let findings_for = |file: &Path, line: u32| -> Vec<NodeFinding> {
        findings
            .iter()
            .filter(|f| f.file == file && f.line == line)
            .map(|f| NodeFinding {
                kind: f.kind.as_str().to_string(),
                message: f.message.clone(),
                blocking: f.blocking,
            })
            .collect()
    };

    for req in index.requirements.values() {
        for clause in req.clause_keys() {
            let id = match &clause {
                Some(c) => format!("{}.{}", req.id, c),
                None => req.id.clone(),
            };
            let links = index.links_for_clause(&req.id, clause.as_deref());
            if links.is_empty() {
                unlinked.push(id);
                continue;
            }

            let assurance = index.assurance(&req.id, clause.as_deref());
            let strength = super::strength::declared(index, &req.id, clause.as_deref());
            nodes.insert(
                id.clone(),
                RoleNode {
                    label: clause.clone().unwrap_or_else(|| req.id.clone()),
                    sublabel: req.title.clone(),
                    column: Column::Requirement,
                    file: req.file.clone(),
                    start_line: 0,
                    end_line: 0,
                    roles: Vec::new(),
                    assurance: Some(assurance.weakest()),
                    strength: Some(strength.as_str().to_string()),
                    coverage: None,
                    tests: None,
                    harness: None,
                    stale: false,
                    exempt: links.iter().any(|l| l.is_exempt()),
                    partial: links.iter().any(|l| l.is_partial()),
                    findings: index
                        .findings
                        .iter()
                        .filter(|f| {
                            f.req_id.as_deref() == Some(req.id.as_str())
                                && f.clause == clause
                        })
                        .map(|f| NodeFinding {
                            kind: f.kind.as_str().to_string(),
                            message: f.message.clone(),
                            blocking: f.blocking,
                        })
                        .collect(),
                    id: id.clone(),
                },
            );

            for link in links {
                let Some(symbol) = link.anchor.symbol_path() else { continue };
                let code_id = format!("{}::{}", link.anchor.file.display(), symbol);
                let label = symbol.rsplit("::").next().unwrap_or(symbol).to_string();
                let column = Column::for_role(link.role);
                let entry = nodes.entry(code_id.clone()).or_insert_with(|| RoleNode {
                    id: code_id.clone(),
                    column,
                    label,
                    sublabel: link.anchor.file.display().to_string(),
                    file: link.anchor.file.clone(),
                    start_line: link.anchor.start_line,
                    end_line: link.anchor.end_line,
                    roles: Vec::new(),
                    assurance: None,
                    // A model's strength is the question asked of the
                    // declaration, so it is answered here and not only on the
                    // requirement that points at it.
                    strength: (column == Column::Model)
                        .then(|| strength.as_str().to_string()),
                    coverage: coverage.for_span(
                        &link.anchor.file,
                        link.anchor.start_line,
                        link.anchor.end_line,
                    ),
                    tests: (column == Column::Implementation)
                        .then_some(TestState { claimed: 0, passing: None }),
                    harness: None,
                    stale: false,
                    exempt: false,
                    partial: false,
                    findings: findings_for(&link.anchor.file, link.anchor.start_line),
                });
                // One declaration can carry several roles. The leftmost column
                // wins, so a function that both implements and is tested is
                // drawn once, where its primary claim is.
                if column < entry.column {
                    entry.column = column;
                }
                // A model serving several clauses takes the weakest answer, the
                // way assurance does. Keeping whichever clause happened to be
                // visited first would make the badge depend on map order.
                if column == Column::Model {
                    let candidate = strength.as_str().to_string();
                    entry.strength = Some(match entry.strength.take() {
                        Some(existing) => weaker_strength(existing, candidate),
                        None => candidate,
                    });
                }
                let role = link.role.as_str().to_string();
                if !entry.roles.contains(&role) {
                    entry.roles.push(role.clone());
                }
                entry.stale |= is_stale(index, link);
                entry.exempt |= link.is_exempt();
                entry.partial |= link.is_partial();

                edges.push(RoleEdge {
                    from: id.clone(),
                    to: code_id,
                    role,
                    stale: is_stale(index, link),
                });
            }

            // Tests belong to the implementation they exercise, not to a column
            // of their own: "is this code tested, and how well" is one question
            // about one node.
            let test_links: Vec<_> = index
                .links_for_clause(&req.id, clause.as_deref())
                .into_iter()
                .filter(|l| l.role == Role::Tests)
                .collect();
            let test_count = test_links.len();
            // Passing is only reported where a runner actually said so. A test
            // nobody has run through TraceLean contributes to the count and not
            // to the verdict, because "not measured" and "failed" are different
            // facts and collapsing them would make the badge a lie.
            let mut measured = 0usize;
            let mut passing = 0usize;
            for link in &test_links {
                let Some(symbol) = link.anchor.symbol_path() else { continue };
                let name = symbol.rsplit("::").next().unwrap_or(symbol);
                if let Some(outcome) = tests.outcome_for(name) {
                    measured += 1;
                    if outcome == super::test_results::Outcome::Passed {
                        passing += 1;
                    }
                }
            }
            for link in index.links_for_clause(&req.id, clause.as_deref()) {
                if link.role != Role::Implements {
                    continue;
                }
                let Some(symbol) = link.anchor.symbol_path() else { continue };
                let code_id = format!("{}::{}", link.anchor.file.display(), symbol);
                if let Some(node) = nodes.get_mut(&code_id) {
                    let previous = node.tests.unwrap_or(TestState { claimed: 0, passing: None });
                    let claimed = previous.claimed + test_count;
                    let passing = match (previous.passing, measured > 0) {
                        (Some(before), true) => Some(before + passing),
                        (Some(before), false) => Some(before),
                        (None, true) => Some(passing),
                        (None, false) => None,
                    };
                    node.tests = Some(TestState { claimed, passing });
                }
            }

            // The harness is a node of its own. It has no source declaration to
            // hang on -- the binding lives in `.tracelean/drt.json` and there is
            // no adapter file any more -- and leaving it out is why the state of
            // the thing that binds model to code was invisible in this picture.
            let bound = bindings
                .iter()
                .any(|(r, c)| *r == req.id && (c == &clause || c.is_none()));
            let record = index.evidence.iter().find(|e| {
                e.key.req_id == req.id
                    && e.key.clause == clause
                    && e.key.bond == super::evidence::Bond::ModelImpl
            });
            if bound || record.is_some() {
                let harness_id = format!("drt:{id}");
                let (cases, divergences, covered, total) = match record.map(|r| &r.detail) {
                    Some(super::evidence::EvidenceDetail::Drt {
                        cases,
                        divergences,
                        coverage_covered,
                        coverage_total,
                        ..
                    }) => (*cases, *divergences, *coverage_covered, *coverage_total),
                    _ => (0, 0, 0, 0),
                };
                let stale = record
                    .map(|r| !r.stale_inputs(&index.current_hashes(&req.id, clause.as_deref())).is_empty())
                    .unwrap_or(false);
                nodes.insert(
                    harness_id.clone(),
                    RoleNode {
                        id: harness_id.clone(),
                        column: Column::Evidence,
                        label: "differential test".to_string(),
                        sublabel: if !bound {
                            "no binding".to_string()
                        } else if record.is_none() {
                            "bound, never run".to_string()
                        } else if divergences > 0 {
                            format!("{divergences} divergence(s) in {cases} cases")
                        } else {
                            format!("{cases} cases, no divergence")
                        },
                        file: PathBuf::from(".tracelean/drt.json"),
                        start_line: 0,
                        end_line: 0,
                        roles: vec!["drt".to_string()],
                        assurance: record.map(|r| r.level),
                        strength: None,
                        coverage: None,
                        tests: None,
                        harness: Some(HarnessState {
                            bound,
                            cases,
                            divergences,
                            coverage_covered: covered,
                            coverage_total: total,
                            stale,
                            never_run: record.is_none(),
                        }),
                        stale,
                        exempt: false,
                        partial: false,
                        findings: Vec::new(),
                    },
                );
                edges.push(RoleEdge {
                    from: id.clone(),
                    to: harness_id,
                    role: "drt".into(),
                    stale,
                });
            }
        }
    }

    RoleGraph {
        nodes: nodes.into_values().collect(),
        edges,
        unlinked_clauses: unlinked,
    }
}
