use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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
    pub file: String,
    pub refines: Vec<String>,
    pub decomposition: Decomposition,
    pub clauses: BTreeMap<String, String>,
    #[serde(default)]
    pub narrowings: BTreeMap<String, BTreeMap<String, String>>,
    pub status: Status,
    pub body: String,
    pub content_hash: String,
}

impl Requirement {
    /// @implements REQ-REQDOC.clause_addressable
    /// @implements REQ-STALE.requirement_reopens_all
    pub fn clause_hash(&self, clause: Option<&str>) -> Option<String> {
        let none = BTreeMap::new();
        match clause {
            None if self.clauses.is_empty() => Some(super::hash::clause(None, &self.body, &none)),
            None => Some(self.content_hash.clone()),
            Some(key) => self.clauses.get(key).map(|text| {
                super::hash::clause(Some(key), text, self.narrowings.get(key).unwrap_or(&none))
            }),
        }
    }

    pub fn clause_text_with_narrowings(&self, clause: &str) -> Option<String> {
        let text = self.clauses.get(clause)?;
        let mut out = text.clone();
        for (key, narrowing) in self.narrowings.get(clause).into_iter().flatten() {
            out.push_str(&format!("\n  {key}: {narrowing}"));
        }
        Some(out)
    }

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

/// @implements ARCH-HONEST.named_findings
/// @implements REQ-REQDOC.malformed_reported
/// @implements REQ-REQDOC.clause_unique
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FrontmatterKind {
    NotAKeyValue,
    DuplicateClause,
    DuplicateNarrowing,
    MissingText,
    InvalidKey,
}

const KEYS: [&str; 6] = ["id", "title", "refines", "status", "decomposition", "clauses"];

fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
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
    /// @implements REQ-REQDOC.malformed_reported
    NotARequirement(Vec<FrontmatterProblem>),
}

/// @implements REQ-REQDOC.id_is_identity
/// @implements REQ-REQDOC.malformed_reported
/// @implements REQ-REQDOC.narrowings_nest
/// @implements REQ-REQDOC.clause_unique
pub fn parse_markdown(file: &str, content: &str) -> ParseOutcome {
    parse_prose(file, content).unwrap_or(ParseOutcome::NotARequirement(Vec::new()))
}

/// A requirement written as plain markdown:
///
/// ```text
/// # REQ-X: The title
///
/// - status: approved
/// - refines: REQ-A, REQ-B
///
/// Prose, as much as is wanted.
///
/// ## Clauses
///
/// - **holds**: The system shall hold.
///   - empty: With nothing, it still holds.
/// ```
///
/// The first line names it, the bullets under it say what it is, and the clauses are a list.
/// `None` when the text is not written this way.
///
/// @implements REQ-REQDOC.markdown_document
fn parse_prose(file: &str, content: &str) -> Option<ParseOutcome> {
    let lines: Vec<&str> = content.lines().collect();
    let first = lines.iter().position(|l| !l.trim().is_empty())?;
    let heading = lines[first].strip_prefix("# ")?;
    let (id, title) = match heading.split_once(':') {
        Some((id, title)) => (id.trim(), title.trim()),
        None => (heading.trim(), ""),
    };
    let named = !id.is_empty()
        && !id.contains(char::is_whitespace)
        && id.contains('-')
        && id.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !named {
        return None;
    }
    let mut problems = Vec::new();
    let at = |index: usize| index as u32 + 1;

    // The bullets straight under the heading that name a key of the requirement.
    let mut scalars: BTreeMap<String, String> = BTreeMap::new();
    let mut lists: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut next = first + 1;
    let mut after_meta = next;
    while next < lines.len() {
        let line = lines[next].trim_end();
        if line.trim().is_empty() {
            next += 1;
            continue;
        }
        let Some((key, value)) = line.strip_prefix("- ").and_then(|b| b.split_once(':')) else { break };
        let key = key.trim().to_ascii_lowercase();
        if !KEYS.contains(&key.as_str()) || key == "clauses" || key == "id" || key == "title" {
            break;
        }
        let value = value.trim();
        if key == "refines" {
            let items = value
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(|item| item.trim().trim_matches(['`', '"', '\'']).to_string())
                .filter(|item| !item.is_empty())
                .collect();
            lists.insert(key, items);
        } else {
            scalars.insert(key, value.to_string());
        }
        next += 1;
        after_meta = next;
    }

    // The prose, and the clauses list under `## Clauses`.
    let mut prose: Vec<&str> = Vec::new();
    let mut clauses: BTreeMap<String, String> = BTreeMap::new();
    let mut narrowings: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut in_clauses = false;
    let mut last: Option<String> = None;
    let mut started: BTreeMap<String, u32> = BTreeMap::new();
    let mut fenced = false;
    for (index, raw) in lines.iter().enumerate().skip(after_meta) {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fenced = !fenced;
        }
        if !fenced && line.trim() == "## Clauses" {
            in_clauses = true;
            continue;
        }
        if !fenced && in_clauses && line.starts_with("## ") {
            in_clauses = false;
        }
        if !in_clauses {
            prose.push(raw);
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let item = line.trim_start().strip_prefix("- ");
        match (indent, item) {
            (0, Some(entry)) => {
                let named = entry.strip_prefix("**").and_then(|e| e.split_once("**:"));
                let Some((key, text)) = named else {
                    problems.push(FrontmatterProblem {
                        file: file.to_string(),
                        line: at(index),
                        kind: FrontmatterKind::NotAKeyValue,
                        message: format!("`{}` is not a `- **key**: text` clause", line.trim()),
                    });
                    continue;
                };
                let key = key.trim().to_string();
                if !valid_key(&key) {
                    problems.push(FrontmatterProblem {
                        file: file.to_string(),
                        line: at(index),
                        kind: FrontmatterKind::InvalidKey,
                        message: format!("`{key}` is not a valid clause key"),
                    });
                }
                if clauses.contains_key(&key) {
                    problems.push(FrontmatterProblem {
                        file: file.to_string(),
                        line: at(index),
                        kind: FrontmatterKind::DuplicateClause,
                        message: format!("the clause `{key}` is declared twice"),
                    });
                }
                started.entry(key.clone()).or_insert(at(index));
                clauses.insert(key.clone(), text.trim().to_string());
                last = Some(key);
            }
            (_, Some(entry)) => {
                let Some(clause) = last.clone() else { continue };
                let Some((key, text)) = entry.split_once(':') else {
                    problems.push(FrontmatterProblem {
                        file: file.to_string(),
                        line: at(index),
                        kind: FrontmatterKind::NotAKeyValue,
                        message: format!("`{}` is not a `key: text` narrowing", entry.trim()),
                    });
                    continue;
                };
                let (key, text) = (key.trim(), text.trim());
                if !valid_key(key) {
                    problems.push(FrontmatterProblem {
                        file: file.to_string(),
                        line: at(index),
                        kind: FrontmatterKind::InvalidKey,
                        message: format!("`{key}` is not a valid narrowing key"),
                    });
                }
                let held = narrowings.entry(clause.clone()).or_default();
                if held.contains_key(key) {
                    problems.push(FrontmatterProblem {
                        file: file.to_string(),
                        line: at(index),
                        kind: FrontmatterKind::DuplicateNarrowing,
                        message: format!("`{clause}.{key}` is given twice"),
                    });
                }
                held.insert(key.to_string(), text.to_string());
            }
            (_, None) => {
                // A line that goes on with the clause above it.
                if let Some(text) = last.as_ref().and_then(|k| clauses.get_mut(k)) {
                    text.push(' ');
                    text.push_str(line.trim());
                }
            }
        }
    }
    for (key, text) in &clauses {
        if text.is_empty() {
            problems.push(FrontmatterProblem {
                file: file.to_string(),
                line: started.get(key).copied().unwrap_or(at(first)),
                kind: FrontmatterKind::MissingText,
                message: format!("the clause `{key}` has no text"),
            });
        }
    }

    let title = if title.is_empty() { id.to_string() } else { title.to_string() };
    let narrowings: BTreeMap<String, BTreeMap<String, String>> =
        narrowings.into_iter().filter(|(_, n)| !n.is_empty()).collect();
    let mut whole = clauses.clone();
    for (clause, entries) in &narrowings {
        for (key, text) in entries {
            whole.insert(format!("{clause}.{key}"), text.clone());
        }
    }
    // The document as a whole keeps its title heading and its prose, as it always has.
    let prose = prose.join("\n");
    let body = format!("# {title}\n\n{}\n", prose.trim_matches('\n'));
    let content_hash = super::hash::requirement(&whole, &body);
    Some(ParseOutcome::Requirement(
        Box::new(Requirement {
            id: id.to_string(),
            title,
            file: file.to_string(),
            refines: lists.get("refines").cloned().unwrap_or_default(),
            decomposition: scalars.get("decomposition").map(|s| Decomposition::parse(s)).unwrap_or_default(),
            clauses,
            narrowings,
            status: scalars.get("status").map(|s| Status::parse(s)).unwrap_or_default(),
            body,
            content_hash,
        }),
        problems,
    ))
}

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

/// @implements ARCH-CORE-SHELL.decision_total
pub fn refinement_graph(requirements: &BTreeMap<String, Requirement>) -> BTreeMap<String, Vec<String>> {
    requirements.iter().map(|(id, req)| (id.clone(), req.refines.clone())).collect()
}

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
    let graph: BTreeMap<String, Vec<String>> = edges.into_iter().collect();
    GraphReport {
        cycle: cycle_in(&graph),
        dangling: dangling_in(&graph).into_iter().collect(),
    }
}

/// @implements REQ-REQDOC.refines_dag
pub fn refinement_cycle(requirements: &BTreeMap<String, Requirement>) -> Option<Vec<String>> {
    cycle_in(&refinement_graph(requirements))
}

fn cycle_in(requirements: &BTreeMap<String, Vec<String>>) -> Option<Vec<String>> {
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

    const DOC: &str = "# REQ-EVID: Evidence algebra\n\n- status: approved\n- decomposition: complete\n- refines: ARCH-HONEST, ARCH-CORE-SHELL\n\nProse.\n\n## Clauses\n\n- **ladder**: Levels are ordered.\n- **weakest_link**: The minimum over bonds.\n";

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
        let req = parse("# REQ-X: X\n\nbody\n");
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
        let req = parse("# REQ-X: X\n\nbody\n");
        assert_eq!(req.clause_keys(), vec![None]);
    }

    /// @tests REQ-REQDOC.malformed_reported
    #[test]
    fn malformed_frontmatter_is_reported_not_skipped() {
        match parse_markdown("reqs/x.md", "# REQ-X: X\n\n## Clauses\n\n- not a clause\n") {
            ParseOutcome::Requirement(req, problems) => {
                assert_eq!(req.id, "REQ-X");
                assert_eq!(problems.len(), 1);
                assert_eq!(problems[0].line, 5);
            }
            ParseOutcome::NotARequirement(_) => panic!("a malformed line must not hide the document"),
        }
    }

    #[test]
    fn content_hash_tracks_clauses_and_prose() {
        let a = parse(DOC);
        let b = parse(&DOC.replace("The minimum over bonds.", "The mean over bonds."));
        let c = parse(&DOC.replace("Prose.", "Different prose."));
        assert_ne!(a.content_hash, b.content_hash);
        assert_ne!(a.content_hash, c.content_hash);
    }

    fn kinds(content: &str) -> Vec<(u32, FrontmatterKind)> {
        match parse_markdown("reqs/x.md", content) {
            ParseOutcome::Requirement(_, problems) | ParseOutcome::NotARequirement(problems) => {
                problems.iter().map(|p| (p.line, p.kind)).collect()
            }
        }
    }

    const BLOCK: &str = "# REQ-R: R\n\nProse.\n\n## Clauses\n\n- **weakest_link**: The minimum over bonds.\n- **min_not_mean**: The minimum of its parts.\n  - empty: With no parts, L1.\n  - order: Ascending id.\n";

    /// @tests REQ-REQDOC.narrowings_nest
    #[test]
    fn a_block_clause_carries_its_narrowings() {
        let req = parse(BLOCK);
        assert_eq!(req.clauses["min_not_mean"], "The minimum of its parts.");
        assert_eq!(req.clauses["weakest_link"], "The minimum over bonds.");
        assert_eq!(req.narrowings["min_not_mean"]["empty"], "With no parts, L1.");
        assert_eq!(req.narrowings["min_not_mean"].len(), 2);
        assert!(!req.narrowings.contains_key("weakest_link"));
        assert_eq!(
            req.clause_keys(),
            vec![Some("min_not_mean".to_string()), Some("weakest_link".to_string())]
        );
        assert_eq!(
            req.clause_text_with_narrowings("min_not_mean").unwrap(),
            "The minimum of its parts.\n  empty: With no parts, L1.\n  order: Ascending id."
        );
    }

    /// @tests REQ-REQDOC.narrowings_nest
    /// @tests REQ-REQDOC.malformed_reported
    #[test]
    fn a_block_without_text_is_reported_where_it_starts() {
        let doc = "# REQ-R: R\n\n## Clauses\n\n- **one**:\n  - empty: With none, L1.\n- **two**: Fine.\n";
        assert_eq!(kinds(doc), vec![(5, FrontmatterKind::MissingText)]);
        let doc = "# REQ-R: R\n\n## Clauses\n\n- **one**:\n";
        assert_eq!(kinds(doc), vec![(5, FrontmatterKind::MissingText)]);
    }

    /// @tests REQ-REQDOC.clause_unique
    #[test]
    fn a_clause_or_narrowing_given_twice_is_reported() {
        let doc = "# REQ-R: T\n\n## Clauses\n\n- **one**: First.\n- **one**: Again.\n";
        assert_eq!(kinds(doc), vec![(6, FrontmatterKind::DuplicateClause)]);
        let doc = "# REQ-R: T\n\n## Clauses\n\n- **one**: T.\n  - empty: A.\n  - empty: B.\n";
        assert_eq!(
            kinds(doc),
            vec![(7, FrontmatterKind::DuplicateNarrowing)]
        );
    }

    /// @tests REQ-REQDOC.malformed_reported
    #[test]
    fn a_clause_without_text_is_reported_and_other_markdown_is_not_a_requirement() {
        assert_eq!(
            kinds("# REQ-X: T\n\n## Clauses\n\n- **one**:\n"),
            vec![(5, FrontmatterKind::MissingText)]
        );
        assert_eq!(kinds("# Notes\n\nSome prose.\n"), vec![]);
        assert_eq!(kinds("---\nadr: 3\ntitle: A decision\n---\n"), vec![]);
    }

    /// @tests REQ-REQDOC.malformed_reported
    #[test]
    fn a_key_an_annotation_cannot_name_is_reported() {
        let doc = "# REQ-R: T\n\n## Clauses\n\n- **weakest-link**: T.\n- **one**: T.\n  - two words: N.\n";
        assert_eq!(
            kinds(doc),
            vec![(5, FrontmatterKind::InvalidKey), (7, FrontmatterKind::InvalidKey)]
        );
    }

    /// @tests REQ-REQDOC.clause_addressable
    /// @tests REQ-STALE.requirement_reopens_all
    #[test]
    fn a_clause_hash_moves_with_that_clause_only() {
        let a = parse(BLOCK);
        let reworded = parse(&BLOCK.replace("The minimum over bonds.", "The least over bonds."));
        let narrowed = parse(&BLOCK.replace("With no parts, L1.", "With no parts, L0."));
        let prose = parse(&BLOCK.replace("Prose.", "Other prose."));
        let hash = |r: &Requirement, c: &str| r.clause_hash(Some(c)).unwrap();
        assert_ne!(hash(&a, "weakest_link"), hash(&reworded, "weakest_link"));
        assert_eq!(hash(&a, "min_not_mean"), hash(&reworded, "min_not_mean"));
        assert_ne!(hash(&a, "min_not_mean"), hash(&narrowed, "min_not_mean"));
        assert_eq!(hash(&a, "weakest_link"), hash(&narrowed, "weakest_link"));
        assert_eq!(hash(&a, "weakest_link"), hash(&prose, "weakest_link"));
        assert_ne!(a.content_hash, prose.content_hash, "a document about it is reviewed against the prose");
        assert_eq!(a.clause_hash(Some("absent")), None);
        let bare = parse("# REQ-X: X\n\nbody\n");
        assert_eq!(bare.clause_hash(None), Some(bare.content_hash.clone()));
    }

    fn graph(edges: &[(&str, &[&str])]) -> BTreeMap<String, Requirement> {
        edges
            .iter()
            .map(|(id, parents)| {
                let doc = format!(
                    "# {id}: T\n\n- refines: {}\n\nbody\n",
                    parents.join(", ")
                );
                (id.to_string(), parse(&doc))
            })
            .collect()
    }

    /// @tests REQ-REQDOC.refines_dag
    #[test]
    fn a_cycle_is_found() {
        let g = graph(&[("REQ-A", &["REQ-B"]), ("REQ-B", &["REQ-C"]), ("REQ-C", &["REQ-A"])]);
        let cycle = refinement_cycle(&g).expect("a cycle");
        assert!(cycle.len() >= 3, "{cycle:?}");
    }

    #[test]
    fn a_diamond_is_not_a_cycle() {
        let g = graph(&[("REQ-A", &["REQ-B", "REQ-C"]), ("REQ-B", &["REQ-D"]), ("REQ-C", &["REQ-D"]), ("REQ-D", &[])]);
        assert_eq!(refinement_cycle(&g), None);
    }

    #[test]
    fn a_self_refinement_is_a_cycle() {
        let g = graph(&[("REQ-A", &["REQ-A"])]);
        assert!(refinement_cycle(&g).is_some());
    }

    /// @tests REQ-REQDOC.refines_resolves
    #[test]
    fn dangling_parents_are_listed_and_are_not_cycles() {
        let g = graph(&[("REQ-A", &["REQ-MISSING"])]);
        assert_eq!(refinement_cycle(&g), None);
        assert_eq!(
            dangling_refines(&g).into_iter().collect::<Vec<_>>(),
            vec![("REQ-A".to_string(), "REQ-MISSING".to_string())]
        );
    }
}

/// @implements REQ-REQDOC.id_is_identity
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parsed {
    pub is_requirement: bool,
    pub id: String,
    pub title: String,
    pub refines: Vec<String>,
    pub decomposition: Decomposition,
    pub status: Status,
    #[serde(with = "crate::wire::pairs")]
    pub clauses: BTreeMap<String, String>,
    pub narrowings: Vec<(String, Vec<(String, String)>)>,
    pub addressable: Vec<Option<String>>,
    pub problems: Vec<(u32, FrontmatterKind)>,
}

/// @implements REQ-REQDOC.id_is_identity
/// @implements REQ-REQDOC.id_unique
/// @implements REQ-REQDOC.clauseless_uniform
/// @implements REQ-REQDOC.decomposition_claimed
/// @implements REQ-REQDOC.malformed_reported
/// @implements REQ-REQDOC.narrowings_nest
/// @implements REQ-REQDOC.clause_unique
/// @drt REQ-REQDOC.id_is_identity
/// @drt REQ-REQDOC.clauseless_uniform
/// @drt REQ-REQDOC.decomposition_claimed
/// @drt REQ-REQDOC.malformed_reported
/// @drt REQ-REQDOC.narrowings_nest
/// @drt REQ-REQDOC.clause_unique
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
            narrowings: Vec::new(),
            addressable: Vec::new(),
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
            narrowings: req
                .narrowings
                .iter()
                .map(|(clause, entries)| {
                    (clause.clone(), entries.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                })
                .collect(),
            addressable: req.clause_keys(),
            problems: problems.iter().map(|p| (p.line, p.kind)).collect(),
        },
    }
}
