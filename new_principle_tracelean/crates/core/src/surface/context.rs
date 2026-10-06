//! What an agent needs to see to change a requirement: its context.
//!
//! From a requirement or a clause, the trace already knows the neighbourhood
//! an agent never reads — what the clause refines and what refines it, the
//! code that implements it, the tests that pin it, the model that says what it
//! means, and the tests that may break. This gathers it, lets the person
//! choose which parts go, and writes the chosen parts as one prompt to copy.
//! Nothing is sent (`REQ-CONTEXT.copied_not_sent`): the person carries it.
//!
//! Pure: the index and the files' text come in, a value goes out.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::surface::sandbox_view::Lines;
use crate::surface::view::{Buffer, Role};
use crate::trace::anchor::AnchorKind;
use crate::trace::annotation::Role as Claimed;
use crate::trace::index::{Index, Link};

/// A requirement as the refinement graph sees it: its name and its parents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub refines: Vec<String>,
}

/// Everything reachable from `id` one `next` step at a time, without `id`,
/// each once, in name order.
fn closure(id: &str, next: impl Fn(&str) -> Vec<String>) -> Vec<String> {
    let mut seen: BTreeSet<String> = BTreeSet::from([id.to_string()]);
    let mut queue = vec![id.to_string()];
    while let Some(at) = queue.pop() {
        for found in next(&at) {
            if seen.insert(found.clone()) {
                queue.push(found);
            }
        }
    }
    seen.remove(id);
    seen.into_iter().collect()
}

/// Everything `id` refines, transitively.
///
/// @implements REQ-CONTEXT.neighbourhood_is_closed
/// @drt REQ-CONTEXT.neighbourhood_is_closed
pub fn ancestors(nodes: Vec<Node>, id: String) -> Vec<String> {
    closure(&id, |at| nodes.iter().filter(|n| n.id == at).flat_map(|n| n.refines.clone()).collect())
}

/// Everything that refines `id`, transitively.
///
/// @implements REQ-CONTEXT.neighbourhood_is_closed
/// @drt REQ-CONTEXT.neighbourhood_is_closed
pub fn descendants(nodes: Vec<Node>, id: String) -> Vec<String> {
    closure(&id, |at| nodes.iter().filter(|n| n.refines.iter().any(|p| p == at)).map(|n| n.id.clone()).collect())
}

/// One part of a context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Part {
    Requirement,
    Refines,
    RefinedBy,
    Code,
    Tests,
    Models,
    Affected,
}

/// The parts, in the one order they are shown and copied in.
pub const ALL_PARTS: [Part; 7] =
    [Part::Requirement, Part::Refines, Part::RefinedBy, Part::Code, Part::Tests, Part::Models, Part::Affected];

impl Part {
    /// What the view calls it.
    pub fn label(self) -> &'static str {
        match self {
            Part::Requirement => "requirement",
            Part::Refines => "refines",
            Part::RefinedBy => "refined by",
            Part::Code => "code",
            Part::Tests => "tests",
            Part::Models => "models",
            Part::Affected => "affected tests",
        }
    }
}

/// The part a label names, if it names one.
///
/// @implements REQ-CONTEXT.part_named
/// @drt REQ-CONTEXT.part_named
pub fn part_named(name: String) -> Option<Part> {
    ALL_PARTS.into_iter().find(|p| p.label() == name)
}

/// The parts the copied text holds: those included that have something in
/// them, in the fixed order.
///
/// @implements REQ-CONTEXT.person_chooses
/// @drt REQ-CONTEXT.person_chooses
pub fn parts_shown(included: Vec<Part>, filled: Vec<Part>) -> Vec<Part> {
    ALL_PARTS.into_iter().filter(|p| included.contains(p) && filled.contains(p)).collect()
}

/// What a part holds when nobody has chosen: everything but what refines the
/// target, which in a large tree is most of it.
pub fn default_parts() -> BTreeSet<Part> {
    ALL_PARTS.into_iter().filter(|p| *p != Part::RefinedBy).collect()
}

/// A requirement as the context says it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Said {
    pub id: String,
    pub title: String,
    pub file: String,
    /// Clause key and text, in key order.
    pub clauses: Vec<(String, String)>,
}

/// A claimed item, or a line that names one: where it is and its source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// `implements`, `tests`, `models`, `proves`, `drt`; `uses` for a test
    /// line that names an implementing item without claiming anything.
    pub role: String,
    /// The clause claimed, as `REQ-X.clause`.
    pub claims: String,
    pub path: String,
    /// One-based.
    pub line: u32,
    pub symbol: Option<String>,
    pub source: String,
}

/// Everything an agent may be shown about one requirement or clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    /// `REQ-X` or `REQ-X.clause`, as asked for.
    pub target: String,
    /// The clause, when one was asked for.
    pub clause: Option<String>,
    pub requirement: Said,
    pub refines: Vec<Said>,
    pub refined_by: Vec<Said>,
    pub code: Vec<Item>,
    pub tests: Vec<Item>,
    pub models: Vec<Item>,
    pub affected: Vec<Item>,
}

impl Context {
    /// The parts with something in them.
    pub fn filled(&self) -> Vec<Part> {
        ALL_PARTS
            .into_iter()
            .filter(|p| match p {
                Part::Requirement => true,
                Part::Refines => !self.refines.is_empty(),
                Part::RefinedBy => !self.refined_by.is_empty(),
                Part::Code => !self.code.is_empty(),
                Part::Tests => !self.tests.is_empty(),
                Part::Models => !self.models.is_empty(),
                Part::Affected => !self.affected.is_empty(),
            })
            .collect()
    }
}

/// The most source an item contributes, so one long function cannot crowd
/// out the rest.
const MOST_LINES: usize = 60;

/// The source of the item a link sits on.
fn source_of(link: &Link, files: &BTreeMap<String, String>) -> String {
    let Some(text) = files.get(&link.anchor.file) else { return String::new() };
    let lines: Vec<&str> = text.lines().collect();
    // A declaration's rows count from zero; anything else, from the
    // annotation's own line, which counts from one.
    let (from, to) = match link.anchor.kind {
        AnchorKind::Decl { .. } => (link.anchor.start_line as usize, link.anchor.end_line as usize),
        _ => {
            let from = (link.line as usize).saturating_sub(1);
            (from, from + 20)
        }
    };
    let to = to.min(lines.len().saturating_sub(1)).min(from + MOST_LINES - 1);
    if from >= lines.len() {
        return String::new();
    }
    lines[from..=to].join("\n")
}

fn said(index: &Index, id: &str) -> Option<Said> {
    let r = index.requirements.get(id)?;
    Some(Said {
        id: r.id.clone(),
        title: r.title.clone(),
        file: r.file.clone(),
        clauses: r.clauses.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
    })
}

fn item(link: &Link, files: &BTreeMap<String, String>) -> Item {
    Item {
        role: link.role.as_str().to_string(),
        claims: match &link.clause {
            Some(clause) => format!("{}.{clause}", link.req_id),
            None => link.req_id.clone(),
        },
        path: link.anchor.file.clone(),
        line: link.line,
        symbol: match &link.anchor.kind {
            AnchorKind::Decl { symbol_path } => Some(symbol_path.clone()),
            _ => None,
        },
        source: source_of(link, files),
    }
}

/// Whether `text` names `word` as a whole word.
fn names(text: &str, word: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

/// A path that holds tests by where it is or what it is called.
fn is_test_path(path: &str) -> bool {
    path.split('/').any(|part| part == "tests" || part == "test" || part.contains("_test") || part.starts_with("test_"))
}

/// The most lines named outside any claim that an affected list carries.
const MOST_USES: usize = 20;

/// The context of `target` — `REQ-X` or `REQ-X.clause` — or nothing when no
/// requirement is called that.
///
/// @implements REQ-CONTEXT.claims_with_source
/// @implements REQ-CONTEXT.affected_tests
pub fn gather(target: &str, index: &Index, files: &BTreeMap<String, String>) -> Option<Context> {
    let (id, clause) = match target.split_once('.') {
        Some((id, clause)) => (id, Some(clause.to_string())),
        None => (target, None),
    };
    let requirement = said(index, id)?;
    let nodes: Vec<Node> =
        index.requirements.values().map(|r| Node { id: r.id.clone(), refines: r.refines.clone() }).collect();
    let up = ancestors(nodes.clone(), id.to_string());
    let down = descendants(nodes, id.to_string());

    let on_target = |link: &&Link| link.req_id == id && (clause.is_none() || link.clause == clause);
    let claims: Vec<&Link> = index.links.iter().filter(on_target).collect();
    // One item an anchor: an item claiming two of the clauses is shown once,
    // naming both.
    let of = |roles: &[Claimed]| -> Vec<Item> {
        let mut out: Vec<(String, Item)> = Vec::new();
        for link in claims.iter().filter(|l| roles.contains(&l.role)) {
            let found = item(link, files);
            match out.iter_mut().find(|(ident, _)| *ident == link.anchor.ident()) {
                Some((_, held)) if held.role == found.role => held.claims = format!("{}, {}", held.claims, found.claims),
                _ => out.push((link.anchor.ident(), found)),
            }
        }
        out.into_iter().map(|(_, item)| item).collect()
    };
    let code = of(&[Claimed::Implements]);
    let tests = of(&[Claimed::Tests]);
    let models = of(&[Claimed::Models, Claimed::Proves, Claimed::Drt, Claimed::Pins]);

    // Affected: tests claiming what refines the target, and tests naming
    // what implements it, past those that claim the target already.
    // An item is placed once, however many clauses its annotations name.
    let mut placed: BTreeSet<String> =
        claims.iter().filter(|l| l.role == Claimed::Tests).map(|l| l.anchor.ident()).collect();
    let mut affected = Vec::new();
    for link in index.links.iter().filter(|l| l.role == Claimed::Tests && down.contains(&l.req_id)) {
        if placed.insert(link.anchor.ident()) {
            affected.push(item(link, files));
        }
    }
    let symbols: BTreeSet<String> = code
        .iter()
        .filter_map(|c| c.symbol.as_deref())
        .map(|s| s.rsplit("::").next().unwrap_or(s).to_string())
        .collect();
    for link in index.links.iter().filter(|l| l.role == Claimed::Tests) {
        if placed.contains(&link.anchor.ident()) {
            continue;
        }
        let found = item(link, files);
        if symbols.iter().any(|s| names(&found.source, s)) {
            placed.insert(link.anchor.ident());
            affected.push(found);
        }
    }
    // A test nobody annotated still breaks: lines in test files that name an
    // implementing item.
    let claimed_files: BTreeSet<String> = affected.iter().chain(tests.iter()).map(|i| i.path.clone()).collect();
    let mut uses = 0;
    for (path, text) in files.iter().filter(|(p, _)| is_test_path(p) && !claimed_files.contains(*p)) {
        for (n, line) in text.lines().enumerate() {
            if uses < MOST_USES && symbols.iter().any(|s| names(line, s)) {
                affected.push(Item {
                    role: "uses".into(),
                    claims: String::new(),
                    path: path.clone(),
                    line: n as u32 + 1,
                    symbol: None,
                    source: line.trim().to_string(),
                });
                uses += 1;
            }
        }
    }

    Some(Context {
        target: target.to_string(),
        clause,
        requirement,
        refines: up.iter().filter_map(|r| said(index, r)).collect(),
        refined_by: down.iter().filter_map(|r| said(index, r)).collect(),
        code,
        tests,
        models,
        affected,
    })
}

/// The fence language for a path's source.
fn fence(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("rs") => "rust",
        Some("lean") => "lean",
        Some("ts") => "typescript",
        Some("py") => "python",
        Some("md") => "markdown",
        _ => "",
    }
}

fn write_said(out: &mut String, r: &Said, marked: Option<&str>) {
    out.push_str(&format!("### {} — {} (`{}`)\n", r.id, r.title, r.file));
    for (key, text) in &r.clauses {
        if Some(key.as_str()) == marked {
            out.push_str(&format!("- **{key}** (the clause being changed): {text}\n"));
        } else {
            out.push_str(&format!("- {key}: {text}\n"));
        }
    }
    out.push('\n');
}

fn write_items(out: &mut String, items: &[Item]) {
    for i in items {
        let name = i.symbol.as_deref().map(|s| format!(" `{s}`")).unwrap_or_default();
        let claims = if i.claims.is_empty() { String::new() } else { format!(" — {} {}", i.role, i.claims) };
        out.push_str(&format!("### {}:{}{name}{claims}\n```{}\n{}\n```\n\n", i.path, i.line, fence(&i.path), i.source));
    }
}

/// The chosen parts, as one prompt for an agent.
///
/// @implements REQ-CONTEXT.person_chooses
pub fn context_text(context: &Context, included: &BTreeSet<Part>) -> String {
    let mut out = format!("# Context for changing {}\n\n", context.target);
    out.push_str(
        "This is what the change touches: the requirement, how it relates to others, \
         and the code, tests and models that claim it. Keep every claim true, or say \
         which requirement has to change first.\n\n",
    );
    for part in parts_shown(included.iter().copied().collect(), context.filled()) {
        match part {
            Part::Requirement => {
                out.push_str("## The requirement\n\n");
                write_said(&mut out, &context.requirement, context.clause.as_deref());
            }
            Part::Refines => {
                out.push_str("## What it refines (it must keep meeting these)\n\n");
                context.refines.iter().for_each(|r| write_said(&mut out, r, None));
            }
            Part::RefinedBy => {
                out.push_str("## What refines it (these may have to change with it)\n\n");
                context.refined_by.iter().for_each(|r| write_said(&mut out, r, None));
            }
            Part::Code => {
                out.push_str("## Code that implements it\n\n");
                write_items(&mut out, &context.code);
            }
            Part::Tests => {
                out.push_str("## Tests that claim it\n\n");
                write_items(&mut out, &context.tests);
            }
            Part::Models => {
                out.push_str("## Lean models and proofs that claim it\n\n");
                write_items(&mut out, &context.models);
            }
            Part::Affected => {
                out.push_str("## Other tests that may break\n\n");
                write_items(&mut out, &context.affected);
            }
        }
    }
    out
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The context as a buffer, titled `context <target>`: a line a part, each
/// part's name the switch that includes or leaves it out, the button that
/// copies, and the text that would be copied.
pub fn context_view(context: &Context, included: &BTreeSet<Part>, width: usize) -> Buffer {
    let mut out = Lines::new(width);
    out.line(&[("Context for an agent", Role::Heading, &[])]);
    out.line(&[(&context.target, Role::Requirement, &["trace.requirement"]), ("  ", Role::Plain, &[]), (&context.requirement.title, Role::Plain, &[])]);
    out.wrapped("", "Choose what your agent sees — click a part to include it or leave it out — then copy it and paste it to the agent.", Role::Plain, &[]);
    out.blank();
    let names = |rs: &[Said]| rs.iter().map(|r| r.id.clone()).collect::<Vec<_>>().join(" ");
    for part in ALL_PARTS {
        let what = match part {
            Part::Requirement => match &context.clause {
                Some(clause) => format!("{} with {clause} marked", context.requirement.id),
                None => format!("{} and its clauses", context.requirement.id),
            },
            Part::Refines => count(context.refines.len(), "requirement", "requirements") + "  " + &names(&context.refines),
            Part::RefinedBy => count(context.refined_by.len(), "requirement", "requirements") + "  " + &names(&context.refined_by),
            Part::Code => count(context.code.len(), "item", "items"),
            Part::Tests => count(context.tests.len(), "test", "tests"),
            Part::Models => count(context.models.len(), "model or proof", "models and proofs"),
            Part::Affected => count(context.affected.len(), "place", "places"),
        };
        let mark = if included.contains(&part) { "[x] " } else { "[ ] " };
        let gap = " ".repeat(16usize.saturating_sub(part.label().chars().count()));
        let role = if included.contains(&part) { Role::Added } else { Role::Plain };
        out.line(&[(mark, role, &[]), (part.label(), Role::Entry, &["context.toggle"]), (&gap, Role::Plain, &[]), (what.trim_end(), Role::Plain, &[])]);
    }
    let text = context_text(context, included);
    out.blank();
    let size = format!("  {} characters", text.chars().count());
    out.line(&[("[ Copy for an agent ]", Role::Added, &["context.copy"]), (&size, Role::Plain, &[])]);
    out.blank();
    out.line(&[("What will be copied", Role::Heading, &[])]);
    for line in text.lines() {
        if line.is_empty() {
            out.blank();
        } else {
            out.line(&[(line, Role::Plain, &[])]);
        }
    }
    out.finish(&format!("context {}", context.target))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, refines: &[&str]) -> Node {
        Node { id: id.into(), refines: refines.iter().map(|s| s.to_string()).collect() }
    }

    /// @tests REQ-CONTEXT.neighbourhood_is_closed
    #[test]
    fn the_neighbourhood_is_transitive_each_once_and_without_the_target() {
        let nodes = vec![node("C", &["B"]), node("B", &["A"]), node("D", &["B", "A"]), node("A", &["A"])];
        assert_eq!(ancestors(nodes.clone(), "C".into()), vec!["A", "B"]);
        assert_eq!(descendants(nodes.clone(), "A".into()), vec!["B", "C", "D"]);
        assert!(descendants(nodes, "C".into()).is_empty());
    }

    /// @tests REQ-CONTEXT.person_chooses
    #[test]
    fn only_included_parts_with_something_in_them_are_copied_in_order() {
        let shown = parts_shown(vec![Part::Tests, Part::Code, Part::Refines], vec![Part::Requirement, Part::Code, Part::Tests]);
        assert_eq!(shown, vec![Part::Code, Part::Tests]);
    }

    /// @tests REQ-CONTEXT.part_named
    #[test]
    fn a_label_names_its_part_and_nothing_else_does() {
        for part in ALL_PARTS {
            assert_eq!(part_named(part.label().into()), Some(part));
        }
        assert_eq!(part_named("everything".into()), None);
    }

    #[test]
    fn a_name_is_found_as_a_whole_word() {
        assert!(names("assert!(to_celsius(1.0) > 0.0)", "to_celsius"));
        assert!(!names("to_celsius_fast(1.0)", "to_celsius"));
    }
}
