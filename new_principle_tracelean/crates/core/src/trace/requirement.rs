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
    IndentedWithoutKey,
    NotAKeyValue,
    EmptyId,
    UnclosedFrontmatter,
    UnknownKey,
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
    let mut problems = Vec::new();

    let (frontmatter, body) = match split_frontmatter(content) {
        Fence::Absent => return ParseOutcome::NotARequirement(problems),
        Fence::Unclosed => {
            problems.push(FrontmatterProblem {
                file: file.to_string(),
                line: 1,
                kind: FrontmatterKind::UnclosedFrontmatter,
                message: "the frontmatter's `---` is never closed".into(),
            });
            return ParseOutcome::NotARequirement(problems);
        }
        Fence::Split(frontmatter, body) => (frontmatter, body),
    };
    let fields = parse_frontmatter(frontmatter, file, &mut problems);

    let is_requirement = fields.scalars.get("id").is_some_and(|id| !id.is_empty());
    for (line, key) in &fields.unknown {
        if is_requirement || KEYS.contains(&key.to_ascii_lowercase().as_str()) {
            problems.push(FrontmatterProblem {
                file: file.to_string(),
                line: *line,
                kind: FrontmatterKind::UnknownKey,
                message: format!("`{key}` is not a requirement frontmatter key"),
            });
        }
    }

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
    let narrowings: BTreeMap<String, BTreeMap<String, String>> =
        fields.narrowings.into_iter().filter(|(_, n)| !n.is_empty()).collect();
    let mut whole = clauses.clone();
    for (clause, entries) in &narrowings {
        for (key, text) in entries {
            whole.insert(format!("{clause}.{key}"), text.clone());
        }
    }
    let content_hash = super::hash::requirement(&whole, body);

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
            narrowings,
            status: fields.scalars.get("status").map(|s| Status::parse(s)).unwrap_or_default(),
            body: body.to_string(),
            content_hash,
        }),
        problems,
    )
}

enum Fence<'a> {
    Absent,
    Unclosed,
    Split(&'a str, &'a str),
}

fn split_frontmatter(content: &str) -> Fence<'_> {
    let Some(rest) = content.strip_prefix("---\n").or_else(|| content.strip_prefix("---\r\n")) else {
        return Fence::Absent;
    };
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']).trim_end() == "---" {
            return Fence::Split(&rest[..offset], &rest[offset + line.len()..]);
        }
        offset += line.len();
    }
    Fence::Unclosed
}

#[derive(Default)]
struct Fields {
    scalars: BTreeMap<String, String>,
    lists: BTreeMap<String, Vec<String>>,
    maps: BTreeMap<String, BTreeMap<String, String>>,
    narrowings: BTreeMap<String, BTreeMap<String, String>>,
    unknown: Vec<(u32, String)>,
}

struct Block {
    clause: String,
    indent: usize,
    line: u32,
    has_text: bool,
}

fn problem(problems: &mut Vec<FrontmatterProblem>, file: &str, line: u32, kind: FrontmatterKind, message: String) {
    problems.push(FrontmatterProblem { file: file.to_string(), line, kind, message });
}

fn close_block(fields: &Fields, block: &mut Option<Block>, file: &str, problems: &mut Vec<FrontmatterProblem>) {
    let Some(open) = block.take() else { return };
    let text = fields.maps.get("clauses").and_then(|m| m.get(&open.clause));
    if !matches!(text, Some(t) if !t.is_empty()) {
        problem(
            problems,
            file,
            open.line,
            FrontmatterKind::MissingText,
            format!("the clause `{}` is a block without `text:`", open.clause),
        );
    }
}

fn read_narrowing(
    fields: &mut Fields,
    open: &mut Block,
    line_no: u32,
    line: &str,
    file: &str,
    problems: &mut Vec<FrontmatterProblem>,
) {
    let Some((key, value)) = line.split_once(':') else {
        problem(problems, file, line_no, FrontmatterKind::NotAKeyValue, format!("`{line}` is not a `key: value` entry"));
        return;
    };
    let (key, value) = (key.trim(), value.trim());
    if !valid_key(key) {
        problem(problems, file, line_no, FrontmatterKind::InvalidKey, format!("`{key}` is not a valid narrowing key"));
    }
    if key == "text" {
        if open.has_text {
            problem(problems, file, line_no, FrontmatterKind::DuplicateNarrowing, format!("`{}` has two `text:` lines", open.clause));
        }
        open.has_text = true;
        fields.maps.entry("clauses".into()).or_default().insert(open.clause.clone(), value.to_string());
        return;
    }
    let entries = fields.narrowings.entry(open.clause.clone()).or_default();
    if entries.contains_key(key) {
        problem(problems, file, line_no, FrontmatterKind::DuplicateNarrowing, format!("`{}.{key}` is given twice", open.clause));
    }
    entries.insert(key.to_string(), value.to_string());
}

#[allow(clippy::too_many_arguments)]
fn read_entry(
    fields: &mut Fields,
    block: &mut Option<Block>,
    map_key: &str,
    indent: usize,
    line_no: u32,
    line: &str,
    file: &str,
    problems: &mut Vec<FrontmatterProblem>,
) {
    let Some((key, value)) = line.split_once(':') else {
        problem(problems, file, line_no, FrontmatterKind::NotAKeyValue, format!("`{line}` is not a `key: value` entry"));
        return;
    };
    let (key, value) = (key.trim(), value.trim());
    if map_key != "clauses" {
        fields.maps.entry(map_key.to_string()).or_default().insert(key.to_string(), value.to_string());
        return;
    }
    if !valid_key(key) {
        problem(problems, file, line_no, FrontmatterKind::InvalidKey, format!("`{key}` is not a valid clause key"));
    }
    let clauses = fields.maps.entry("clauses".into()).or_default();
    if clauses.contains_key(key) {
        problem(problems, file, line_no, FrontmatterKind::DuplicateClause, format!("the clause `{key}` is declared twice"));
    }
    clauses.insert(key.to_string(), value.to_string());
    fields.narrowings.remove(key);
    if value.is_empty() {
        *block = Some(Block { clause: key.to_string(), indent, line: line_no, has_text: false });
    }
}

fn parse_frontmatter(text: &str, file: &str, problems: &mut Vec<FrontmatterProblem>) -> Fields {
    let mut fields = Fields::default();
    let mut current_map: Option<String> = None;
    let mut block: Option<Block> = None;

    for (index, raw) in text.lines().enumerate() {
        let line_no = index as u32 + 2;
        let line = raw.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        if indent > 0 {
            let Some(map_key) = current_map.clone() else {
                problem(problems, file, line_no, FrontmatterKind::IndentedWithoutKey, "indented entry without a key above it".into());
                continue;
            };
            if let Some(open) = block.as_mut() {
                if indent > open.indent {
                    read_narrowing(&mut fields, open, line_no, line.trim(), file, problems);
                    continue;
                }
            }
            close_block(&fields, &mut block, file, problems);
            read_entry(&mut fields, &mut block, &map_key, indent, line_no, line.trim(), file, problems);
            continue;
        }

        close_block(&fields, &mut block, file, problems);
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
        if !KEYS.contains(&key.as_str()) {
            fields.unknown.push((line_no, key.clone()));
        }

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
    close_block(&fields, &mut block, file, problems);

    fields
}

fn first_heading(body: &str) -> Option<String> {
    body.lines()
        .find(|line| line.starts_with("# "))
        .map(|line| line[2..].trim().to_string())
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

    const BLOCK: &str = "---\nid: REQ-R\nclauses:\n  weakest_link: The minimum over bonds.\n  min_not_mean:\n    text: The minimum of its parts.\n    empty: With no parts, L1.\n    order: Ascending id.\n---\nProse.\n";

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
        let doc = "---\nid: REQ-R\nclauses:\n  one:\n    empty: With none, L1.\n  two: Fine.\n---\n";
        assert_eq!(kinds(doc), vec![(4, FrontmatterKind::MissingText)]);
        let doc = "---\nid: REQ-R\nclauses:\n  one:\n---\n";
        assert_eq!(kinds(doc), vec![(4, FrontmatterKind::MissingText)]);
    }

    /// @tests REQ-REQDOC.clause_unique
    #[test]
    fn a_clause_or_narrowing_given_twice_is_reported() {
        let doc = "---\nid: REQ-R\nclauses:\n  one: First.\n  one: Again.\n---\n";
        assert_eq!(kinds(doc), vec![(5, FrontmatterKind::DuplicateClause)]);
        let doc = "---\nid: REQ-R\nclauses:\n  one:\n    text: T.\n    empty: A.\n    empty: B.\n    text: U.\n---\n";
        assert_eq!(
            kinds(doc),
            vec![(7, FrontmatterKind::DuplicateNarrowing), (8, FrontmatterKind::DuplicateNarrowing)]
        );
    }

    /// @tests REQ-REQDOC.malformed_reported
    #[test]
    fn an_unclosed_fence_and_an_unknown_key_are_reported() {
        assert_eq!(
            kinds("---\nid: REQ-X\nclauses:\n  one: t\n"),
            vec![(1, FrontmatterKind::UnclosedFrontmatter)]
        );
        assert_eq!(kinds("---\niD: REQ-X\n---\n"), vec![(2, FrontmatterKind::UnknownKey)]);
        assert_eq!(kinds("---\nid: REQ-X\nclauess:\n---\n"), vec![(3, FrontmatterKind::UnknownKey)]);
        assert_eq!(kinds("---\nadr: 3\ntitle: A decision\n---\n"), vec![]);
        assert_eq!(kinds("# Notes\n---\n"), vec![]);
    }

    /// @tests REQ-REQDOC.malformed_reported
    #[test]
    fn a_key_an_annotation_cannot_name_is_reported() {
        let doc = "---\nid: REQ-R\nclauses:\n  weakest-link: T.\n  one:\n    text: T.\n    two words: N.\n---\n";
        assert_eq!(
            kinds(doc),
            vec![(4, FrontmatterKind::InvalidKey), (7, FrontmatterKind::InvalidKey)]
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
        let bare = parse("---\nid: REQ-X\n---\nbody");
        assert_eq!(bare.clause_hash(None), Some(bare.content_hash.clone()));
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
