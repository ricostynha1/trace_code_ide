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
pub fn ancestors(nodes: Vec<Node>, id: String) -> Vec<String> {
    closure(&id, |at| nodes.iter().filter(|n| n.id == at).flat_map(|n| n.refines.clone()).collect())
}

/// Everything that refines `id`, transitively.
pub fn descendants(nodes: Vec<Node>, id: String) -> Vec<String> {
    closure(&id, |at| nodes.iter().filter(|n| n.refines.iter().any(|p| p == at)).map(|n| n.id.clone()).collect())
}

/// The neighbourhood of `id`: what it refines and what refines it, each
/// transitively, each once, in name order, and never `id` itself.
///
/// @implements REQ-CONTEXT.neighbourhood_is_closed
/// @drt REQ-CONTEXT.neighbourhood_is_closed
pub fn neighbourhood(nodes: Vec<Node>, id: String) -> (Vec<String>, Vec<String>) {
    (ancestors(nodes.clone(), id.clone()), descendants(nodes, id))
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

/// What a shell's `--parts` chose: the parts, in their fixed order and each
/// once, or the first name that is no part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShellParts {
    Chosen { parts: Vec<Part> },
    Unknown { name: String },
}

/// Space, tab, carriage return and line feed: what is trimmed from a name or a
/// line. Written out so the model trims the same characters.
fn blank(c: char) -> bool {
    c == ' ' || c == '\t' || c == '\r' || c == '\n'
}

/// The parts a shell asked for: `parts` is a comma-separated list of labels
/// (a dash for a space: `refined-by`), or `all`, or nothing for the default.
/// The first label that names no part is refused.
///
/// @implements REQ-CONTEXT.from_the_shell
pub fn shell_parts(parts: Option<String>) -> ShellParts {
    let chosen: BTreeSet<Part> = match parts.as_deref() {
        None => default_parts(),
        Some("all") => ALL_PARTS.into_iter().collect(),
        Some(list) => {
            let mut chosen = BTreeSet::new();
            for name in list.split(',') {
                let name = name.trim_matches(blank).replace('-', " ");
                match part_named(name.clone()) {
                    Some(part) => {
                        chosen.insert(part);
                    }
                    None => return ShellParts::Unknown { name },
                }
            }
            chosen
        }
    };
    ShellParts::Chosen { parts: chosen.into_iter().collect() }
}

/// The context for `target` as an agent asks for it from a shell, with the
/// parts `shell_parts` chose. A part or a requirement that does not exist is
/// refused with what does.
///
/// @implements REQ-CONTEXT.from_the_shell
pub fn for_the_shell(
    target: &str,
    parts: Option<&str>,
    index: &Index,
    files: &BTreeMap<String, String>,
) -> Result<String, String> {
    let included: BTreeSet<Part> = match shell_parts(parts.map(str::to_string)) {
        ShellParts::Chosen { parts } => parts.into_iter().collect(),
        ShellParts::Unknown { name } => {
            let known: Vec<String> = ALL_PARTS.iter().map(|p| p.label().replace(' ', "-")).collect();
            return Err(format!("no part is called `{name}`; the parts are {}", known.join(", ")));
        }
    };
    let context = gather(target, index, files).ok_or_else(|| format!("no requirement is called `{target}`"))?;
    Ok(context_text(&context, &included))
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
        // A clause's narrowings are part of it, so they are written with it,
        // each on an indented line under the clause.
        clauses: r
            .clauses
            .iter()
            .map(|(k, v)| (k.clone(), r.clause_text_with_narrowings(k).unwrap_or_else(|| v.clone())))
            .collect(),
    })
}

/// A claim as the context reads it: one annotation, flattened, with the
/// source of the item it sits on already cut from its file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    /// `implements`, `tests`, `models`, `proves`, `drt`, `pins`, `specifies`.
    pub role: String,
    pub req: String,
    pub clause: Option<String>,
    /// The anchor's identity: one item, however many annotations it carries.
    pub ident: String,
    pub path: String,
    /// One-based.
    pub line: u32,
    pub symbol: Option<String>,
    pub source: String,
}

fn claim(link: &Link, files: &BTreeMap<String, String>) -> Claim {
    Claim {
        role: link.role.as_str().to_string(),
        req: link.req_id.clone(),
        clause: link.clause.clone(),
        ident: link.anchor.ident(),
        path: link.anchor.file.clone(),
        line: link.line,
        symbol: match &link.anchor.kind {
            AnchorKind::Decl { symbol_path } => Some(symbol_path.clone()),
            _ => None,
        },
        source: source_of(link, files),
    }
}

fn item(claim: &Claim) -> Item {
    Item {
        role: claim.role.clone(),
        claims: match &claim.clause {
            Some(clause) => format!("{}.{clause}", claim.req),
            None => claim.req.clone(),
        },
        path: claim.path.clone(),
        line: claim.line,
        symbol: claim.symbol.clone(),
        source: claim.source.clone(),
    }
}

/// Whether a claim is on the target: its requirement, and its clause when
/// one was asked for.
fn on_target(claim: &Claim, id: &str, clause: &Option<String>) -> bool {
    claim.req == id && (clause.is_none() || claim.clause == *clause)
}

/// The claims on the target with one of `roles`, an item an anchor: an item
/// claiming two of the clauses is shown once, naming both — merged into the
/// first item of its anchor when that one has the same role.
fn of_roles(claims: &[Claim], id: &str, clause: &Option<String>, roles: &[&str]) -> Vec<Item> {
    let mut out: Vec<(String, Item)> = Vec::new();
    for claim in claims.iter().filter(|c| on_target(c, id, clause) && roles.contains(&c.role.as_str())) {
        let found = item(claim);
        match out.iter_mut().find(|(ident, _)| *ident == claim.ident) {
            Some((_, held)) if held.role == found.role => held.claims = format!("{}, {}", held.claims, found.claims),
            _ => out.push((claim.ident.clone(), found)),
        }
    }
    out.into_iter().map(|(_, item)| item).collect()
}

/// What claims a target, by kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claimed {
    pub code: Vec<Item>,
    pub tests: Vec<Item>,
    pub models: Vec<Item>,
}

/// Every claim on `id` (and `clause`, when one was asked for), each with its
/// place and source: implementations, tests, and models with their proofs.
///
/// @implements REQ-CONTEXT.claims_with_source
pub fn claims_on(claims: Vec<Claim>, id: String, clause: Option<String>) -> Claimed {
    Claimed {
        code: of_roles(&claims, &id, &clause, &["implements"]),
        tests: of_roles(&claims, &id, &clause, &["tests"]),
        models: of_roles(&claims, &id, &clause, &["models", "proves", "drt", "pins"]),
    }
}

/// A letter, a digit or an underscore, ASCII only so the model agrees.
fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether `text` names `word` as a whole word. Matches are taken left to
/// right without overlapping, as `str::match_indices` takes them.
fn names(text: &str, word: &str) -> bool {
    let text: Vec<char> = text.chars().collect();
    let word: Vec<char> = word.chars().collect();
    let mut at = 0;
    while at <= text.len() {
        if text[at..].starts_with(&word) {
            let before = if at == 0 { None } else { Some(text[at - 1]) };
            let after = text.get(at + word.len()).copied();
            if !before.is_some_and(is_word) && !after.is_some_and(is_word) {
                return true;
            }
            at += word.len().max(1);
        } else {
            at += 1;
        }
    }
    false
}

/// A path that holds tests by where it is or what it is called.
fn is_test_path(path: &str) -> bool {
    path.split('/').any(|part| part == "tests" || part == "test" || part.contains("_test") || part.starts_with("test_"))
}

/// A text's lines: split at line feeds, no line after a final one, and a
/// carriage return before a line feed dropped.
fn lines_of(text: &str) -> Vec<String> {
    let mut pieces: Vec<&str> = text.split('\n').collect();
    let ended = pieces.last() == Some(&"");
    if ended {
        pieces.pop();
    }
    let last = pieces.len();
    pieces
        .into_iter()
        .enumerate()
        .map(|(n, piece)| {
            let followed = n + 1 < last || ended;
            if followed { piece.strip_suffix('\r').unwrap_or(piece).to_string() } else { piece.to_string() }
        })
        .collect()
}

/// The most lines named outside any claim that an affected list carries.
const MOST_USES: usize = 20;

/// The tests a change to the target may break, past those that claim it: tests
/// claiming what refines it (`down`), tests whose source names an item that
/// implements it, and — for a test nobody annotated — lines of test files
/// that name one, at most `MOST_USES` of them. An item is placed once.
///
/// @implements REQ-CONTEXT.affected_tests
pub fn affected(
    claims: Vec<Claim>,
    id: String,
    clause: Option<String>,
    down: Vec<String>,
    files: Vec<(String, String)>,
) -> Vec<Item> {
    let claimed = claims_on(claims.clone(), id.clone(), clause.clone());
    let mut placed: BTreeSet<String> =
        claims.iter().filter(|c| c.role == "tests" && on_target(c, &id, &clause)).map(|c| c.ident.clone()).collect();
    let mut out = Vec::new();
    for claim in claims.iter().filter(|c| c.role == "tests" && down.contains(&c.req)) {
        if placed.insert(claim.ident.clone()) {
            out.push(item(claim));
        }
    }
    let symbols: Vec<String> = claimed
        .code
        .iter()
        .filter_map(|c| c.symbol.as_deref())
        .map(|s| s.rsplit("::").next().unwrap_or(s).to_string())
        .collect();
    for claim in claims.iter().filter(|c| c.role == "tests") {
        if placed.contains(&claim.ident) {
            continue;
        }
        if symbols.iter().any(|s| names(&claim.source, s)) {
            placed.insert(claim.ident.clone());
            out.push(item(claim));
        }
    }
    let claimed_files: Vec<String> = out.iter().chain(claimed.tests.iter()).map(|i| i.path.clone()).collect();
    let mut uses = 0;
    for (path, text) in files.iter().filter(|(p, _)| is_test_path(p) && !claimed_files.contains(p)) {
        for (n, line) in lines_of(text).iter().enumerate() {
            if uses < MOST_USES && symbols.iter().any(|s| names(line, s)) {
                out.push(Item {
                    role: "uses".into(),
                    claims: String::new(),
                    path: path.clone(),
                    line: n as u32 + 1,
                    symbol: None,
                    source: line.trim_matches(blank).to_string(),
                });
                uses += 1;
            }
        }
    }
    out
}

/// The context of `target` — `REQ-X` or `REQ-X.clause` — or nothing when no
/// requirement is called that. The shell around `neighbourhood`, `claims_on`
/// and `affected`: it reads the index and cuts each claim's source.
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
    let (up, down) = neighbourhood(nodes, id.to_string());
    let claims: Vec<Claim> = index.links.iter().map(|link| claim(link, files)).collect();
    let Claimed { code, tests, models } = claims_on(claims.clone(), id.to_string(), clause.clone());
    let files: Vec<(String, String)> = files.iter().map(|(p, t)| (p.clone(), t.clone())).collect();
    let affected = affected(claims, id.to_string(), clause.clone(), down.clone(), files);

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

/// Each clause in view that lacks code, a test or a Lean model, as
/// `key: no test, no Lean model` — the work a change could also do.
fn gaps(context: &Context) -> Vec<String> {
    let id = &context.requirement.id;
    let claimed = |items: &[Item], name: &str| items.iter().any(|i| i.claims.split(", ").any(|c| c == name));
    context
        .requirement
        .clauses
        .iter()
        .filter(|(key, _)| context.clause.as_deref().map_or(true, |c| c == key))
        .filter_map(|(key, _)| {
            let name = format!("{id}.{key}");
            let missing: Vec<&str> = [
                (&context.code, "no code implements it"),
                (&context.tests, "no test"),
                (&context.models, "no Lean model"),
            ]
            .into_iter()
            .filter(|(items, _)| !claimed(items, &name))
            .map(|(_, what)| what)
            .collect();
            (!missing.is_empty()).then(|| format!("{key}: {}", missing.join(", ")))
        })
        .collect()
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
                let gaps = gaps(context);
                if !gaps.is_empty() {
                    out.push_str("## Not yet met (what the change could add)\n\n");
                    gaps.iter().for_each(|g| out.push_str(&format!("- {g}\n")));
                    out.push('\n');
                }
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
    // As Markdown, which it is: headings, code and requirement names marked.
    let text = text.trim_end();
    out.marked(text, crate::surface::highlight::markdown(text));
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
